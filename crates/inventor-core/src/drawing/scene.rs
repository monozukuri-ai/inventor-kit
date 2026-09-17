//! Opt-in interpretation of the observed major31 stored-display layout.
//! This is not a qualified active drawing state or a model reprojection API.
use super::*;
use crate::{
    assembly::{compose, Matrix, IDENTITY},
    property::guid,
    rse, Error, Limits, Result,
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;

#[derive(Debug, Serialize)]
pub struct ExperimentalScene {
    pub status: &'static str,
    pub qualified: bool,
    pub source_sha256: String,
    pub units: &'static str,
    pub spaces: Vec<DisplaySpace>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct DisplaySpace {
    pub segment_id: String,
    pub extent_candidate: [f64; 2],
    pub bindings: Vec<DisplayBinding>,
    pub items: Vec<DisplayItem>,
    pub omitted: Vec<Omission>,
}
#[derive(Debug, Serialize)]
pub struct DisplayBinding {
    pub placement_record: usize,
    pub display_reference: u32,
    pub target_segment: String,
    pub target_record: usize,
    pub sources: Vec<SourceSpan>,
}
#[derive(Debug, Serialize)]
pub struct Omission {
    pub segment_id: String,
    pub record_ordinal: usize,
    pub reason: &'static str,
}
#[derive(Debug, Serialize)]
pub struct DisplayItem {
    pub segment_id: String,
    pub record_ordinal: usize,
    pub source: SourceSpan,
    pub placement_record: usize,
    pub group_path: Vec<usize>,
    pub transform: Matrix,
    pub geometry: DisplayGeometry,
    pub style: DisplayStyle,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DisplayGeometry {
    Image {
        reference: u32,
        format: u8,
        origin: [f64; 3],
        u: [f64; 3],
        v: [f64; 3],
    },
    Polyline {
        points: Vec<[f64; 3]>,
    },
    Curve {
        center: [f64; 3],
        u: [f64; 3],
        v: [f64; 3],
        start: f64,
        end: f64,
    },
    Text {
        text: String,
        position: [f64; 3],
        direction: [f64; 3],
        up: [f64; 3],
        raw_flags: u16,
        font: Option<DisplayFont>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct DisplayFont {
    pub id: u32,
    pub family: String,
    pub height_candidate: f64,
    pub weight_candidate: u16,
    pub width_factor: Option<f64>,
    pub flags: u16,
    pub source: SourceSpan,
}

pub(super) fn error(message: &str) -> Error {
    Error(message.into())
}
pub(super) fn field<'a>(o: &'a PayloadObservation, name: &str) -> Result<&'a FieldValue> {
    let mut it = o.fields.iter().filter(|f| f.name == name);
    let f = it.next().ok_or_else(|| error("missing display field"))?;
    if it.next().is_some() {
        return Err(error("duplicate display field"));
    }
    Ok(&f.value)
}
pub(super) fn word(o: &PayloadObservation, name: &str) -> Result<u32> {
    match field(o, name)? {
        FieldValue::U32(v) if v.len() == 1 => Ok(v[0]),
        _ => Err(error("invalid display word")),
    }
}
pub(super) fn references<'a>(o: &'a PayloadObservation, name: &str) -> Result<&'a [u32]> {
    match field(o, name)? {
        FieldValue::U32(v) => Ok(v),
        _ => Err(error("invalid display references")),
    }
}
pub(super) fn doubles<'a>(o: &'a PayloadObservation, name: &str, n: usize) -> Result<&'a [f64]> {
    match field(o, name)? {
        FieldValue::F64(v) if v.len() == n && v.iter().all(|x| x.is_finite()) => Ok(v),
        _ => Err(error("invalid display numbers")),
    }
}
fn matrix(o: &PayloadObservation) -> Result<Matrix> {
    let FieldValue::CompactTransform { values, .. } = field(o, "transform_candidate")? else {
        return Err(error("missing display transform"));
    };
    let mut m = IDENTITY;
    for (r, row) in m.iter_mut().enumerate() {
        row.copy_from_slice(&values[4 * r..4 * r + 4]);
    }
    compose(&IDENTITY, &m)
}
fn transform(m: &Matrix, p: &[f64], w: f64) -> Result<[f64; 3]> {
    let mut q = [0.; 3];
    for i in 0..3 {
        q[i] = (0..3).map(|j| m[i][j] * p[j]).sum::<f64>() + m[i][3] * w;
    }
    if q.iter().any(|v| !v.is_finite()) {
        return Err(error("display transform overflow"));
    }
    Ok(q)
}
fn slot(raw: u32) -> Result<usize> {
    if raw & 0x80000000 == 0 {
        return Err(error("unsupported local display reference tag"));
    }
    (raw as usize & 0x7fffffff)
        .checked_sub(1)
        .ok_or_else(|| error("zero tagged display reference"))
}
fn by_ordinal<'a>(
    s: &'a SegmentInventory,
    work: &mut usize,
) -> Result<BTreeMap<usize, &'a PayloadObservation>> {
    rse::charge(work, s.observations.len())?;
    let mut map = BTreeMap::new();
    for o in &s.observations {
        if o.status != "unqualified" || map.insert(o.record_ordinal, o).is_some() {
            return Err(error("ambiguous display observation"));
        }
    }
    Ok(map)
}
fn table_entry(
    meta: &MetaInventory,
    section: u8,
    index: usize,
    size: usize,
) -> Result<(&[u8], SourceSpan)> {
    let mut tables = meta
        .reference_tables
        .iter()
        .filter(|t| t.section == section);
    let t = tables
        .next()
        .ok_or_else(|| error("missing display reference table"))?;
    if tables.next().is_some()
        || t.count.checked_mul(size) != Some(t.bytes.len())
        || index >= t.count
    {
        return Err(error("invalid display reference table/index"));
    }
    let start = index * size;
    let mut source = t.source.clone();
    source.start_offset = source
        .start_offset
        .checked_add(start)
        .ok_or_else(|| error("display reference source overflow"))?;
    source.end_offset = source
        .start_offset
        .checked_add(size)
        .filter(|end| *end <= t.source.end_offset)
        .ok_or_else(|| error("display reference escapes source span"))?;
    Ok((&t.bytes[start..start + size], source))
}
pub(super) fn short(b: &[u8]) -> u16 {
    u16::from_le_bytes(b[..2].try_into().unwrap())
}
pub(super) fn long(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}

