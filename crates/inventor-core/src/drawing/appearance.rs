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
#[allow(clippy::too_many_arguments)] // Shared immutable indexes and per-document work allowance.
pub(super) fn apply<'a>(
    doc: &'a DrawingInventory,
    segment: &SegmentInventory,
    object: &PayloadObservation,
    nodes: &BTreeMap<usize, &PayloadObservation>,
    ordinals: &BTreeSet<usize>,
    parent: &DisplayStyle,
    work: &mut usize,
    cache: &mut ResolveCache<'a>,
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
            rse::charge(work, 1)?;
            if !ordinals.contains(&(refs[0] as usize - 1)) {
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
                let (target, layer, sources, identity_verified) = resolve_cached(
                    doc,
                    segment,
                    word(attr, "layer_reference")?,
                    "AppSegmentType",
                    work,
                    cache,
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
                } else if target.registry.major == 31 && scale == 1. {
                    let dash = layer_dashes(layer)?;
                    rse::charge(work, dash.as_ref().map_or(0, Vec::len))?;
                    if dash.is_none() {
                        style.unresolved.push("layer_dash_pattern_not_decoded");
                    }
                    dash
                } else {
                    style.unresolved.push("layer_dash_pattern_not_decoded");
                    None
                };
            }
            "stroke_override_candidate" => {
                let mask = word(attr, "stroke_mask")?;
                if mask == 8 {
                    let width = floats(attr, "stroke_width", 1)?[0] as f64;
                    // A default sketch style writes -1 for inherited width.
                    // Admit only the complete major31 tuple observed in native
                    // controls; other nonpositive values still fail closed.
                    let inherited = width == -1.
                        && segment.registry.major == 31
                        && default_sketch_stroke(attr)?;
                    if width <= 0. && !inherited {
                        return Err(error("invalid stroke override width"));
                    }
                    if !inherited {
                        style.width = Some(width);
                    }
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
    style
        .unresolved
        .retain(|v| *v != "dash_phase_and_fit_unverified");
    if style.dash.as_ref().is_some_and(|v| !v.is_empty()) {
        style.unresolved.push("dash_phase_and_fit_unverified");
    }
    style.unresolved.sort_unstable();
    style.unresolved.dedup();
    Ok(style)
}
// Nominal pattern lengths measured from the paired native major31 layer and
// override controls. PDF export adjusts periods and phase to fit each curve;
// these arrays preserve the nominal stored style, not that fitting algorithm.
fn layer_dashes(layer: &PayloadObservation) -> Result<Option<Vec<f64>>> {
    if doubles(layer, "layer_scale", 1)? != [1.]
        || !matches!(field(layer, "layer_flag_b")?, FieldValue::U8(1))
        || !matches!(field(layer, "layer_flag_c")?, FieldValue::U8(0))
    {
        return Ok(None);
    }
    let base = match field(layer, "layer_flag_a")? {
        FieldValue::U8(0) => 0.038_f32 as f64,
        FieldValue::U8(1) => doubles(layer, "layer_width", 1)?[0],
        _ => return Ok(None),
    };
    let pattern: &[f64] = match word(layer, "line_pattern")? {
        28101 => &[12., 3.],
        28102 => &[12., 12.],
        28103 => &[24., 3., 0.5, 3.],
        28104 => &[24., 3., 0.5, 3., 0.5, 3.],
        28105 => &[24., 3., 0.5, 3., 0.5, 3., 0.5, 3.],
        28106 => &[0.5, 3.],
        28107 => &[24., 3., 6., 3.],
        28108 => &[24., 3., 6., 3., 6., 3.],
        28109 => &[12., 3., 0.5, 3., 0.5, 3.],
        28110 => &[12., 3., 0.5, 3.],
        28111 => &[12., 3., 12., 3., 0.5, 3.],
        28112 => &[12., 3., 12., 3., 0.5, 3., 0.5, 3.],
        28113 => &[12., 3., 0.5, 3., 0.5, 3., 0.5, 3.],
        28114 => &[12., 3., 12., 3., 0.5, 3., 0.5, 3., 0.5, 3.],
        _ => return Ok(None),
    };
    let dash: Vec<_> = pattern.iter().map(|v| v * base).collect();
    if dash.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err(error("invalid layer dash lengths"));
    }
    Ok(Some(dash))
}
fn default_sketch_stroke(o: &PayloadObservation) -> Result<bool> {
    Ok(matches!(field(o, "stroke_flags")?, FieldValue::U16(0))
        && matches!(field(o, "stroke_mode")?, FieldValue::U16(1))
        && matches!(field(o, "stroke_pattern")?, FieldValue::U16(u16::MAX))
        && matches!(field(o, "stroke_byte")?, FieldValue::U8(0))
        && matches!(field(o, "stroke_list_type")?, FieldValue::U16(2))
        && matches!(field(o, "stroke_dashes")?, FieldValue::F64(v) if v.is_empty())
        && floats(o, "stroke_parameters", 3)? == [-10000., 1., -0.038]
        && word(o, "stroke_pattern_copy")? == u32::MAX
        && word(o, "stroke_kind")? == 6)
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
