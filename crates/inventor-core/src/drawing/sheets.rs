//! Stored sheet order and SM backlinks. Physical units and active state are
//! deliberately separate from the structural identity established here.
use super::{scene, *};
use crate::{rse, Error, Limits, Result};
use std::collections::BTreeSet;

#[derive(Debug, Serialize)]
pub struct StoredSheets {
    pub status: &'static str,
    pub qualified: bool,
    pub length_unit: Option<&'static str>,
    pub millimeters_per_unit: Option<f64>,
    pub sheets: Vec<StoredSheet>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StoredSheet {
    pub id: String,
    pub index: usize,
    pub name: String,
    pub definition_segment: String,
    pub definition_record: usize,
    pub status: &'static str,
    pub space_segment: Option<String>,
    pub size_in_source_units: Option<[f64; 2]>,
    pub sources: Vec<SourceSpan>,
    pub diagnostics: Vec<String>,
}

fn text<'a>(o: &'a PayloadObservation, name: &str) -> Result<&'a str> {
    match scene::field(o, name)? {
        FieldValue::Utf16(value) => Ok(value),
        _ => Err(Error("invalid stored sheet name".into())),
    }
}

/// Follow the document's tagged local sheet list in wire order, then require
/// exactly one same-context SM definition reference to the full object key.
/// Names, registry order, paper-size guesses and thumbnails never select owners.
pub fn stored_sheets(doc: &DrawingInventory, limits: &Limits) -> StoredSheets {
    let mut result = StoredSheets {
        status: "unavailable",
        qualified: false,
        length_unit: None,
        millimeters_per_unit: None,
        sheets: vec![],
        diagnostics: vec!["physical_units_and_active_state_unverified".into()],
    };
    let mut work = limits.max_records;
    match collect(doc, &mut work) {
        Ok(sheets) => {
            result.status = "stored_order_unverified_state";
            result.sheets = sheets;
        }
        Err(e) => result.diagnostics.push(e.to_string()),
    }
    result
}

fn collect(doc: &DrawingInventory, work: &mut usize) -> Result<Vec<StoredSheet>> {
    rse::charge(work, doc.segments.len())?;
    let mut candidates = vec![];
    for segment in &doc.segments {
        if segment.status != "framed"
            || segment.registry.major != 31
            || segment.registry.kind != "DlDocDcSegmentType"
        {
            continue;
        }
        rse::charge(work, segment.observations.len())?;
        for root in &segment.observations {
            if root.proposed_role == "document_sheet_list_candidate" {
                candidates.push((segment, root));
            }
        }
    }
    if candidates.len() != 1 {
        return Err(Error("missing or ambiguous document sheet list".into()));
    }
    let (segment, root) = candidates[0];
    let refs = scene::references(root, "sheet_references_candidate")?;
    rse::charge(work, refs.len())?;
    let mut seen = BTreeSet::new();
    let mut sheets = Vec::with_capacity(refs.len());
    for (index, raw) in refs.iter().copied().enumerate() {
        if raw & 0x80000000 == 0 || raw == 0x80000000 || !seen.insert(raw) {
            return Err(Error("invalid or repeated document sheet reference".into()));
        }
        let ordinal = (raw & 0x7fffffff) as usize - 1;
        rse::charge(work, segment.observations.len())?;
        let mut definitions = segment
            .observations
            .iter()
            .filter(|o| o.record_ordinal == ordinal);
        let definition = definitions
            .next()
            .ok_or_else(|| Error("sheet definition not decoded".into()))?;
        if definitions.next().is_some()
            || definition.proposed_role != "sheet_segment_links_candidate"
        {
            return Err(Error("ambiguous or unsupported sheet definition".into()));
        }
        let name = text(definition, "name")?;
        rse::charge(work, name.len())?;
        let mut sheet = StoredSheet {
            id: format!("sheet-{}-{ordinal}", segment.registry.id),
            index,
            name: name.into(),
            definition_segment: segment.registry.id.clone(),
            definition_record: ordinal,
            status: "unavailable",
            space_segment: None,
            size_in_source_units: None,
            sources: vec![root.source.clone(), definition.source.clone()],
            diagnostics: vec![],
        };
        let mut matches = vec![];
        for sm in &doc.segments {
            rse::charge(work, 1)?;
            if sm.status != "framed"
                || sm.registry.major != 31
                || sm.registry.kind != "DlSheetSmSegmentType"
            {
                continue;
            }
            rse::charge(work, sm.observations.len())?;
            let roots: Vec<_> = sm
                .observations
                .iter()
                .filter(|o| o.proposed_role == "sheet_space_candidate")
                .collect();
            if roots.len() != 1 {
                continue;
            }
            let space = roots[0];
            let binding = scene::word(space, "definition_reference")
                .and_then(|raw| scene::resolve(doc, sm, raw, "DlDocDcSegmentType", work));
            if let Ok((target, object, sources, _)) = binding {
                if target.registry.id == segment.registry.id && object.record_ordinal == ordinal {
                    matches.push((sm, space, sources));
                }
            }
        }
        if matches.len() == 1 {
            let (sm, space, sources) = matches.remove(0);
            let rect = scene::doubles(space, "origin_and_extent_candidate", 4)?;
            if rect[2] <= 0. || rect[3] <= 0. {
                return Err(Error("invalid stored sheet extent".into()));
            }
            sheet.status = "bound_unverified_state";
            sheet.space_segment = Some(sm.registry.id.clone());
            sheet.size_in_source_units = Some([rect[2], rect[3]]);
            sheet.sources.extend(sources);
            sheet.sources.push(space.source.clone());
        } else {
            sheet
                .diagnostics
                .push("missing_or_ambiguous_sheet_space_binding".into());
        }
        sheets.push(sheet);
    }
    // A depleted budget must not be mistaken for an ordinary absent backlink.
    rse::charge(work, 1)?;
    Ok(sheets)
}
