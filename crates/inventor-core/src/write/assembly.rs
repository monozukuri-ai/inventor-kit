//! Bounded flat IAM authoring with 1..32 visible grounded parts at identity.
//! Encoding does not call Inventor or read templates. This bounded profile was
//! qualified with Inventor build 311270010; individual outputs are not opened.

use crate::{Error, Limits, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
mod allocation;
mod input;
mod wire;

pub const NATIVE_QUALIFIED: bool = true;
pub const PROFILE_ID: &str = "inventor-iam-rse31-fixed-parts-v1";

/// Identity of an existing part, bound to its bytes and a portable package path.
/// No existing IAM, template or record payload is an input to this model.
#[derive(Debug, Clone, Serialize)]
pub struct PartDefinition {
    relative_path: String,
    document_id: String,
    database_revision_id: String,
    bytes: usize,
    sha256: String,
    display_name: String,
    bounds_cm: Option<[f64; 6]>,
}

impl PartDefinition {
    /// Inspect supplied IPT bytes. Does not open files or load Inventor.
    /// This checks the bounded identity profile, not the part's native usability.
    pub fn from_ipt(relative_path: &str, bytes: &[u8]) -> Result<Self> {
        validate_part_path(relative_path)?;
        let limits = Limits::default();
        if bytes.is_empty() || bytes.len() > limits.max_file_bytes {
            return Err(Error("IAM part bytes outside the supported limit".into()));
        }
        let doc = crate::assembly::inspect(bytes, "assembly-part-input", &limits)?;
        if doc.kind != "part"
            || doc.ufrx.status != "identity_only"
            || doc.ufrx.schema != Some(15)
            || doc.ufrx.save_version.is_none_or(|v| v[2] != 31)
            || !doc.diagnostics.is_empty()
        {
            return Err(Error(
                "IAM requires an IPT with the observed schema15/major31 identity profile".into(),
            ));
        }
        let identifier = |value: Option<String>| -> Result<String> {
            let text = value.ok_or_else(|| Error("IPT identity is missing".into()))?;
            let id = uuid::Uuid::parse_str(&text).map_err(super::err)?;
            if id.is_nil() {
                return Err(Error("IPT identity is nil".into()));
            }
            Ok(id.to_string())
        };
        Ok(Self {
            relative_path: relative_path.into(),
            document_id: identifier(doc.ufrx.document_id)?,
            database_revision_id: identifier(doc.ufrx.database_revision_id)?,
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            display_name: String::new(),
            bounds_cm: None,
        })
    }

    /// Refuse a substituted or modified part at the output boundary.
    pub fn verify_bytes(&self, bytes: &[u8]) -> Result<()> {
        if bytes.len() != self.bytes || format!("{:x}", Sha256::digest(bytes)) != self.sha256 {
            return Err(Error(
                "IPT bytes differ from the assembly definition".into(),
            ));
        }
        Ok(())
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// Bind a newly generated IPT in the bounded writer profile. The identity,
    /// generation, result framing, display name and bounds come from its bytes.
    /// Native-resaved or arbitrary vendor IPTs are intentionally unsupported.
    pub fn from_authored_ipt(filename: &str, bytes: &[u8]) -> Result<Self> {
        validate_part_path(filename)?;
        if filename.contains('/') {
            return Err(Error(
                "IAM encoding requires same-folder IPT filenames".into(),
            ));
        }
        let mut part = Self::from_ipt(filename, bytes)?;
        let (name, bounds) = input::inspect(bytes, &part)?;
        part.display_name = name;
        part.bounds_cm = Some(bounds);
        Ok(part)
    }
}

/// Flat assembly of visible, grounded parts, each at the identity transform.
/// Empty documents can be planned, but are outside this encoding profile.
#[derive(Debug, Clone)]
pub struct AssemblyDocument {
    name: String,
    part_number: String,
    parts: Vec<(PartDefinition, String)>,
}

impl AssemblyDocument {
    pub fn empty(name: &str) -> Result<Self> {
        validate_name(name)?;
        Ok(Self {
            name: name.into(),
            part_number: name.into(),
            parts: Vec::new(),
        })
    }

    pub fn single_part(name: &str, part: PartDefinition, occurrence_name: &str) -> Result<Self> {
        validate_name(occurrence_name)?;
        let mut doc = Self::empty(name)?;
        doc.add_part(part, occurrence_name)?;
        Ok(doc)
    }

    pub fn with_part_number(mut self, part_number: &str) -> Result<Self> {
        validate_name(part_number)?;
        self.part_number = part_number.into();
        Ok(self)
    }

    /// Add a placement. Repeating one filename reuses its external definition.
    pub fn add_part(&mut self, part: PartDefinition, name: &str) -> Result<&mut Self> {
        validate_name(name)?;
        if self.parts.len() >= 32
            || self
                .parts
                .iter()
                .any(|(_, old)| old.to_lowercase() == name.to_lowercase())
        {
            return Err(Error(
                "IAM requires at most 32 uniquely named occurrences".into(),
            ));
        }
        for (old, _) in &self.parts {
            if old.relative_path.to_lowercase() == part.relative_path.to_lowercase()
                && (old.relative_path != part.relative_path || old.sha256 != part.sha256)
            {
                return Err(Error("conflicting Windows IPT filenames".into()));
            }
            if old.document_id == part.document_id && old.sha256 != part.sha256 {
                return Err(Error(
                    "conflicting IPT bytes with the same document identity".into(),
                ));
            }
        }
        self.parts.push((part, name.into()));
        Ok(self)
    }

    /// Encode a complete IAM in memory. All definitions must be constructed by
    /// `PartDefinition::from_authored_ipt`; the caller packages those exact bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.prepare().encode()
    }

    /// Allocate independent identities and a manifest. A fresh preparation is
    /// a new document, even when every part and occurrence is unchanged.
    pub fn prepare(&self) -> PreparedAssembly {
        let fresh = || uuid::Uuid::new_v4().to_string();
        let mut used: BTreeSet<String> = self
            .parts
            .iter()
            .flat_map(|(p, _)| [p.document_id.clone(), p.database_revision_id.clone()])
            .chain(wire::ANCHORS.iter().map(|s| s.to_string()))
            .collect();
        let mut allocate = || loop {
            let id = fresh();
            if used.insert(id.clone()) {
                break id;
            }
        };
        let document_id = allocate();
        let creation_revision_id = allocate();
        let segments = [
            ("AmDcSegment", "AmDcSegmentType"),
            ("AmGraphicsSegment", "AmGRxSegmentType"),
            ("AmBrowserSegment", "AmBRxSegmentType"),
            ("AmAppSegment", "AmAppSegmentType"),
            ("NBNotebookSegment", "NotebookSegmentType"),
            ("AmRxSegment", "AmRxSegmentType"),
            ("AmBREPSegment", "AmBREPSegmentType"),
            ("DesignViewSegment", "FWxDesignViewManagerType"),
        ]
        .into_iter()
        .map(|(name, kind)| SegmentIdentity {
            name,
            kind,
            id: allocate(),
        })
        .collect();
        let mut definitions = Vec::new();
        let mut occurrences = Vec::new();
        for (part, name) in &self.parts {
            let reference_id = if let Some(old) = definitions
                .iter()
                .find(|d: &&ExternalDefinition| d.part.relative_path == part.relative_path)
            {
                old.reference_id
            } else {
                let id = definitions.len() as u32 + 1;
                definitions.push(ExternalDefinition {
                    reference_id: id,
                    part: part.clone(),
                });
                id
            };
            occurrences.push(Placement {
                occurrence_id: occurrences.len() as u32 + 1,
                reference_id,
                name: name.clone(),
                grounded: true,
                visible: true,
                matrix_mm: [
                    [1., 0., 0., 0.],
                    [0., 1., 0., 0.],
                    [0., 0., 1., 0.],
                    [0., 0., 0., 1.],
                ],
            });
        }
        PreparedAssembly {
            profile: PROFILE_ID,
            native_qualified: NATIVE_QUALIFIED
                && !self.parts.is_empty()
                && self.parts.iter().all(|(p, _)| p.bounds_cm.is_some()),
            document_name: self.name.clone(),
            part_number: self.part_number.clone(),
            document_id,
            creation_revision_id,
            segments,
            definitions,
            occurrences,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedAssembly {
    pub profile: &'static str,
    pub native_qualified: bool,
    pub document_name: String,
    pub part_number: String,
    pub document_id: String,
    pub creation_revision_id: String,
    pub segments: Vec<SegmentIdentity>,
    pub definitions: Vec<ExternalDefinition>,
    pub occurrences: Vec<Placement>,
}
impl PreparedAssembly {
    /// Encode a prepared document after validating all publicly mutable fields.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let plan = allocation::expand(self)?;
        let bytes = super::cfb::encode_document(
            wire::streams(&plan, self)?,
            "e60f81e1-49b3-11d0-93c3-7e0706000000",
        )?;
        let result = crate::assembly::inspect(&bytes, "authored-assembly", &Limits::default())?;
        if result.kind != "assembly"
            || !result.diagnostics.is_empty()
            || result.ufrx.document_id.as_deref() != Some(self.document_id.as_str())
            || result.ufrx.references.len() != self.definitions.len()
            || result.ufrx.occurrences.len() != self.occurrences.len()
        {
            return Err(Error("authored IAM independent inspection failed".into()));
        }
        Ok(bytes)
    }
    fn validate(&self) -> Result<()> {
        validate_name(&self.document_name)?;
        validate_name(&self.part_number)?;
        if self.profile != PROFILE_ID
            || !(1..=32).contains(&self.occurrences.len())
            || self.definitions.is_empty()
            || self.definitions.len() > self.occurrences.len()
            || self.segments.len() != 8
        {
            return Err(Error("unsupported IAM profile or placement count".into()));
        }
        let expected = AssemblyDocument::empty("check")?.prepare().segments;
        let mut ids: BTreeSet<String> = wire::ANCHORS.iter().map(|s| s.to_string()).collect();
        for def in &self.definitions {
            ids.insert(def.part.document_id.clone());
            ids.insert(def.part.database_revision_id.clone());
        }
        for id in [&self.document_id, &self.creation_revision_id]
            .into_iter()
            .chain(self.segments.iter().map(|s| &s.id))
        {
            let parsed = uuid::Uuid::parse_str(id).map_err(super::err)?;
            if parsed.is_nil() || parsed.to_string() != *id || !ids.insert(id.clone()) {
                return Err(Error("invalid or colliding IAM identity".into()));
            }
        }
        for (s, e) in self.segments.iter().zip(expected) {
            if s.name != e.name || s.kind != e.kind {
                return Err(Error("invalid IAM segment roles".into()));
            }
        }
        let mut paths = BTreeSet::new();
        let mut part_identities = BTreeMap::new();
        let mut uses = vec![0; self.definitions.len()];
        let mut names = BTreeSet::new();
        for (i, d) in self.definitions.iter().enumerate() {
            if part_identities
                .insert(&d.part.document_id, &d.part.sha256)
                .is_some_and(|prior| prior != &d.part.sha256)
            {
                return Err(Error(
                    "conflicting IPT bytes with the same document identity".into(),
                ));
            }
            if d.reference_id != i as u32 + 1
                || d.part.bounds_cm.is_none()
                || d.part.relative_path.contains('/')
                || !paths.insert(d.part.relative_path.to_lowercase())
            {
                return Err(Error(
                    "IAM encoding requires distinct authored IPT definitions".into(),
                ));
            }
        }
        let identity = [
            [1., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        for (i, o) in self.occurrences.iter().enumerate() {
            validate_name(&o.name)?;
            if o.occurrence_id != i as u32 + 1
                || o.reference_id == 0
                || o.reference_id as usize > uses.len()
                || !o.grounded
                || !o.visible
                || o.matrix_mm != identity
                || !names.insert(o.name.to_lowercase())
            {
                return Err(Error(
                    "IAM supports unique visible grounded occurrences at identity only".into(),
                ));
            }
            uses[o.reference_id as usize - 1] += 1;
        }
        if uses.contains(&0) {
            return Err(Error("unused IAM external definition".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct SegmentIdentity {
    pub name: &'static str,
    pub kind: &'static str,
    pub id: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ExternalDefinition {
    pub reference_id: u32,
    pub part: PartDefinition,
}
#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    pub occurrence_id: u32,
    pub reference_id: u32,
    pub name: String,
    pub grounded: bool,
    pub visible: bool,
    pub matrix_mm: [[f64; 4]; 4],
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.chars().any(char::is_control) || name.encode_utf16().count() > 255 {
        return Err(Error(
            "IAM name must contain 1..255 UTF-16 units without NUL".into(),
        ));
    }
    Ok(())
}

fn validate_part_path(path: &str) -> Result<()> {
    // One spelling of a portable relative path prevents Windows path aliases
    // from turning a repeated definition into multiple native reference rows.
    if path.is_empty()
        || path.encode_utf16().count() > 240
        || path
            .chars()
            .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
        || !path.to_ascii_lowercase().ends_with(".ipt")
    {
        return Err(Error(
            "IAM part path must be a portable relative .ipt path".into(),
        ));
    }
    for component in path.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|p| {
                stem.strip_prefix(p).is_some_and(|n| {
                    matches!(
                        n,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.ends_with([' ', '.'])
            || device
        {
            return Err(Error(
                "IAM part path contains an ambiguous or escaping component".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_parts_encode_dynamic_flat_assemblies() {
        let mut box_doc = super::super::BoxDocument::new(acis_core::authoring::BoxSpec::new(
            (20., 30., 40.).into(),
            (0., 0., 0.).into(),
        ));
        box_doc.document_name = "Test part".into();
        let bytes = box_doc.encode().unwrap();
        let part = PartDefinition::from_authored_ipt("box.ipt", &bytes).unwrap();
        assert_eq!(part.bounds_cm, Some([0., 0., 0., 2., 3., 4.]));
        for n in [1, 3, 32] {
            let mut doc = AssemblyDocument::empty("Test assembly").unwrap();
            for i in 0..n {
                doc.add_part(part.clone(), &format!("Box:{}", i + 1))
                    .unwrap();
            }
            let prepared = doc.prepare();
            assert!(prepared.native_qualified);
            let output = prepared.encode().unwrap();
            let read = crate::assembly::inspect(&output, "test", &Limits::default()).unwrap();
            assert_eq!(read.ufrx.occurrences.len(), n);
            assert_eq!(read.ufrx.references.len(), 1);
            assert_eq!(read.ufrx.references[0].path, "box.ipt");
            assert_eq!(prepared.encode().unwrap(), output);
            let mut bad = prepared.clone();
            bad.occurrences[0].matrix_mm[0][3] = 10.;
            assert!(bad.encode().is_err());
            let mut bad = prepared.clone();
            bad.creation_revision_id = bad.document_id.clone();
            assert!(bad.encode().is_err());
            if n > 1 {
                let mut bad = prepared.clone();
                let mut alias = bad.definitions[0].clone();
                alias.reference_id = 2;
                alias.part.relative_path = "changed.ipt".into();
                alias.part.sha256 = "different bytes".into();
                bad.definitions.push(alias);
                bad.occurrences[1].reference_id = 2;
                assert!(bad.encode().is_err());
            }
        }
    }

    #[test]
    fn paths_cannot_escape_or_alias_windows_devices() {
        for bad in [
            "../a.ipt",
            "/a.ipt",
            "C:/a.ipt",
            "a\\b.ipt",
            "a//b.ipt",
            "a/./b.ipt",
            "a /b.ipt",
            "a./b.ipt",
            "con.ipt",
            "a/COM1.x.ipt",
            "LPT².ipt",
            "a:stream.ipt",
            "a.iam",
        ] {
            assert!(validate_part_path(bad).is_err(), "{bad}");
        }
        validate_part_path("parts/部品 A.ipt").unwrap();
    }

    #[test]
    fn planned_documents_have_independent_role_ids() {
        let doc = AssemblyDocument::empty("新規アセンブリ").unwrap();
        let a = doc.prepare();
        let b = doc.prepare();
        assert!(a.occurrences.is_empty() && a.definitions.is_empty());
        assert!(!a.native_qualified);
        let ids: std::collections::BTreeSet<_> = [&a, &b]
            .into_iter()
            .flat_map(|p| {
                std::iter::once(&p.document_id)
                    .chain(std::iter::once(&p.creation_revision_id))
                    .chain(p.segments.iter().map(|s| &s.id))
            })
            .collect();
        assert_eq!(ids.len(), 20);
        assert_eq!(a.segments.len(), 8);
        assert!(AssemblyDocument::empty("").is_err());
        assert!(AssemblyDocument::empty(&"𠮷".repeat(128)).is_err());
    }

    #[test]
    fn definition_is_bound_to_ipt_bytes_and_referenced_by_id() {
        let bytes = super::super::BoxDocument::new(acis_core::authoring::BoxSpec::new(
            (20., 30., 40.).into(),
            (0., 0., 0.).into(),
        ))
        .encode()
        .unwrap();
        let part = PartDefinition::from_ipt("parts/box.ipt", &bytes).unwrap();
        part.verify_bytes(&bytes).unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(part.verify_bytes(&changed).is_err());
        assert!(PartDefinition::from_ipt("parts/box.ipt", b"not an IPT").is_err());
        let p = AssemblyDocument::single_part("Assembly", part, "Box:1")
            .unwrap()
            .prepare();
        assert_eq!(p.definitions.len(), 1);
        assert_eq!(p.occurrences.len(), 1);
        assert_eq!(p.definitions[0].reference_id, p.occurrences[0].reference_id);
        assert!(p.occurrences[0].grounded && p.occurrences[0].visible);
        assert_eq!(p.occurrences[0].matrix_mm[0], [1., 0., 0., 0.]);
    }

    #[test]
    fn shared_cfb_container_has_assembly_class_and_independent_streams() {
        use std::io::{Cursor, Read};
        let streams =
            std::collections::BTreeMap::from([("/RSeStorage/V1/RSeDb".into(), vec![1, 2, 3])]);
        let bytes =
            super::super::cfb::encode_document(streams, "e60f81e1-49b3-11d0-93c3-7e0706000000")
                .unwrap();
        let mut cfb = ::cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
        let clsid = uuid::Uuid::parse_str("e60f81e1-49b3-11d0-93c3-7e0706000000").unwrap();
        assert_eq!(cfb.root_entry().clsid(), &clsid);
        assert_eq!(cfb.entry("/RSeStorage/V1").unwrap().clsid(), &clsid);
        let mut data = Vec::new();
        cfb.open_stream("/RSeStorage/V1/RSeDb")
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        assert_eq!(data, [1, 2, 3]);
    }
}