pub(super) fn resolve<'a>(
    doc: &'a DrawingInventory,
    sm: &SegmentInventory,
    raw: u32,
    kind: &str,
    work: &mut usize,
) -> Result<(
    &'a SegmentInventory,
    &'a PayloadObservation,
    Vec<SourceSpan>,
    bool,
)> {
    let index = raw
        .checked_sub(1)
        .ok_or_else(|| error("null display binding"))? as usize;
    let meta = sm
        .meta
        .as_ref()
        .ok_or_else(|| error("missing placement metadata"))?;
    let (object, object_source) = table_entry(meta, 10, index, 8)?;
    let (namespace, namespace_source) = table_entry(meta, 8, short(object) as usize, 20)?;
    let (segment, segment_source) = table_entry(meta, 7, short(&namespace[16..]) as usize, 32)?;
    let same_context = namespace[..16] == segment[16..32];
    // Layer caches can name an older revision. Retain that uncertainty; geometry
    // bindings still require the matching context and never use this exception.
    if !same_context && kind != "AppSegmentType" {
        return Err(error("external display context is not admitted"));
    }
    let id = guid(&segment[..16]);
    rse::charge(work, doc.segments.len())?;
    let mut targets = doc.segments.iter().filter(|s| s.registry.id == id);
    let target = targets
        .next()
        .ok_or_else(|| error("missing display segment"))?;
    if targets.next().is_some()
        || target.status != "framed"
        || target.registry.major != 31
        || target.registry.kind != kind
    {
        return Err(error("ambiguous or unsupported display segment"));
    }
    rse::charge(work, target.observations.len())?;
    let mut objects = target.observations.iter().filter(|o| {
        word(o, "header_flags").ok() == Some(long(&object[2..6]))
            && matches!(field(o,"object_id"),Ok(FieldValue::U16(id)) if *id==short(&object[6..]))
    });
    let root = objects
        .next()
        .ok_or_else(|| error("display object key is not decoded"))?;
    if objects.next().is_some() {
        return Err(error("ambiguous display object key"));
    }
    Ok((
        target,
        root,
        vec![
            object_source,
            namespace_source,
            segment_source,
            root.source.clone(),
        ],
        same_context,
    ))
}
fn bind<'a>(
    doc: &'a DrawingInventory,
    sm: &SegmentInventory,
    placement: &PayloadObservation,
    work: &mut usize,
) -> Result<(&'a SegmentInventory, usize, DisplayBinding)> {
    let raw = word(placement, "display_reference")?;
    let (target, root, mut sources, _) = resolve(doc, sm, raw, "DlSheetDlSegmentType", work)?;
    if root.proposed_role != "display_group_candidate" || word(root, "owner_reference")? != 0 {
        return Err(error("display object key/root is ambiguous"));
    }
    sources.insert(0, placement.source.clone());
    let binding = DisplayBinding {
        placement_record: placement.record_ordinal,
        display_reference: raw,
        target_segment: target.registry.id.clone(),
        target_record: root.record_ordinal,
        sources,
    };
    Ok((target, root.record_ordinal, binding))
}

