use super::*;
use acis_core::authoring::AuthoredBox;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Allocation {
    pub ordinal: usize,
    pub key: Option<u16>,
    pub class: String,
}
pub(super) struct Graph<'a> {
    pub profile: profile::Profile,
    pub input: &'a BoxDocument,
    pub shape: AuthoredBox,
    pub kernels: Option<[Vec<u8>; 2]>,
    pub label_count: Option<u32>,
    pub metadata: Option<(String, String)>,
    pub birth: [u8; 16],
    pub revision: [u8; 16],
    pub ids: BTreeMap<String, [u8; 16]>,
    pub allocations: BTreeMap<(String, String), Allocation>,
}
impl<'a> Graph<'a> {
    pub fn new(
        profile: profile::Profile,
        input: &'a BoxDocument,
        shape: AuthoredBox,
    ) -> Result<Self> {
        let birth = uuid::Uuid::new_v4().to_bytes_le();
        // Independent roles, coherent fresh creation revision (M1 V36 contract).
        let revision = birth;
        let mut ids = BTreeMap::new();
        let mut used = BTreeSet::from([birth]);
        for owner in array(&profile.0, "owners")? {
            let name = string(owner, "name")?;
            let id = uuid::Uuid::new_v4().to_bytes_le();
            if !used.insert(id) {
                return Err(Error("GUID collision".into()));
            }
            ids.insert(name.to_owned(), id);
        }
        let mut counts = BTreeMap::<String, usize>::new();
        let mut keys = BTreeSet::new();
        let mut allocations = BTreeMap::new();
        for rec in array(&profile.0, "records")? {
            let owner = string(rec, "owner")?.to_owned();
            let role = string(rec, "role")?.to_owned();
            let ordinal = counts.entry(owner.clone()).or_default();
            let assigned = if rec["keyed"] == true {
                let key = profile.initial_key(rec)?;
                if !keys.insert((owner.clone(), key)) {
                    return Err(Error("duplicate profile key".into()));
                }
                Some(key)
            } else {
                None
            };
            let value = Allocation {
                ordinal: *ordinal,
                key: assigned,
                class: string(rec, "class")?.into(),
            };
            *ordinal += 1;
            if allocations.insert((owner, role), value).is_some() {
                return Err(Error("duplicate record role".into()));
            }
        }
        let graph = Self {
            profile,
            input,
            shape,
            kernels: None,
            label_count: None,
            metadata: None,
            birth,
            revision,
            ids,
            allocations,
        };
        for e in array(&graph.profile.0, "endpoints")? {
            let owner = string(e, "target_owner")?;
            let a = graph.get(owner, string(e, "target_role")?)?;
            if a.class != string(e, "target_class")?
                || a.key != Some(number(&e["target_key_role"])? as u16)
            {
                return Err(Error("registry target class/key differs".into()));
            }
        }
        Ok(graph)
    }
    pub fn get(&self, owner: &str, role: &str) -> Result<&Allocation> {
        self.allocations
            .get(&(owner.into(), role.into()))
            .ok_or_else(|| Error(format!("unresolved {owner}/{role}")))
    }
    pub fn reference(&self, owner: &str, role: &str, namespace: &str) -> Result<u32> {
        let ordinal = self.get(owner, role)?.ordinal as u32 + 1;
        match namespace {
            "local_qualified" => Ok(0x80000000 | ordinal),
            "local_bare" => Ok(ordinal),
            _ => Err(Error("unknown local reference namespace".into())),
        }
    }
    pub fn registry_ref(&self, owner: &str, f: &Value) -> Result<u32> {
        let target = string(f, "target_role")?.replace("/profile-key:", "/key:");
        let eps: Vec<_> = array(&self.profile.0, "endpoints")?
            .iter()
            .filter(|e| e["owner"] == owner)
            .collect();
        for (i, e) in eps.iter().enumerate() {
            let role = format!(
                "{}/{}",
                string(e, "target_owner")?,
                e["target_role"]
                    .as_str()
                    .unwrap_or("omitted-cache-endpoint")
            );
            if (role == target
                || format!(
                    "{}/{}/key:{}",
                    string(e, "target_owner")?,
                    string(e, "target_class")?,
                    number(&e["target_key_role"])?
                ) == target)
                && e["target_class"] == f["target_class"]
                && e["target_owner"] == f["target_owner"]
            {
                return Ok(i as u32 + 1);
            }
        }
        Err(Error(format!("unresolved registry field {target}")))
    }
    pub fn recipe(&self, rec: &Value) -> Result<Vec<u8>> {
        let owner = string(rec, "owner")?;
        let role = string(rec, "role")?;
        let recipe = &rec["recipe"];
        let mut w = Wire::default();
        for f in array(recipe, "fields")? {
            let value = match string(f, "policy")? {
                "fixed" => f["value"].clone(),
                "assigned_key" => self
                    .get(owner, role)?
                    .key
                    .ok_or_else(|| Error("absent assigned key".into()))?
                    .into(),
                "local_reference" => {
                    let target = self.get(owner, string(f, "target_role")?)?;
                    if target.class != string(f, "target_class")? {
                        return Err(Error("local target class differs".into()));
                    }
                    if rec["generation_contract"] == "dc-helper" && f["name"] == "kernel_reference"
                    {
                        let carrier = self.records(owner)?.into_iter().find(|record| {
                            record["role"] == f["target_role"]
                                && record["generation_contract"] == "source-carrier"
                        });
                        if carrier.is_none() {
                            return Err(Error(
                                "DC helper kernel target is not a source carrier".into(),
                            ));
                        }
                    }
                    self.reference(owner, string(f, "target_role")?, string(f, "namespace")?)?
                        .into()
                }
                "registry_reference" => self.registry_ref(owner, f)?.into(),
                "text" => {
                    let role = string(f, "text_role")?;
                    let text = match role {
                        "document-name" => &self.input.document_name,
                        "feature-name" => &self.input.feature_name,
                        "body-name" => &self.input.body_name,
                        "f7676ab1-11d2-3618-a000-1280d694c7c9/profile-key:4" => {
                            &self.input.document_name
                        }
                        "dd1adf96-4b8f-8262-0930-74b9bed6c949/profile-key:41" => {
                            &self.input.body_name
                        }
                        "f7676ab2-11d2-3618-a000-1280d694c7c9/profile-key:40" => {
                            &self.input.feature_name
                        }
                        _ => return Err(Error("unknown name role".into())),
                    };
                    Value::String(text.clone())
                }
                "fresh-next-document" => 16.into(),
                "fresh-next-label" => 12.into(),
                "fresh-feature-counter" => 1.into(),
                "topology-label-count" => {
                    // This is faces plus closed edges, not all edges of the
                    // solid. A straight box edge has distinct end vertices.
                    let labels = self
                        .shape
                        .model()
                        .entities()
                        .iter()
                        .filter(|entity| match entity {
                            acis_core::entities::Entity::Face(_) => true,
                            acis_core::entities::Entity::Edge(edge) => {
                                edge.start_vertex == edge.end_vertex
                            }
                            _ => false,
                        })
                        .count();
                    self.label_count
                        .unwrap_or(u32::try_from(labels).map_err(err)?)
                        .into()
                }
                "body_document_id" => 2.into(),
                "body-bounds" => {
                    let coordinate = number(&f["coordinate"])? as usize;
                    geometry::bounds(self)
                        .get(coordinate)
                        .copied()
                        .ok_or_else(|| Error("invalid body bounds coordinate".into()))?
                        .into()
                }
                "browser_group_terminal" => 0.into(),
                "view_state" => 0x3000u32.into(),
                "geometry" => geometry::field(self, recipe, f)?,
                other => return Err(Error(format!("unknown field policy {other}"))),
            };
            w.field(string(f, "kind")?, &value)?;
        }
        Ok(w.0)
    }
    pub fn records(&self, owner: &str) -> Result<Vec<&Value>> {
        Ok(array(&self.profile.0, "records")?
            .iter()
            .filter(|r| r["owner"] == owner)
            .collect())
    }
}
