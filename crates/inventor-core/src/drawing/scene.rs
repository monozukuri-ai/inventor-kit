//! Opt-in interpretation of the observed major23/31 stored-display layouts.
//! This is not a qualified active drawing state or a model reprojection API.
use super::limits::DisplayBudget;
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
    /// Registry-bound cache profiles; never inferred from the compression envelope.
    #[serde(skip)]
    pub(crate) bitmap_majors: BTreeMap<String, u8>,
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
    pub views: Vec<StoredView>,
    pub items: Vec<DisplayItem>,
    pub omitted: Vec<Omission>,
}
/// A reachable saved view placement. Its matrix positions the cache; rotation
/// may already be baked into pixels, so this is not a model projection matrix.
#[derive(Debug, Serialize)]
pub struct StoredView {
    pub placement_record: usize,
    pub name: String,
    pub placement_transform: Matrix,
    pub cache_bounds: Option<[f64; 6]>,
    pub image_reference: Option<u32>,
    pub source: SourceSpan,
    pub diagnostics: Vec<&'static str>,
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
    Triangles {
        vertices: Vec<[f64; 3]>,
        indices: Vec<u32>,
    },
    Curve {
        center: [f64; 3],
        u: [f64; 3],
        v: [f64; 3],
        start: f64,
        end: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        filled: Option<bool>,
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

type Resolved<'a> = (
    &'a SegmentInventory,
    &'a PayloadObservation,
    Vec<SourceSpan>,
    bool,
);
type ObjectKeys<'a> = BTreeMap<(u32, u16), Option<&'a PayloadObservation>>;

#[derive(Default)]
pub(super) struct ResolveCache<'a> {
    objects: BTreeMap<String, ObjectKeys<'a>>,
    bindings: BTreeMap<(String, u32, String), std::result::Result<Resolved<'a>, String>>,
}

pub(super) fn resolve_cached<'a>(
    doc: &'a DrawingInventory,
    sm: &SegmentInventory,
    raw: u32,
    kind: &str,
    work: &mut usize,
    cache: &mut ResolveCache<'a>,
) -> Result<Resolved<'a>> {
    rse::charge(work, 1)?;
    let key = (sm.registry.id.clone(), raw, kind.to_owned());
    if let Some(result) = cache.bindings.get(&key) {
        if let Ok((_, _, sources, _)) = result {
            rse::charge(work, sources.len())?;
        }
        return result.clone().map_err(Error);
    }
    let result = resolve_inner(doc, sm, raw, kind, work, cache);
    cache.bindings.insert(
        key,
        match &result {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(error.to_string()),
        },
    );
    result
}

#[cfg(test)]
fn resolve<'a>(
    doc: &'a DrawingInventory,
    sm: &SegmentInventory,
    raw: u32,
    kind: &str,
    work: &mut usize,
) -> Result<Resolved<'a>> {
    resolve_cached(doc, sm, raw, kind, work, &mut ResolveCache::default())
}