fn fonts(doc: &DrawingInventory, work: &mut usize) -> Result<BTreeMap<u32, DisplayFont>> {
    rse::charge(work, doc.segments.len())?;
    for s in &doc.segments {
        rse::charge(work, s.observations.len())?;
    }
    let tables: Vec<_> = doc
        .segments
        .iter()
        .filter(|s| {
            s.status == "framed"
                && s.registry.kind == "DlDirectorySegmentType"
                && s.registry.major == 31
        })
        .flat_map(|s| s.observations.iter())
        .filter(|o| o.proposed_role == "font_table_candidate")
        .collect();
    if tables.is_empty() {
        return Ok(BTreeMap::new());
    }
    if tables.len() != 1 {
        return Err(error("ambiguous font directory"));
    }
    let mut result = BTreeMap::new();
    let fields = &tables[0].fields;
    let Some((last, entries)) = fields.split_last() else {
        return Err(error("empty font directory"));
    };
    if last.name != "font_next_id" || entries.len() % 7 != 0 {
        return Err(error("unsupported font directory fields"));
    }
    for row in entries.chunks_exact(7) {
        rse::charge(work, 1)?;
        let [id, tag, weight, flags, size, name, tail] = row else {
            unreachable!()
        };
        if [
            id.name,
            tag.name,
            weight.name,
            flags.name,
            size.name,
            name.name,
            tail.name,
        ] != [
            "font_id",
            "font_tag",
            "font_weight",
            "font_flags",
            "font_size_parameters",
            "font_name",
            "font_tail_parameters",
        ] {
            return Err(error("unsupported font directory layout"));
        }
        let (
            FieldValue::U32(ids),
            FieldValue::U16(4),
            FieldValue::U16(weight),
            FieldValue::F32(size),
            FieldValue::Utf16(name),
        ) = (
            &id.value,
            &tag.value,
            &weight.value,
            &size.value,
            &name.value,
        )
        else {
            return Err(error("unsupported font directory values"));
        };
        if ids.len() != 1 || size.len() != 2 || !size[0].is_finite() || size[0] <= 0. {
            return Err(error("invalid font size or identifier"));
        }
        rse::charge(work, name.len())?;
        let FieldValue::U16(raw_flags) = flags.value else {
            return Err(error("invalid font flags"));
        };
        if !size[1].is_finite() || size[1] < 0. {
            return Err(error("invalid font width factor"));
        }
        let mut source = id.source.clone();
        source.end_offset = tail.source.end_offset;
        if result
            .insert(
                ids[0],
                DisplayFont {
                    id: ids[0],
                    family: name.clone(),
                    height_candidate: size[0] as f64,
                    weight_candidate: *weight,
                    width_factor: (size[1] > 0.).then_some(size[1] as f64),
                    flags: raw_flags,
                    source,
                },
            )
            .is_some()
        {
            return Err(error("duplicate font identifier"));
        }
    }
    Ok(result)
}

