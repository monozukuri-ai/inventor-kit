//! Experimental attribute inheritance. Uninterpreted properties remain explicit.
use super::{scene::*, *};
use crate::{rse, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct DisplayStyle {
    pub visible: bool,
    pub width: Option<f64>,
    pub rgba: Option<[f64; 4]>,
    pub dash: Option<Vec<f64>>,
    pub layer: Option<String>,
    pub sources: Vec<SourceSpan>,
    pub unresolved: Vec<&'static str>,
}
impl DisplayStyle {
    pub(super) fn cost(&self) -> usize {
        self.sources.len()
            + self.unresolved.len()
            + self.dash.as_ref().map_or(0, Vec::len)
            + self.layer.as_ref().map_or(0, String::len)
    }
}
impl Default for DisplayStyle {
    fn default() -> Self {
        Self {
            visible: true,
            width: None,
            rgba: None,
            dash: None,
            layer: None,
            sources: vec![],
            unresolved: vec![],
        }
    }
}
pub(super) fn apply(
    doc: &DrawingInventory,
    segment: &SegmentInventory,
    object: &PayloadObservation,
    nodes: &BTreeMap<usize, &PayloadObservation>,
    parent: &DisplayStyle,
    work: &mut usize,
) -> Result<DisplayStyle> {
    rse::charge(work, parent.cost() + 1)?;
    let mut style = parent.clone();
    // All decoded native display headers have this field; synthetic callers may omit it.
    let raw = if object
        .fields
        .iter()
        .any(|f| f.name == "attribute_reference")
    {
        word(object, "attribute_reference")?
    } else {
        0
    };
    if raw == 0 {
        return Ok(style);
    }
    let list = nodes
        .get(&(raw as usize - 1))
        .ok_or_else(|| error("missing display attributes"))?;
    if list.proposed_role != "display_attributes_candidate" {
        return Err(error("invalid display attribute target"));
    }
    style.sources.push(list.source.clone());
    let mut seen = BTreeSet::new();
    for entry in &list.fields {
        rse::charge(work, 1)?;
        let FieldValue::U32(refs) = &entry.value else {
            return Err(error("invalid attribute reference"));
        };
        if entry.name != "attribute_entry"
            || refs.len() != 1
            || refs[0] == 0
            || !seen.insert(refs[0])
        {
            return Err(error("invalid/duplicate attribute entry"));
        }
        let Some(attr) = nodes.get(&(refs[0] as usize - 1)) else {
            rse::charge(work, segment.records.len())?;
            if !segment
                .records
                .iter()
                .any(|r| r.ordinal == refs[0] as usize - 1)
            {
                return Err(error("dangling display attribute"));
            }
            style.unresolved.push("attribute_type_not_decoded");
            continue;
        };
        style.sources.push(attr.source.clone());
        match attr.proposed_role {
            "display_boolean_candidate" => {
                if word(attr, "attribute_mask")? == 4 {
                    let FieldValue::U8(b) = field(attr, "attribute_boolean")? else {
                        return Err(error("invalid visibility attribute"));
                    };
                    match b {
                        0 => style.visible = false,
                        1 => (),
                        _ => return Err(error("unknown visibility value")),
                    }
                } else {
                    style.unresolved.push("boolean_mask_not_interpreted");
                }
            }
            "layer_binding_candidate" => {
                let (target, layer, sources, identity_verified) = resolve(
                    doc,
                    segment,
                    word(attr, "layer_reference")?,
                    "AppSegmentType",
                    work,
                )?;
                if target.registry.kind != "AppSegmentType"
                    || layer.proposed_role != "layer_definition_candidate"
                {
                    return Err(error("invalid layer reference"));
                }
                if !identity_verified {
                    style.unresolved.push("layer_revision_binding_unverified");
                }
                if word(attr, "layer_binding_mask")? != 1 {
                    style.unresolved.push("layer_binding_mask_unverified");
                }
                let FieldValue::Utf16(name) = field(layer, "layer_name")? else {
                    return Err(error("invalid layer name"));
                };
                rse::charge(work, name.len() + sources.len())?;
                style.layer = Some(name.clone());
                style.sources.extend(sources);
                let width = doubles(layer, "layer_width", 1)?[0];
                let scale = floats(attr, "stroke_scale", 1)?[0] as f64;
                if width <= 0. || scale <= 0. || !(width * scale).is_finite() {
                    return Err(error("invalid layer width/scale"));
                }
                style.width = Some(width * scale);
                let rgba = floats(layer, "layer_rgba", 4)?;
                style.rgba = if rgba == [-2.; 4] {
                    None
                } else {
                    Some(color(rgba)?)
                };
                if word(attr, "color_override")? != u32::MAX {
                    style
                        .unresolved
                        .push("layer_color_override_not_interpreted");
                }
                style.dash = if word(layer, "line_pattern")? == 0x6dc4 {
                    Some(vec![])
                } else {
                    style.unresolved.push("layer_dash_pattern_not_decoded");
                    None
                };
            }
            "stroke_override_candidate" => {
                let mask = word(attr, "stroke_mask")?;
                if mask == 8 {
                    let width = floats(attr, "stroke_width", 1)?[0] as f64;
                    if width <= 0. {
                        return Err(error("invalid stroke override width"));
                    }
                    style.width = Some(width);
                    let FieldValue::F64(dashes) = field(attr, "stroke_dashes")? else {
                        return Err(error("invalid stroke dashes"));
                    };
                    rse::charge(work, dashes.len())?;
                    if dashes.len() % 2 != 0
                        || dashes.iter().enumerate().any(|(i, v)| {
                            !v.is_finite() || if i % 2 == 0 { *v <= 0. } else { *v >= 0. }
                        })
                    {
                        return Err(error("unsupported signed dash sequence"));
                    }
                    style.dash = Some(dashes.iter().map(|x| x.abs()).collect());
                } else {
                    style.unresolved.push("stroke_mask_not_interpreted");
                }
            }
            "display_color_candidate" => {
                if word(attr, "color_mask")? != 1 {
                    style.unresolved.push("display_color_mask_not_interpreted");
                } else {
                    style.rgba = Some(color(&floats(attr, "color_rgba_parameters", 21)?[..4])?);
                }
            }
            _ => style.unresolved.push("attribute_semantics_not_decoded"),
        }
    }
    style.unresolved.sort_unstable();
    style.unresolved.dedup();
    Ok(style)
}
fn floats<'a>(o: &'a PayloadObservation, name: &str, n: usize) -> Result<&'a [f32]> {
    match field(o, name)? {
        FieldValue::F32(v) if v.len() == n && v.iter().all(|x| x.is_finite()) => Ok(v),
        _ => Err(error("invalid style numbers")),
    }
}
fn color(v: &[f32]) -> Result<[f64; 4]> {
    if v.len() != 4 || v.iter().any(|x| !x.is_finite() || *x < 0. || *x > 1.) {
        return Err(error("invalid color"));
    }
    Ok([v[0] as f64, v[1] as f64, v[2] as f64, v[3] as f64])
}