fn resolve_inner<'a>(
    doc: &'a DrawingInventory,
    sm: &SegmentInventory,
    raw: u32,
    kind: &str,
    work: &mut usize,
    cache: &mut ResolveCache<'a>,
) -> Result<Resolved<'a>> {
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
    let id = guid(&segment[..16]);
    rse::charge(work, doc.segments.len())?;
    let mut targets = doc.segments.iter().filter(|s| s.registry.id == id);
    let target = targets
        .next()
        .ok_or_else(|| error("missing display segment"))?;
    if targets.next().is_some()
        || target.status != "framed"
        || target.registry.major != sm.registry.major
        || super::profile::get(target.registry.major).is_none()
        || target.registry.kind != kind
    {
        return Err(error("ambiguous or unsupported display segment"));
    }
    let mut sources = vec![object_source, namespace_source, segment_source];
    // A namespace is the object's identity revision, not necessarily the segment's
    // latest save. Require the target's exact ID interval and document revision
    // table before accepting an older namespace. Missing/malformed data is never
    // replaced by a GUID-presence scan or a name match.
    let mut identity_verified = same_context;
    if !same_context || doc.revisions.is_some() {
        match super::revisions::binding(doc, target, object, namespace, segment, work) {
            Ok(evidence) => {
                sources.extend(evidence);
                identity_verified = true;
            }
            Err(e) if kind != "AppSegmentType" => return Err(e),
            Err(_) => identity_verified = false, // Existing unqualified layer-cache fallback only.
        }
    }
    if !cache.objects.contains_key(&id) {
        rse::charge(work, target.observations.len())?;
        let mut keys = BTreeMap::new();
        for o in &target.observations {
            if let (Ok(flags), Ok(FieldValue::U16(id))) =
                (word(o, "header_flags"), field(o, "object_id"))
            {
                keys.entry((flags, *id))
                    .and_modify(|v| *v = None)
                    .or_insert(Some(o));
            }
        }
        cache.objects.insert(id.clone(), keys);
    }
    let root = cache.objects[&id]
        .get(&(long(&object[2..6]), short(&object[6..])))
        .ok_or_else(|| error("display object key is not decoded"))?
        .ok_or_else(|| error("ambiguous display object key"))?;
    sources.push(root.source.clone());
    Ok((target, root, sources, identity_verified))
}
fn bind<'a>(
    doc: &'a DrawingInventory,
    sm: &'a SegmentInventory,
    placement: &PayloadObservation,
    work: &mut usize,
    cache: &mut ResolveCache<'a>,
) -> Result<(&'a SegmentInventory, usize, DisplayBinding)> {
    let raw = word(placement, "display_reference")?;
    let (target, root, mut sources) = if matches!(
        placement.proposed_role,
        "sheet_local_display_candidate" | "sheet_local_transformed_display_candidate"
    ) {
        let ordinal = slot(raw)?;
        rse::charge(work, sm.observations.len())?;
        // Local tagged references address record slots in this SM, not object IDs.
        let mut roots = sm
            .observations
            .iter()
            .filter(|o| o.record_ordinal == ordinal);
        let root = roots
            .next()
            .ok_or_else(|| error("missing local display root"))?;
        if roots.next().is_some() {
            return Err(error("ambiguous local display root"));
        }
        (sm, root, vec![root.source.clone()])
    } else {
        let (target, root, sources, _) =
            resolve_cached(doc, sm, raw, "DlSheetDlSegmentType", work, cache)?;
        (target, root, sources)
    };
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

fn fonts(
    doc: &DrawingInventory,
    work: &mut usize,
    diagnostics: &mut Vec<String>,
) -> Result<BTreeMap<u32, DisplayFont>> {
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
                && super::profile::get(s.registry.major).is_some()
        })
        .flat_map(|s| s.observations.iter().map(move |o| (s.registry.major, o)))
        .filter(|(_, o)| o.proposed_role == "font_table_candidate")
        .collect();
    if tables.is_empty() {
        return Ok(BTreeMap::new());
    }
    if tables.len() != 1 {
        return Err(error("ambiguous font directory"));
    }
    let mut result = BTreeMap::new();
    let fields = &tables[0].1.fields;
    let Some((last, entries)) = fields.split_last() else {
        return Err(error("empty font directory"));
    };
    let profile =
        super::profile::get(tables[0].0).ok_or_else(|| error("unsupported font profile"))?;
    let stride = if profile.legacy_fonts { 10 } else { 7 };
    if last.name != "font_next_id" || entries.len() % stride != 0 {
        return Err(error("unsupported font directory fields"));
    }
    let mut identifiers = BTreeSet::new();
    let mut unsupported = 0;
    for row in entries.chunks_exact(stride) {
        rse::charge(work, 1)?;
        if row[7..]
            .iter()
            .any(|f| f.name != "font_tail_flag_unresolved" || !matches!(f.value, FieldValue::U8(_)))
        {
            return Err(error("unsupported font directory flags"));
        }
        let [id, tag, weight, flags, size, name, tail] = &row[..7] else {
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
            FieldValue::U16(tag),
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
        if ids.len() != 1 || size.len() != 2 || !size[0].is_finite() {
            return Err(error("invalid font size or identifier"));
        }
        if !identifiers.insert(ids[0]) {
            return Err(error("duplicate font identifier"));
        }
        if *tag != 4 || size[0] <= 0. {
            unsupported += 1;
            continue;
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
    if unsupported != 0 {
        diagnostics.push(format!(
            "{unsupported} native font entries have unsupported tags or nonpositive sizes"
        ));
    }
    Ok(result)
}

fn geometry_budgeted(
    o: &PayloadObservation,
    m: &Matrix,
    fonts: &BTreeMap<u32, DisplayFont>,
    work: &mut usize,
    budget: &mut DisplayBudget,
) -> Result<Option<DisplayGeometry>> {
    match o.proposed_role {
        "stored_triangles_candidate" => {
            let FieldValue::F32(raw) = field(o, "triangle_vertices")? else {
                return Err(error("invalid saved triangle vertices"));
            };
            let indices = references(o, "triangle_indices")?;
            let count = raw.len() / 3;
            if !matches!(count, 3 | 6)
                || raw.len() != count * 3
                || indices.len() != count
                || indices.iter().any(|&i| i as usize >= count)
            {
                return Err(error("invalid saved triangle topology"));
            }
            budget.item()?;
            budget.points(count)?;
            rse::charge(work, raw.len() + indices.len())?;
            let vertices = raw
                .chunks_exact(3)
                .map(|p| transform(m, &[p[0] as f64, p[1] as f64, p[2] as f64], 1.))
                .collect::<Result<Vec<_>>>()?;
            for t in indices.chunks_exact(3) {
                let [a, b, c] = [
                    vertices[t[0] as usize],
                    vertices[t[1] as usize],
                    vertices[t[2] as usize],
                ];
                let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if !area.is_finite() || area == 0. || a[2] != b[2] || b[2] != c[2] {
                    return Err(error("degenerate or nonplanar saved triangle"));
                }
            }
            Ok(Some(DisplayGeometry::Triangles {
                vertices,
                indices: indices.to_vec(),
            }))
        }
        "stored_bspline_candidate" => {
            let spline = super::spline::Spline::observation(o)?;
            // Fixed parameter sampling is experimental, not a geometric error
            // bound. Each nonempty trimmed knot span has 16 line segments.
            const STEPS: usize = 16;
            let count = spline.spans().count() * STEPS + 1;
            budget.item()?;
            budget.points(count)?;
            // Charge inspection of the stored curve once. Derived samples use
            // the shared point budget; degree <= 4 independently bounds their
            // evaluation, without consuming the reference traversal allowance.
            rse::charge(work, spline.stored_values())?;
            let mut points = Vec::with_capacity(count);
            for (span, start, end) in spline.spans() {
                let first = usize::from(!points.is_empty());
                for step in first..=STEPS {
                    let t = if step == STEPS {
                        end
                    } else {
                        start + (end - start) * (step as f64 / STEPS as f64)
                    };
                    points.push(transform(m, &spline.evaluate(span, t)?, 1.)?);
                }
            }
            Ok(Some(DisplayGeometry::Polyline { points }))
        }
        "stored_polyline_candidate" | "stored_line_candidate" => {
            budget.item()?;
            let count = if o.proposed_role == "stored_line_candidate" {
                2
            } else {
                match field(o, "xyz_points_candidate")? {
                    FieldValue::F32(v) if v.len() % 3 == 0 => v.len() / 3,
                    _ => return Err(error("invalid display polyline")),
                }
            };
            budget.points(count)?;
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
            budget.item()?;
            budget.text(text.len())?;
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
                budget.text(f.family.len())?;
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
        "stored_circle_candidate" | "stored_arc_candidate" | "stored_ellipse_candidate" => {
            budget.item()?;
            let is_arc = o.proposed_role == "stored_arc_candidate";
            let is_ellipse = o.proposed_role == "stored_ellipse_candidate";
            rse::charge(work, if is_ellipse { 13 } else { 12 })?;
            let v = if is_ellipse {
                doubles(o, "ellipse_center_radii_axes_angles", 13)?
            } else if is_arc {
                doubles(o, "arc_center_normal_axis_radius_angles", 12)?
            } else {
                doubles(o, "circle_center_normal_radius", 7)?
            };
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
            let cross = |a: &[f64], b: &[f64]| {
                [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ]
            };
            // This ellipse record stores two axes, unlike the circle/arc
            // record's normal and axis. Do not reuse that wire interpretation.
            let normal = if is_ellipse {
                cross(&v[5..8], &v[8..11])
            } else {
                <[f64; 3]>::try_from(&v[3..6]).unwrap()
            };
            let n: &[f64] = &normal;
            if (dot(n, n) - 1.).abs() > 1e-9 {
                return Err(error("non-unit stored curve normal"));
            }
            let axis = if is_ellipse {
                <[f64; 3]>::try_from(&v[5..8]).unwrap()
            } else if is_arc {
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
            let minor_axis = if is_ellipse {
                let b = <[f64; 3]>::try_from(&v[8..11]).unwrap();
                if (dot(&b, &b) - 1.).abs() > 1e-9 || dot(&axis, &b).abs() > 1e-9 {
                    return Err(error("invalid stored ellipse basis"));
                }
                b
            } else {
                cross(n, &axis)
            };
            let radius = v[if is_ellipse {
                3
            } else if is_arc {
                9
            } else {
                6
            }];
            let minor_radius = if is_ellipse { v[4] } else { radius };
            let (start, end) = if is_ellipse {
                (v[11], v[12])
            } else if is_arc {
                (v[10], v[11])
            } else {
                (0., std::f64::consts::TAU)
            };
            if radius <= 0.
                || minor_radius <= 0.
                || end <= start
                || end - start > std::f64::consts::TAU + 1e-9
            {
                return Err(error("unsupported stored curve range"));
            }
            let center = transform(m, &v[..3], 1.)?;
            let u = transform(m, &axis.map(|x| x * radius), 0.)?;
            let v = transform(m, &minor_axis.map(|x| x * minor_radius), 0.)?;
            let filled = if o.fields.iter().any(|f| f.name == "filled_conic_candidate") {
                if !matches!(field(o, "filled_conic_candidate")?, FieldValue::U8(1)) {
                    return Err(error("unsupported stored conic fill"));
                }
                Some(true)
            } else {
                None
            };
            Ok(Some(DisplayGeometry::Curve {
                center,
                u,
                v,
                start,
                end,
                filled,
            }))
        }
        _ => Ok(None),
    }
}

// Bidirectional, unique ownership is required before a branch is interpreted.
struct DisplayNodes<'a> {
    nodes: BTreeMap<usize, &'a PayloadObservation>,
    ordinals: BTreeSet<usize>,
    external_children: BTreeSet<usize>,
}
fn owners_with_depth<'a>(
    s: &'a SegmentInventory,
    work: &mut usize,
    max_depth: usize,
) -> Result<DisplayNodes<'a>> {
    let map = by_ordinal(s, work)?;
    rse::charge(work, s.records.len())?;
    let ordinals: BTreeSet<_> = s.records.iter().map(|r| r.ordinal).collect();
    let mut incoming = BTreeMap::new();
    let mut external_children = BTreeSet::new();
    for (&parent, o) in &map {
        if o.proposed_role != "display_group_candidate" {
            continue;
        }
        for &raw in references(o, "child_references_unresolved")? {
            rse::charge(work, 1)?;
            if raw == 0 {
                continue;
            }
            if super::profile::get(s.registry.major).is_some_and(|p| p.external_children)
                && raw & 0x80000000 == 0
            {
                // Observed cross-segment display child (including DC targets).
                // Preserve the raw reference and omit this branch; never treat
                // a foreign object key as a local slot or poison other roots.
                external_children.insert(parent);
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
            if !seen.insert(at) || seen.len() > max_depth {
                return Err(error("cyclic/deep display ownership"));
            }
            at = *parent;
        }
    }
    Ok(DisplayNodes {
        nodes: map,
        ordinals,
        external_children,
    })
}

#[allow(clippy::too_many_arguments)] // Container work and expanded display budgets are independent.
fn display_branch<'a>(
    space: &mut DisplaySpace,
    doc: &'a DrawingInventory,
    target: &SegmentInventory,
    binding: &DisplayBinding,
    parent: Matrix,
    fonts: &BTreeMap<u32, DisplayFont>,
    work: &mut usize,
    budget: &mut DisplayBudget,
    display: &DisplayNodes,
    cache: &mut ResolveCache<'a>,
) -> Result<()> {
    let root = binding.target_record;
    let placement = binding.placement_record;
    let nodes = &display.nodes;
    let mut stack = vec![(root, parent, Vec::new(), DisplayStyle::default())];
    let mut visited = BTreeSet::new();
    while let Some((id, mut m, mut path, inherited)) = stack.pop() {
        rse::charge(work, 1)?;
        if !visited.insert(id) || path.len() > budget.limits.max_nesting_depth {
            return Err(error("repeated/deep display branch"));
        }
        if display.external_children.contains(&id) {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: "cross_segment_display_children_not_supported",
            });
            continue;
        }
        let Some(o) = nodes.get(&id) else {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: "display_type_not_decoded",
            });
            continue;
        };
        let style = super::appearance::apply(
            doc,
            target,
            o,
            nodes,
            &display.ordinals,
            &inherited,
            work,
            cache,
        )?;
        let unknown_color = style
            .unresolved
            .contains(&"display_color_mask_not_interpreted");
        if !style.visible || unknown_color {
            space.omitted.push(Omission {
                segment_id: target.registry.id.clone(),
                record_ordinal: id,
                reason: if unknown_color {
                    "display_color_mask_not_interpreted"
                } else {
                    "hidden_by_stored_attribute"
                },
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
        } else if let Some(geometry) = geometry_budgeted(o, &m, fonts, work, budget)? {
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
                reason: if o.proposed_role == "stored_point_marker_candidate" {
                    "point_marker_visibility_unverified"
                } else {
                    "unsupported_display_role"
                },
            });
        }
    }
    Ok(())
}