fn geometry(
    o: &PayloadObservation,
    m: &Matrix,
    fonts: &BTreeMap<u32, DisplayFont>,
    work: &mut usize,
) -> Result<Option<DisplayGeometry>> {
    match o.proposed_role {
        "stored_polyline_candidate" | "stored_line_candidate" => {
            let values: Vec<f64> = if o.proposed_role == "stored_line_candidate" {
                doubles(o, "line_endpoints", 6)?.to_vec()
            } else {
                match field(o, "xyz_points_candidate")? {
                    FieldValue::F32(v) if v.len() % 3 == 0 => v.iter().map(|x| *x as f64).collect(),
                    _ => return Err(error("invalid display polyline")),
                }
            };
            rse::charge(work, values.len())?;
            let points = values
                .chunks_exact(3)
                .map(|p| transform(m, p, 1.))
                .collect::<Result<_>>()?;
            Ok(Some(DisplayGeometry::Polyline { points }))
        }
        "stored_text_candidate" => {
            let FieldValue::Utf16(text) = field(o, "text")? else {
                return Err(error("invalid display text"));
            };
            rse::charge(work, text.len() + 6)?;
            let v = doubles(o, "position_and_direction_candidate", 6)?;
            let position = transform(m, &v[..3], 1.)?;
            let direction = transform(m, &v[3..], 0.)?;
            if v[5] != 0. || (v[3] * v[3] + v[4] * v[4] - 1.).abs() > 1e-9 {
                return Err(error("unsupported text plane/direction"));
            }
            let up = transform(m, &[-v[4], v[3], 0.], 0.)?;
            let FieldValue::U16(raw_flags) = field(o, "raw_text_flags")? else {
                return Err(error("invalid text flags"));
            };
            let font = fonts.get(&word(o, "style_index_candidate")?);
            if let Some(f) = font {
                rse::charge(work, f.family.len())?;
            }
            let font = font.cloned();
            Ok(Some(DisplayGeometry::Text {
                text: text.clone(),
                position,
                direction,
                up,
                raw_flags: *raw_flags,
                font,
            }))
        }
        "stored_circle_candidate" | "stored_arc_candidate" => {
            rse::charge(work, 12)?;
            let is_arc = o.proposed_role == "stored_arc_candidate";
            let v = if is_arc {
                doubles(o, "arc_center_normal_axis_radius_angles", 12)?
            } else {
                doubles(o, "circle_center_normal_radius", 7)?
            };
            let n: &[f64] = &v[3..6];
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
            let cross = |a: &[f64], b: &[f64]| {
                [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ]
            };
            if (dot(n, n) - 1.).abs() > 1e-9 {
                return Err(error("non-unit stored curve normal"));
            }
            let axis = if is_arc {
                <[f64; 3]>::try_from(&v[6..9]).unwrap()
            } else {
                let seed = if n[0].abs() < 0.9 {
                    [1., 0., 0.]
                } else {
                    [0., 1., 0.]
                };
                let a = cross(n, &seed);
                let len = dot(&a, &a).sqrt();
                a.map(|x| x / len)
            };
            if (dot(&axis, &axis) - 1.).abs() > 1e-9 || dot(n, &axis).abs() > 1e-9 {
                return Err(error("invalid stored arc basis"));
            }
            let radius = v[if is_arc { 9 } else { 6 }];
            let (start, end) = if is_arc {
                (v[10], v[11])
            } else {
                (0., std::f64::consts::TAU)
            };
            if radius <= 0. || end <= start || end - start > std::f64::consts::TAU + 1e-9 {
                return Err(error("unsupported stored curve range"));
            }
            let center = transform(m, &v[..3], 1.)?;
            let u = transform(m, &axis.map(|x| x * radius), 0.)?;
            let v = transform(m, &cross(n, &axis).map(|x| x * radius), 0.)?;
            Ok(Some(DisplayGeometry::Curve {
                center,
                u,
                v,
                start,
                end,
            }))
        }
        _ => Ok(None),
    }
}

// Bidirectional, unique ownership is required before a branch is interpreted.
fn owners<'a>(
    s: &'a SegmentInventory,
    work: &mut usize,
) -> Result<BTreeMap<usize, &'a PayloadObservation>> {
    let map = by_ordinal(s, work)?;
    rse::charge(work, s.records.len())?;
    let ordinals: BTreeSet<_> = s.records.iter().map(|r| r.ordinal).collect();
    let mut incoming = BTreeMap::new();
    for (&parent, o) in &map {
        if o.proposed_role != "display_group_candidate" {
            continue;
        }
        for &raw in references(o, "child_references_unresolved")? {
            rse::charge(work, 1)?;
            if raw == 0 {
                continue;
            }
            let child = slot(raw)?;
            if incoming.insert(child, parent).is_some() {
                return Err(error("duplicate/multiple display parents"));
            }
            if let Some(target) = map.get(&child) {
                if word(target, "owner_reference")? != 0x80000000 | (parent as u32 + 1) {
                    return Err(error("display owner backlink mismatch"));
                }
            } else if !ordinals.contains(&child) {
                return Err(error("dangling display child"));
            }
        }
    }
    // Also reject missing forward links and cycles outside the selected roots.
    for (&id, o) in &map {
        let Ok(raw) = word(o, "owner_reference") else {
            continue;
        };
        if raw != 0 && incoming.get(&id) != Some(&slot(raw)?) {
            return Err(error("display owner lacks matching forward reference"));
        }
        let mut at = id;
        let mut seen = BTreeSet::new();
        while let Some(parent) = incoming.get(&at) {
            rse::charge(work, 1)?;
            if !seen.insert(at) || seen.len() > 128 {
                return Err(error("cyclic/deep display ownership"));
            }
            at = *parent;
        }
    }
    Ok(map)
}

