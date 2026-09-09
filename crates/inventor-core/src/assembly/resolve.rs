//! Offline filesystem resolution and bounded expansion of the saved graph.
use super::{compose, digest, inspect, AssemblyDocument, Matrix, IDENTITY};
use crate::{Error, Limits, Result};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct ResolveOptions {
    pub search_roots: Vec<PathBuf>,
    pub max_documents: usize,
    pub max_instances: usize,
    pub max_depth: usize,
    pub max_total_file_bytes: usize,
    pub max_directory_entries: usize,
}
impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            search_roots: vec![],
            max_documents: 256,
            max_instances: 10000,
            max_depth: 32,
            max_total_file_bytes: 512 * 1024 * 1024,
            max_directory_entries: 100000,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceResolution {
    pub reference_id: u32,
    pub saved_path: String,
    pub status: String,
    pub candidates: Vec<String>,
    pub definition: Option<usize>,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Definition {
    pub key: String,
    pub path: String,
    pub source_paths: Vec<String>,
    pub document: AssemblyDocument,
    pub references: Vec<ReferenceResolution>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Instance {
    pub parent: Option<usize>,
    pub owner_definition: usize,
    pub occurrence_index: usize,
    pub occurrence_id: u32,
    pub path: Vec<u32>,
    pub name: Option<String>,
    pub definition: Option<usize>,
    pub resolution: String,
    pub suppressed: Option<bool>,
    pub visible: Option<bool>,
    pub substitute: Option<bool>,
    pub local_transform_mm: Option<Matrix>,
    pub world_transform_mm: Option<Matrix>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AssemblyGraph {
    pub api_version: u32,
    pub root: usize,
    pub definitions: Vec<Definition>,
    pub instances: Vec<Instance>,
    pub search_roots: Vec<String>,
    pub structure_status: String,
    pub current_state: String,
    pub complete: bool,
    pub diagnostics: Vec<String>,
}

struct Loader<'a> {
    limits: &'a Limits,
    options: &'a ResolveOptions,
    roots: Vec<PathBuf>,
    definitions: Vec<Definition>,
    keys: HashMap<String, usize>,
    paths: HashMap<PathBuf, usize>,
    expanded: HashSet<usize>,
    bytes_left: usize,
    entries_left: usize,
}

fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn inside(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}
fn text_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Loader<'_> {
    // Resolve each path component with Windows-style case folding. An exact
    // spelling cannot silently win over a second case alias on a case-sensitive FS.
    fn case_candidates(&mut self, candidate: &Path) -> Result<Vec<PathBuf>> {
        let candidate = normalized(candidate);
        let Some(root) = self
            .roots
            .iter()
            .filter(|r| candidate.starts_with(r))
            .max_by_key(|r| r.components().count())
            .cloned()
        else {
            return Ok(vec![]);
        };
        let relative = candidate
            .strip_prefix(&root)
            .map_err(|e| Error(e.to_string()))?;
        let mut current = vec![root];
        for component in relative.components() {
            let wanted = component.as_os_str().to_string_lossy().to_lowercase();
            let mut next = vec![];
            for parent in current {
                let Ok(real) = parent.canonicalize() else {
                    continue;
                };
                if !inside(&real, &self.roots) || !real.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(&real).map_err(|e| Error(e.to_string()))? {
                    self.entries_left = self
                        .entries_left
                        .checked_sub(1)
                        .ok_or_else(|| Error("directory entry budget exceeded".into()))?;
                    let entry = entry.map_err(|e| Error(e.to_string()))?;
                    if entry.file_name().to_string_lossy().to_lowercase() == wanted {
                        let real = entry
                            .path()
                            .canonicalize()
                            .map_err(|e| Error(e.to_string()))?;
                        if inside(&real, &self.roots) {
                            next.push(real);
                        }
                    }
                }
            }
            next.sort();
            next.dedup();
            current = next;
        }
        current.retain(|p| p.is_file());
        Ok(current)
    }

    fn locate(&mut self, parent: &Path, saved: &str) -> Result<Vec<PathBuf>> {
        if saved.is_empty() || saved.contains('\0') {
            return Ok(vec![]);
        }
        let saved = saved.replace('\\', "/");
        let p = Path::new(&saved);
        let roots = self.roots.clone();
        let windows_absolute = saved.starts_with("//") || saved.as_bytes().get(1) == Some(&b':');
        let mut exact = vec![];
        if !windows_absolute {
            if p.is_absolute() {
                exact.push(p.to_path_buf());
            } else {
                exact.push(parent.join(p));
                exact.extend(roots.iter().map(|r| r.join(p)));
            }
        }
        let mut fallback = vec![];
        if let Some(name) = p.file_name() {
            fallback.push(parent.join(name));
            fallback.extend(roots.iter().map(|r| r.join(name)));
        }
        for mut tier in [exact, fallback] {
            tier.sort();
            tier.dedup();
            let mut matches = vec![];
            for candidate in tier {
                matches.extend(self.case_candidates(&candidate)?);
            }
            matches.sort();
            matches.dedup();
            if !matches.is_empty() {
                return Ok(matches);
            }
        }
        Ok(vec![])
    }

    fn load(&mut self, path: &Path, depth: usize) -> Result<usize> {
        if let Some(&i) = self.paths.get(path) {
            return Ok(i);
        }
        if depth > self.options.max_depth || self.definitions.len() >= self.options.max_documents {
            return Err(Error("assembly document/depth limit exceeded".into()));
        }
        let mut file = fs::File::open(path).map_err(|e| Error(e.to_string()))?;
        let length = file.metadata().map_err(|e| Error(e.to_string()))?.len();
        let cap = self.limits.max_file_bytes.min(self.bytes_left);
        if length > cap as u64 {
            return Err(Error("assembly file byte limit exceeded".into()));
        }
        let mut bytes = vec![];
        (&mut file)
            .take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error(e.to_string()))?;
        if bytes.len() > cap {
            return Err(Error("assembly file byte limit exceeded".into()));
        }
        self.bytes_left -= bytes.len();
        let doc = inspect(&bytes, &text_path(path), self.limits)?;
        let state = format!(
            "saved-unverified:{:?}:{:?}:{:?}:{:?}",
            doc.ufrx.active_representation,
            doc.ufrx.secondary_representation,
            doc.ufrx.model_states,
            doc.ufrx.active_model_state
        );
        // Relative child resolution depends on the parent directory for IAMs.
        let context = if doc.kind == "assembly" {
            text_path(path.parent().unwrap())
        } else {
            String::new()
        };
        let key = format!(
            "{}:{}:{}:{}",
            doc.source_sha256,
            doc.ufrx.document_id.as_deref().unwrap_or("unknown"),
            digest(state.as_bytes()),
            digest(context.as_bytes())
        );
        if let Some(&i) = self.keys.get(&key) {
            self.paths.insert(path.to_path_buf(), i);
            self.definitions[i].source_paths.push(text_path(path));
            return Ok(i);
        }
        let i = self.definitions.len();
        self.keys.insert(key.clone(), i);
        self.paths.insert(path.to_path_buf(), i);
        self.definitions.push(Definition {
            key,
            path: text_path(path),
            source_paths: vec![text_path(path)],
            document: doc,
            references: vec![],
        });
        Ok(i)
    }

    /// Follow references only after the caller has validated this definition's
    /// identity. Rejected candidates remain inspectable but cannot load children.
    fn follow(&mut self, i: usize, depth: usize) {
        if self.definitions[i].document.kind != "assembly" || !self.expanded.insert(i) {
            return;
        }
        let path = PathBuf::from(&self.definitions[i].path);
        let refs = self.definitions[i].document.ufrx.references.clone();
        for reference in refs {
            let mut resolution = ReferenceResolution {
                reference_id: reference.reference_id,
                saved_path: reference.path.clone(),
                status: "missing".into(),
                candidates: vec![],
                definition: None,
                detail: None,
            };
            let result: Result<()> = (|| {
                let candidates = self.locate(path.parent().unwrap(), &reference.path)?;
                resolution.candidates = candidates.iter().map(|p| text_path(p)).collect();
                if candidates.len() > 1 {
                    resolution.status = "ambiguous".into();
                    return Ok(());
                }
                let Some(candidate) = candidates.first() else {
                    return Ok(());
                };
                let target = self.load(candidate, depth + 1)?;
                let doc = &self.definitions[target].document;
                let expected = &reference.document_id;
                if expected == "00000000-0000-0000-0000-000000000000"
                    || doc.ufrx.document_id.is_none()
                {
                    resolution.status = "identity_unverified".into();
                    return Ok(());
                }
                if doc.ufrx.document_id.as_ref() != Some(expected) {
                    resolution.status = "identity_mismatch".into();
                    resolution.detail = Some(format!(
                        "expected {expected}, found {:?}",
                        doc.ufrx.document_id
                    ));
                    return Ok(());
                }
                if !matches!(doc.kind.as_str(), "assembly" | "part")
                    || !matches!(doc.ufrx.status.as_str(), "decoded_subset" | "identity_only")
                {
                    resolution.status = "unsupported_document".into();
                    return Ok(());
                }
                resolution.definition = Some(target);
                resolution.status = "resolved".into();
                self.follow(target, depth + 1);
                Ok(())
            })();
            if let Err(e) = result {
                resolution.status = "unavailable".into();
                resolution.detail = Some(e.to_string());
            }
            self.definitions[i].references.push(resolution);
        }
    }
}

