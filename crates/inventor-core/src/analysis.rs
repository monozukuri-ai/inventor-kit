//! Bounded geometry-dependency inventory, not a proof of convertibility.
use acis_core::{AcisModel, Entity, EntityRef, SourceSpan};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct UnknownEntity {
    pub entity_index: usize,
    pub type_name: String,
    pub source: Option<SourceSpanInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceSpanInfo {
    pub source_id: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl From<&SourceSpan> for SourceSpanInfo {
    fn from(s: &SourceSpan) -> Self {
        Self {
            source_id: s.source_id.clone(),
            start_offset: s.start_offset,
            end_offset: s.end_offset,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingReference {
    pub entity_index: usize,
    pub field: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct BodyAnalysis {
    pub body_index: usize,
    pub visited_entities: usize,
    pub complete_traversal: bool,
    pub unresolved_geometry: Vec<UnknownEntity>,
    pub null_geometry_references: Vec<MissingReference>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelAnalysis {
    pub active_records: usize,
    pub typed_records: usize,
    pub raw_types: BTreeMap<String, usize>,
    pub opaque_history: bool,
    pub complete_traversal: bool,
    pub bodies: Vec<BodyAnalysis>,
}

// Follow ownership/geometry edges only. Attribute chains, pattern links,
// back-pointers and partner coedges can cross body boundaries. Raw records on
// this walk remain unresolved; do not guess their subtype's reference roles.
fn children(entity: &Entity) -> Vec<(&'static str, EntityRef, bool)> {
    use Entity::*;
    match entity {
        Body(e) => vec![
            ("lump", e.lump, false),
            ("wire", e.wire, false),
            ("transform", e.transform, false),
        ],
        Lump(e) => vec![("next_lump", e.next_lump, false), ("shell", e.shell, true)],
        Shell(e) => vec![
            ("next_shell", e.next_shell, false),
            ("subshell", e.subshell, false),
            ("face", e.face, false),
            ("wire", e.wire, false),
        ],
        Face(e) => vec![
            ("next_face", e.next_face, false),
            ("loop", e.loop_ref, false),
            ("surface", e.surface, true),
        ],
        Loop(e) => vec![
            ("next_loop", e.next_loop, false),
            ("coedge", e.coedge, true),
        ],
        Coedge(e) => vec![
            ("next_coedge", e.next_coedge, true),
            ("edge", e.edge, true),
            ("pcurve", e.pcurve, false),
        ],
        Edge(e) => vec![
            ("start_vertex", e.start_vertex, false),
            ("end_vertex", e.end_vertex, false),
            ("curve", e.curve, true),
        ],
        Vertex(e) => vec![("point", e.point, true)],
        _ => vec![],
    }
}

/// Inventory every body with a shared traversal budget. Null curve references
/// can describe unsupported degeneracies; they are observations, not proof of
/// corruption. An unknown subtype stops that branch and makes traversal partial.
pub fn analyze_model(model: &AcisModel, max_visits: usize) -> ModelAnalysis {
    let mut result = ModelAnalysis {
        active_records: 0,
        typed_records: 0,
        raw_types: BTreeMap::new(),
        opaque_history: false,
        complete_traversal: true,
        bodies: Vec::new(),
    };
    for entity in model.entities() {
        if entity.raw().type_name == "__opaque_sab_history__" {
            result.opaque_history = true;
            continue;
        }
        result.active_records += 1;
        if let Entity::Raw(raw) = entity {
            *result.raw_types.entry(raw.type_name.clone()).or_default() += 1;
        } else {
            result.typed_records += 1;
        }
    }
    let mut remaining = max_visits;
    for body in model.bodies() {
        let mut analysis = BodyAnalysis {
            body_index: body.raw.index,
            visited_entities: 0,
            complete_traversal: true,
            unresolved_geometry: Vec::new(),
            null_geometry_references: Vec::new(),
        };
        let mut visited = BTreeSet::new();
        let mut pending = vec![body.raw.index];
        while let Some(index) = pending.pop() {
            if visited.contains(&index) {
                continue;
            }
            if remaining == 0 {
                analysis.complete_traversal = false;
                break;
            }
            remaining -= 1;
            visited.insert(index);
            let entity = &model.entities()[index];
            if let Entity::Raw(raw) = entity {
                analysis.unresolved_geometry.push(UnknownEntity {
                    entity_index: index,
                    type_name: raw.type_name.clone(),
                    source: raw.source.as_ref().map(Into::into),
                });
                analysis.complete_traversal = false;
            }
            for (field, reference, required) in children(entity) {
                if reference.0 == -1 {
                    if required {
                        analysis.null_geometry_references.push(MissingReference {
                            entity_index: index,
                            field,
                        });
                    }
                } else {
                    // AcisModel guarantees closure before it can be inspected.
                    pending.push(reference.0 as usize);
                }
            }
        }
        analysis.visited_entities = visited.len();
        analysis.unresolved_geometry.sort_by_key(|e| e.entity_index);
        analysis
            .null_geometry_references
            .sort_by_key(|e| (e.entity_index, e.field));
        result.complete_traversal &= analysis.complete_traversal;
        result.bodies.push(analysis);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use acis_core::{
        entities::{BodyEntity, LumpEntity},
        AcisMetadata, RawEntity, NULL_REF,
    };

    fn model() -> AcisModel {
        let raw = |i, name| RawEntity::new(i, name);
        let body = |i, lump| {
            Entity::Body(BodyEntity {
                raw: raw(i, "body"),
                pattern: EntityRef(6),
                lump: EntityRef(lump),
                wire: NULL_REF,
                transform: NULL_REF,
            })
        };
        let lump = |i, shell, owner| {
            Entity::Lump(LumpEntity {
                raw: raw(i, "lump"),
                pattern: NULL_REF,
                next_lump: EntityRef(i as i64),
                shell: EntityRef(shell),
                body: EntityRef(owner),
            })
        };
        AcisModel::new(
            AcisMetadata::default(),
            vec![
                body(0, 1),
                lump(1, 2, 0),
                Entity::Raw(raw(2, "unknown-shell-a")),
                body(3, 4),
                lump(4, 5, 3),
                Entity::Raw(raw(5, "unknown-shell-b")),
                Entity::Raw(raw(6, "shared-attribute")),
            ],
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn visits_all_bodies_without_crossing_shared_attributes_or_cycles() {
        let analysis = analyze_model(&model(), 100);
        assert_eq!(analysis.active_records, 7);
        assert_eq!(analysis.typed_records, 4);
        assert_eq!(analysis.bodies.len(), 2);
        assert_eq!(analysis.bodies[0].unresolved_geometry[0].entity_index, 2);
        assert_eq!(analysis.bodies[1].unresolved_geometry[0].entity_index, 5);
        assert_eq!(analysis.bodies[0].visited_entities, 3);
        assert!(!analysis.complete_traversal);
    }

    #[test]
    fn budget_exhaustion_never_claims_complete_traversal() {
        let analysis = analyze_model(&model(), 1);
        assert!(!analysis.complete_traversal);
        assert_eq!(analysis.bodies[0].visited_entities, 1);
        assert_eq!(analysis.bodies[1].visited_entities, 0);
        assert!(analysis.bodies.iter().all(|b| !b.complete_traversal));
    }
}