fn display_branch(
    space: &mut DisplaySpace,
    doc: &DrawingInventory,
    target: &SegmentInventory,
    binding: &DisplayBinding,
    parent: Matrix,
    fonts: &BTreeMap<u32, DisplayFont>,
    work: &mut usize,
) -> Result<()> {
    let root = binding.target_record;
    let placement = binding.placement_record;
    let nodes = owners(target, work)?;
    let mut stack = vec![(root, parent, Vec::new(), DisplayStyle::default())];
    let mut visited = BTreeSet::new();
    while let Some((id, mut m, mut path, inherited)) = stack.pop() {
        rse::charge(work, 1)?;
        if !visited.insert(id) || path.len() > 128 {
            return Err(error("repeated/deep display branch"));
        }
        let Some(o) = nodes.get(&id) else {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: "display_type_not_decoded",
            });
            continue;
        };
        let style = super::appearance::apply(doc, target, o, &nodes, &inherited, work)?;
        if !style.visible {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: "hidden_by_stored_attribute",
            });
            if o.proposed_role == "display_group_candidate" {
                path.push(id);
                for &raw in references(o, "child_references_unresolved")?.iter().rev() {
                    if raw != 0 {
                        rse::charge(work, path.len() + style.cost() + 1)?;
                        stack.push((slot(raw)?, m, path.clone(), style.clone()));
                    }
                }
            }
            continue;
        }
        if o.proposed_role == "display_group_candidate" {
            match field(o, "transform_branch")? {
                FieldValue::U8(0) => (), // Experimental grammar: absent local matrix inherits parent.
                FieldValue::U8(1) => {
                    rse::charge(work, 16)?;
                    m = compose(&m, &matrix(o)?)?;
                }
                _ => return Err(error("unsupported display group branch")),
            }
            path.push(id);
            for &raw in references(o, "child_references_unresolved")?.iter().rev() {
                if raw != 0 {
                    rse::charge(work, path.len() + style.cost() + 1)?;
                    stack.push((slot(raw)?, m, path.clone(), style.clone()));
                }
            }
        } else if let Some(geometry) = geometry(o, &m, fonts, work)? {
            space.items.push(DisplayItem {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                source: o.source.clone(),
                placement_record: placement,
                group_path: path,
                transform: m,
                geometry,
                style,
            });
        } else {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: "unsupported_display_role",
            });
        }
    }
    Ok(())
}