/// Resolve the root's parent directory and explicit search roots only. No
/// recursive basename search, .ipj evaluation, network access or Inventor runtime.
pub fn resolve_file(
    path: &Path,
    limits: &Limits,
    options: &ResolveOptions,
) -> Result<AssemblyGraph> {
    limits.validate()?;
    if options.max_depth > 128
        || options.max_instances > 1_000_000
        || options.max_documents > 4096
        || options.search_roots.len() > 256
    {
        return Err(Error("assembly resolver hard limit exceeded".into()));
    }
    let path = path.canonicalize().map_err(|e| Error(e.to_string()))?;
    let mut roots = vec![path
        .parent()
        .ok_or_else(|| Error("root has no parent".into()))?
        .to_path_buf()];
    for r in &options.search_roots {
        let r = r.canonicalize().map_err(|e| Error(e.to_string()))?;
        if !r.is_dir() {
            return Err(Error("search root is not a directory".into()));
        }
        roots.push(r);
    }
    roots.sort();
    roots.dedup();
    let mut loader = Loader {
        limits,
        options,
        roots,
        definitions: vec![],
        keys: HashMap::new(),
        paths: HashMap::new(),
        expanded: HashSet::new(),
        bytes_left: options.max_total_file_bytes,
        entries_left: options.max_directory_entries,
    };
    let root = loader.load(&path, 0)?;
    if loader.definitions[root].document.kind != "assembly" {
        return Err(Error("root document is not an IAM assembly".into()));
    }
    loader.follow(root, 0);
    let mut graph = AssemblyGraph {
        api_version: 1,
        root,
        definitions: loader.definitions,
        instances: vec![],
        search_roots: loader.roots.iter().map(|r| text_path(r)).collect(),
        structure_status: "partial".into(),
        current_state: "unverified".into(),
        complete: false,
        diagnostics: vec![],
    };
    expand(
        &mut graph,
        root,
        None,
        Some(IDENTITY),
        &[],
        &mut HashSet::new(),
        options,
    );
    let resolved = graph.diagnostics.is_empty()
        && graph.definitions.iter().all(|d| {
            d.document.status == "decoded_subset"
                && d.references.iter().all(|r| r.status == "resolved")
        })
        && graph.instances.iter().all(|i| i.resolution == "resolved");
    graph.structure_status = if resolved { "resolved" } else { "partial" }.into();
    Ok(graph)
}