/// Experimental major23/31 scene. Stored display coordinates/transforms only;
/// no external model loading, no active-state claim and no fitted placement.
pub fn experimental_scene(doc: &DrawingInventory, limits: &Limits) -> ExperimentalScene {
    experimental_scene_with_limits(doc, limits, &DrawingLimits::default())
}

pub fn experimental_scene_with_limits(
    doc: &DrawingInventory,
    limits: &Limits,
    drawing: &DrawingLimits,
) -> ExperimentalScene {
    let mut out = ExperimentalScene {
        bitmap_majors: BTreeMap::new(),
        status: "unavailable",
        qualified: false,
        source_sha256: doc.source_sha256.clone(),
        units: "source_units_unverified",
        spaces: vec![],
        diagnostics: vec![],
    };
    if let Err(e) = limits.validate().and_then(|_| drawing.validate()) {
        out.diagnostics.push(e.to_string());
        return out;
    }
    let mut budget = DisplayBudget::new(drawing);
    // Synthetic resource IDs occupy a separate namespace from RefdFile images.
    // A collision is rejected by the asset reader rather than aliasing sources.
    let mut bitmap_ids = BTreeMap::new();
    // Validate each immutable display segment once per document. Rebuilding its
    // owner graph for every annotation made large sheets exhaust the work cap.
    let mut display_nodes = BTreeMap::new();
    let mut resolve_cache = ResolveCache::default();
    let mut work = drawing.max_reference_visits;
    let fonts = match fonts(doc, &mut work, &mut out.diagnostics) {
        Ok(f) => f,
        Err(e) => {
            out.diagnostics.push(e.to_string());
            BTreeMap::new()
        }
    };
    for sm in doc.segments.iter().filter(|s| {
        s.status == "framed"
            && super::profile::get(s.registry.major).is_some()
            && s.registry.kind == "DlSheetSmSegmentType"
    }) {
        if out.spaces.len() >= drawing.max_sheets {
            out.spaces.clear();
            out.diagnostics.push("drawing sheet limit exceeded".into());
            return out;
        }
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
                views: vec![],
                items: vec![],
                omitted: vec![],
            };
            let mut stack = vec![(root.record_ordinal, IDENTITY, 0usize)];
            let mut visited = BTreeSet::new();
            let mut bound = BTreeSet::new();
            while let Some((id, parent, depth)) = stack.pop() {
                rse::charge(&mut work, 1)?;
                if depth > drawing.max_nesting_depth || !visited.insert(id) {
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
                    budget.item()?;
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
                    "sheet_space_candidate"
                        | "sheet_placement_candidate"
                        | "sheet_local_display_candidate"
                        | "sheet_external_display_candidate"
                        | "sheet_local_transformed_display_candidate"
                ) {
                    return Err(error("unsupported sheet placement role"));
                }
                let world = if matches!(
                    node.proposed_role,
                    "sheet_local_display_candidate" | "sheet_external_display_candidate"
                ) {
                    parent // These bounded annotation layouts store paper-space positions.
                } else {
                    compose(&parent, &matrix(node)?)?
                };
                let (target, root, binding) = bind(doc, sm, node, &mut work, &mut resolve_cache)?;
                if !bound.insert((target.registry.id.clone(), root)) {
                    return Err(error(
                        "repeated display-root instance needs explicit semantics",
                    ));
                }
                if !display_nodes.contains_key(&target.registry.id) {
                    display_nodes.insert(
                        target.registry.id.clone(),
                        owners_with_depth(target, &mut work, drawing.max_nesting_depth)?,
                    );
                }
                let display_start = space.items.len();
                display_branch(
                    &mut space,
                    doc,
                    target,
                    &binding,
                    world,
                    &fonts,
                    &mut work,
                    &mut budget,
                    &display_nodes[&target.registry.id],
                    &mut resolve_cache,
                )?;
                space.bindings.push(binding);
                if node
                    .fields
                    .iter()
                    .any(|f| f.name == "view_bitmap_reference")
                {
                    budget.view()?;
                    let FieldValue::Utf16(name) = field(node, "view_name_unresolved")? else {
                        return Err(error("invalid saved view name"));
                    };
                    budget.text(name.len())?;
                    let mut view = StoredView {
                        placement_record: node.record_ordinal,
                        name: name.clone(),
                        placement_transform: world,
                        cache_bounds: None,
                        image_reference: None,
                        source: node.source.clone(),
                        diagnostics: vec!["view_type_parent_rotation_and_clip_unverified"],
                    };
                    let raw = word(node, "view_bitmap_reference")?;
                    if raw != 0 {
                        let id = slot(raw)?;
                        let bitmap = nodes
                            .get(&id)
                            .ok_or_else(|| error("view bitmap not decoded"))?;
                        if bitmap.proposed_role != "stored_view_bitmap_candidate" {
                            return Err(error("invalid view bitmap target"));
                        }
                        let b = doubles(node, "view_bitmap_bounds", 6)?;
                        if b[3] <= b[0] || b[4] <= b[1] {
                            return Err(error("invalid view bitmap bounds"));
                        }
                        view.cache_bounds = Some(b.try_into().unwrap());
                        let next = 0x80000000u32
                            .checked_add(bitmap_ids.len() as u32)
                            .ok_or_else(|| error("view bitmap identifier overflow"))?;
                        let reference = *bitmap_ids
                            .entry((sm.registry.id.clone(), id))
                            .or_insert(next);
                        if let Some(old) = out
                            .bitmap_majors
                            .insert(bitmap.source.stream.clone(), sm.registry.major)
                        {
                            if old != sm.registry.major {
                                return Err(error("conflicting view bitmap profiles"));
                            }
                        }
                        view.image_reference = Some(reference);
                        budget.item()?;
                        space.items.push(DisplayItem {
                            segment_id: sm.registry.id.clone(),
                            record_ordinal: id,
                            source: bitmap.source.clone(),
                            placement_record: node.record_ordinal,
                            group_path: vec![],
                            transform: world,
                            style: DisplayStyle::default(),
                            geometry: DisplayGeometry::Image {
                                reference,
                                format: 3,
                                origin: transform(&world, &[b[0], b[4], 0.], 1.)?,
                                u: transform(&world, &[b[3] - b[0], 0., 0.], 0.)?,
                                v: transform(&world, &[0., b[1] - b[4], 0.], 0.)?,
                            },
                        });
                        // The major23/28 shaded cache is the background of the
                        // view's saved vector edges. Keep annotations above it.
                        // General drawing z-order remains unqualified.
                        if super::profile::get(sm.registry.major)
                            .is_some_and(|p| p.bitmap == Some(super::profile::BitmapLayout::Rgba))
                        {
                            let cache = space.items.pop().unwrap();
                            space.items.insert(display_start, cache);
                        }
                    }
                    if raw == 0 {
                        view.diagnostics.push("view_cache_absent");
                    }
                    space.views.push(view);
                }
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
        if budget.exhausted {
            out.spaces.clear();
            if let Err(e) = result {
                out.diagnostics.push(e.to_string());
            }
            return out;
        }
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