/// Experimental major31 scene. Stored display coordinates/transforms only;
/// no external model loading, no active-state claim and no fitted placement.
pub fn experimental_scene(doc: &DrawingInventory, limits: &Limits) -> ExperimentalScene {
    let mut out = ExperimentalScene {
        status: "unavailable",
        qualified: false,
        source_sha256: doc.source_sha256.clone(),
        units: "source_units_unverified",
        spaces: vec![],
        diagnostics: vec![],
    };
    let mut work = limits.max_records;
    let fonts = match fonts(doc, &mut work) {
        Ok(f) => f,
        Err(e) => {
            out.diagnostics.push(e.to_string());
            BTreeMap::new()
        }
    };
    for sm in doc.segments.iter().filter(|s| {
        s.status == "framed" && s.registry.major == 31 && s.registry.kind == "DlSheetSmSegmentType"
    }) {
        let result = (|| -> Result<DisplaySpace> {
            let nodes = by_ordinal(sm, &mut work)?;
            rse::charge(&mut work, sm.records.len())?;
            let ordinals: BTreeSet<_> = sm.records.iter().map(|r| r.ordinal).collect();
            let roots: Vec<_> = nodes
                .values()
                .filter(|o| o.proposed_role == "sheet_space_candidate")
                .collect();
            if roots.len() != 1 {
                return Err(error("ambiguous stored sheet-space root"));
            }
            let root = roots[0];
            let rect = doubles(root, "origin_and_extent_candidate", 4)?;
            if rect[2] <= 0. || rect[3] <= 0. {
                return Err(error("invalid display extent"));
            }
            let mut space = DisplaySpace {
                segment_id: sm.registry.id.clone(),
                extent_candidate: [rect[2], rect[3]],
                bindings: vec![],
                items: vec![],
                omitted: vec![],
            };
            let mut stack = vec![(root.record_ordinal, IDENTITY, 0usize)];
            let mut visited = BTreeSet::new();
            let mut bound = BTreeSet::new();
            while let Some((id, parent, depth)) = stack.pop() {
                rse::charge(&mut work, 1)?;
                if depth > 128 || !visited.insert(id) {
                    return Err(error("repeated/cyclic sheet placement"));
                }
                let Some(node) = nodes.get(&id) else {
                    if !ordinals.contains(&id) {
                        return Err(error("dangling sheet placement"));
                    }
                    space.omitted.push(Omission {
                        segment_id: sm.registry.id.clone(),
                        record_ordinal: id,
                        reason: "sheet_node_not_decoded",
                    });
                    continue;
                };
                if node.proposed_role == "stored_image_candidate" {
                    let size = doubles(node, "image_height_width", 2)?;
                    let reference = word(node, "image_reference")?;
                    let FieldValue::U8(format) = field(node, "image_format")? else {
                        return Err(error("invalid image format"));
                    };
                    if size.iter().any(|x| *x <= 0.) || reference == 0 || !matches!(format, 0 | 2) {
                        return Err(error("unsupported image placement"));
                    }
                    rse::charge(&mut work, 32)?;
                    let world = compose(&parent, &matrix(node)?)?;
                    space.items.push(DisplayItem {
                        segment_id: sm.registry.id.clone(),
                        record_ordinal: id,
                        source: node.source.clone(),
                        placement_record: id,
                        group_path: vec![],
                        transform: world,
                        style: DisplayStyle::default(),
                        geometry: DisplayGeometry::Image {
                            reference,
                            format: *format,
                            origin: transform(&world, &[0., 0., 0.], 1.)?,
                            u: transform(&world, &[size[1], 0., 0.], 0.)?,
                            v: transform(&world, &[0., -size[0], 0.], 0.)?,
                        },
                    });
                    continue;
                }
                if !matches!(
                    node.proposed_role,
                    "sheet_space_candidate" | "sheet_placement_candidate"
                ) {
                    return Err(error("unsupported sheet placement role"));
                }
                let world = compose(&parent, &matrix(node)?)?;
                let (target, root, binding) = bind(doc, sm, node, &mut work)?;
                if !bound.insert((target.registry.id.clone(), root)) {
                    return Err(error(
                        "repeated display-root instance needs explicit semantics",
                    ));
                }
                display_branch(&mut space, doc, target, &binding, world, &fonts, &mut work)?;
                space.bindings.push(binding);
                for &raw in references(node, "references_unresolved")?.iter().rev() {
                    rse::charge(&mut work, 1)?;
                    if raw != 0 {
                        stack.push((slot(raw)?, world, depth + 1));
                    }
                }
            }
            // Parsed but unreachable primitives are not promoted to visible objects.
            rse::charge(
                &mut work,
                space.items.len() + space.omitted.len() + space.bindings.len() + doc.segments.len(),
            )?;
            let emitted: BTreeSet<_> = space
                .items
                .iter()
                .map(|i| (i.segment_id.clone(), i.record_ordinal))
                .chain(
                    space
                        .omitted
                        .iter()
                        .map(|i| (i.segment_id.clone(), i.record_ordinal)),
                )
                .collect();
            let bound_segments: BTreeSet<_> = space
                .bindings
                .iter()
                .map(|b| b.target_segment.as_str())
                .collect();
            for target in doc
                .segments
                .iter()
                .filter(|s| bound_segments.contains(s.registry.id.as_str()))
            {
                rse::charge(&mut work, target.observations.len())?;
                for o in &target.observations {
                    if o.proposed_role.starts_with("stored_")
                        && !emitted.contains(&(target.registry.id.clone(), o.record_ordinal))
                    {
                        space.omitted.push(Omission {
                            segment_id: target.registry.id.clone(),
                            record_ordinal: o.record_ordinal,
                            reason: "not_reached_from_stored_sheet_bindings",
                        });
                    }
                }
            }
            Ok(space)
        })();
        match result {
            Ok(space) => out.spaces.push(space),
            Err(e) => out.diagnostics.push(format!("{}: {}", sm.registry.id, e)),
        }
    }
    if !out.spaces.is_empty() {
        out.status = "experimental_partial";
    }
    out
}