fn expand(
    graph: &mut AssemblyGraph,
    owner: usize,
    parent: Option<usize>,
    world: Option<Matrix>,
    path: &[u32],
    ancestors: &mut HashSet<usize>,
    options: &ResolveOptions,
) {
    if path.len() >= options.max_depth {
        graph
            .diagnostics
            .push("assembly expansion depth limit exceeded".into());
        return;
    }
    ancestors.insert(owner);
    let occurrences = graph.definitions[owner].document.occurrences.clone();
    for (ordinal, o) in occurrences.into_iter().enumerate() {
        if graph.instances.len() >= options.max_instances {
            graph
                .diagnostics
                .push("assembly instance limit exceeded".into());
            break;
        }
        let refs: Vec<_> = graph.definitions[owner]
            .references
            .iter()
            .filter(|r| r.reference_id == o.reference_id)
            .collect();
        let reference = if refs.len() == 1 { Some(refs[0]) } else { None };
        let target = reference.and_then(|r| r.definition);
        let mut status = if o.status != "resolved" {
            o.status.clone()
        } else {
            reference
                .map(|r| r.status.clone())
                .unwrap_or_else(|| "ambiguous_reference".into())
        };
        let mut world_transform = None;
        if let (Some(w), Some(local)) = (world, o.local_transform_mm) {
            match compose(&w, &local) {
                Ok(m) => world_transform = Some(m),
                Err(e) => {
                    status = "invalid_transform".into();
                    graph.diagnostics.push(e.to_string());
                }
            }
        }
        if world_transform.is_none() && status == "resolved" {
            status = "unresolved_parent_transform".into();
        }
        let cycle = target.is_some_and(|i| ancestors.contains(&i));
        if cycle {
            status = "cycle".into();
        }
        let mut child_path = path.to_vec();
        child_path.push(o.occurrence_id);
        let index = graph.instances.len();
        graph.instances.push(Instance {
            parent,
            owner_definition: owner,
            occurrence_index: ordinal,
            occurrence_id: o.occurrence_id,
            path: child_path.clone(),
            name: o.name,
            definition: target,
            resolution: status,
            suppressed: o.suppressed,
            visible: o.visible,
            substitute: o.substitute,
            local_transform_mm: o.local_transform_mm,
            world_transform_mm: world_transform,
        });
        if let Some(target) = target.filter(|_| !cycle) {
            if graph.definitions[target].document.kind == "assembly" {
                expand(
                    graph,
                    target,
                    Some(index),
                    world_transform,
                    &child_path,
                    ancestors,
                    options,
                );
            }
        }
    }
    ancestors.remove(&owner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::tests::container;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "inventor-m5-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn root(dir: &Path, saved: &str) -> PathBuf {
        let p = dir.join("root.iam");
        fs::write(
            &p,
            container(
                "assembly",
                [1; 16],
                &[(saved, [2; 16], 7)],
                &[(42, 7), (99, 7)],
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn filesystem_missing_case_alias_identity_and_content_cache() {
        let dir = Temp::new();
        let p = root(&dir.0, "C:\\old-machine\\part.ipt");
        let run = || resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        let missing = run();
        assert_eq!(missing.definitions[0].references[0].status, "missing");
        assert_eq!(missing.instances.len(), 2);
        let part = dir.0.join("PART.ipt");
        let bytes = container("part", [2; 16], &[], &[]);
        fs::write(&part, &bytes).unwrap();
        let a = run();
        assert_eq!(a.definitions.len(), 2);
        assert_eq!(a.definitions[0].references[0].status, "resolved");
        assert_eq!(a.instances[0].definition, a.instances[1].definition);
        let key = a.definitions[1].key.clone();
        // A legal CFB file with the same internal ID but different bytes is a new snapshot.
        let mut changed = bytes.clone();
        changed.extend([0; 512]);
        fs::write(&part, changed).unwrap();
        assert_ne!(run().definitions[1].key, key);
        fs::write(&part, container("part", [3; 16], &[], &[])).unwrap();
        assert_eq!(
            run().definitions[0].references[0].status,
            "identity_mismatch"
        );
        #[cfg(unix)]
        {
            fs::write(dir.0.join("part.ipt"), bytes).unwrap();
            assert_eq!(run().definitions[0].references[0].status, "ambiguous");
        }
    }

    #[test]
    fn rejected_iam_does_not_load_children_but_later_valid_reference_can() {
        let dir = Temp::new();
        let p = root(&dir.0, "child.iam"); // expects ID 2
        fs::write(
            dir.0.join("child.iam"),
            container(
                "assembly",
                [3; 16],
                &[("grandchild.ipt", [4; 16], 8)],
                &[(8, 8)],
            ),
        )
        .unwrap();
        fs::write(
            dir.0.join("grandchild.ipt"),
            container("part", [4; 16], &[], &[]),
        )
        .unwrap();
        let graph = resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        assert_eq!(graph.definitions.len(), 2);
        assert_eq!(
            graph.definitions[0].references[0].status,
            "identity_mismatch"
        );
        assert!(graph.definitions[1].references.is_empty());
        assert_eq!(graph.definitions[1].document.ufrx.references.len(), 1);
        // A rejected path cache entry must not poison a later valid identity.
        fs::write(
            &p,
            container(
                "assembly",
                [1; 16],
                &[("child.iam", [2; 16], 7), ("child.iam", [3; 16], 9)],
                &[(42, 7), (43, 9)],
            ),
        )
        .unwrap();
        let graph = resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        assert_eq!(graph.definitions.len(), 3);
        assert_eq!(graph.definitions[0].references[1].status, "resolved");
        assert_eq!(graph.definitions[1].references[0].status, "resolved");
    }

    #[test]
    fn explicit_roots_can_conflict_and_parent_traversal_does_not_escape() {
        let dir = Temp::new();
        let r1 = dir.0.join("one");
        let r2 = dir.0.join("two");
        fs::create_dir(&r1).unwrap();
        fs::create_dir(&r2).unwrap();
        let p = root(&r1, "../two/part.ipt");
        fs::write(r2.join("part.ipt"), container("part", [2; 16], &[], &[])).unwrap();
        let graph = resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        assert_eq!(graph.definitions[0].references[0].status, "missing");
        let options = ResolveOptions {
            search_roots: vec![r2.clone()],
            ..Default::default()
        };
        assert_eq!(
            resolve_file(&p, &Default::default(), &options)
                .unwrap()
                .definitions[0]
                .references[0]
                .status,
            "resolved"
        );
        root(&r1, "C:\\saved\\part.ipt");
        fs::write(r1.join("part.ipt"), container("part", [2; 16], &[], &[])).unwrap();
        assert_eq!(
            resolve_file(&p, &Default::default(), &options)
                .unwrap()
                .definitions[0]
                .references[0]
                .status,
            "ambiguous"
        );
    }

    #[test]
    #[cfg(unix)]
    fn symlink_outside_roots_is_not_opened() {
        let dir = Temp::new();
        let outside = Temp::new();
        let p = root(&dir.0, "part.ipt");
        let secret = outside.0.join("secret.ipt");
        fs::write(&secret, b"not a CFB").unwrap();
        std::os::unix::fs::symlink(&secret, dir.0.join("part.ipt")).unwrap();
        let graph = resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        assert_eq!(graph.definitions[0].references[0].status, "missing");
        assert_eq!(graph.definitions.len(), 1);
    }

    #[test]
    fn filesystem_cycles_and_all_resource_limits_remain_visible() {
        let dir = Temp::new();
        let p = root(&dir.0, "child.iam");
        fs::write(
            dir.0.join("child.iam"),
            container("assembly", [2; 16], &[("root.iam", [1; 16], 8)], &[(8, 8)]),
        )
        .unwrap();
        let graph = resolve_file(&p, &Default::default(), &Default::default()).unwrap();
        assert!(graph.instances.iter().any(|i| i.resolution == "cycle"));
        assert!(!graph.complete);
        for options in [
            ResolveOptions {
                max_documents: 1,
                ..Default::default()
            },
            ResolveOptions {
                max_directory_entries: 0,
                ..Default::default()
            },
            ResolveOptions {
                max_depth: 0,
                ..Default::default()
            },
            ResolveOptions {
                max_instances: 0,
                ..Default::default()
            },
        ] {
            let g = resolve_file(&p, &Default::default(), &options).unwrap();
            assert_eq!(g.structure_status, "partial");
            assert!(
                !g.diagnostics.is_empty() || g.definitions[0].references[0].status == "unavailable"
            );
        }
        assert!(resolve_file(
            &p,
            &Default::default(),
            &ResolveOptions {
                max_total_file_bytes: 1,
                ..Default::default()
            }
        )
        .is_err());
    }

    fn definition(id: u8, targets: &[(u32, usize)], matrices: &[Matrix]) -> Definition {
        let occ: Vec<_> = targets.iter().map(|(o, _)| (*o, *o)).collect();
        let refs: Vec<_> = targets
            .iter()
            .map(|(o, t)| ("part.ipt", [*t as u8; 16], *o))
            .collect();
        let mut doc = inspect(
            &container("assembly", [id; 16], &refs, &occ),
            "synthetic",
            &Default::default(),
        )
        .unwrap();
        for (o, m) in doc.occurrences.iter_mut().zip(matrices) {
            o.local_transform_mm = Some(*m);
            o.status = "resolved".into();
        }
        Definition {
            key: id.to_string(),
            path: format!("{id}.iam"),
            source_paths: vec![format!("{id}.iam")],
            document: doc,
            references: targets
                .iter()
                .map(|(o, t)| ReferenceResolution {
                    reference_id: *o,
                    saved_path: "part.ipt".into(),
                    status: "resolved".into(),
                    candidates: vec![],
                    definition: Some(*t),
                    detail: None,
                })
                .collect(),
        }
    }

    #[test]
    fn repeated_nested_prototypes_have_distinct_world_transforms() {
        let p = [
            [0., -1., 0., 10.],
            [1., 0., 0., 20.],
            [0., 0., 1., 30.],
            [0., 0., 0., 1.],
        ];
        let mut q = IDENTITY;
        q[0][3] = -5.;
        let mut local = IDENTITY;
        local[0][3] = 2.;
        local[1][3] = 3.;
        let mut part = definition(2, &[], &[]);
        part.document.kind = "part".into();
        let mut graph = AssemblyGraph {
            api_version: 1,
            root: 0,
            definitions: vec![
                definition(0, &[(10, 1), (20, 1)], &[p, q]),
                definition(1, &[(3, 2)], &[local]),
                part,
            ],
            instances: vec![],
            search_roots: vec![],
            structure_status: "partial".into(),
            current_state: "unverified".into(),
            complete: false,
            diagnostics: vec![],
        };
        expand(
            &mut graph,
            0,
            None,
            Some(IDENTITY),
            &[],
            &mut HashSet::new(),
            &Default::default(),
        );
        assert_eq!(graph.instances.len(), 4);
        let a = &graph.instances[1];
        let b = &graph.instances[3];
        assert_eq!(a.definition, b.definition);
        assert_eq!(a.path, vec![10, 3]);
        assert_eq!(b.path, vec![20, 3]);
        let xyz = |i: &Instance| {
            let m = i.world_transform_mm.unwrap();
            [m[0][3], m[1][3], m[2][3]]
        };
        assert_eq!(xyz(a), [7., 22., 30.]);
        assert_eq!(xyz(b), [-3., 3., 0.]);
        // A missing parent placement must propagate unknown, not identity.
        graph.definitions[0].document.occurrences[0].local_transform_mm = None;
        graph.instances.clear();
        expand(
            &mut graph,
            0,
            None,
            Some(IDENTITY),
            &[],
            &mut HashSet::new(),
            &Default::default(),
        );
        assert!(graph.instances[1].world_transform_mm.is_none());
    }
}
