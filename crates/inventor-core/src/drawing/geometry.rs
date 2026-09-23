use super::{fields::Fields, FieldValue};
use crate::{rse, Error, Result};

// Observed indexed triangle batches used by saved dimension arrowheads. Their
// vertex type is resolved through the owning Meta table; index flags are kept
// as wire evidence, not interpreted as styles.
pub(super) fn triangles(f: &mut Fields<'_, '_>) -> Result<()> {
    let index_flags = f
        .profile
        .triangle_flags()
        .ok_or_else(|| Error("unsupported saved triangle profile".into()))?;
    f.display_header()?;
    f.require(0x30000002)?;
    let count = f.r.count(6)?;
    if !matches!(count, 3 | 6) || f.r.u32()? < count as u32 {
        return Err(Error("unsupported saved triangle vertex count".into()));
    }
    let start = f.r.pos;
    let vertex_flags = f.point_type()?;
    f.add(
        "triangle_point_type_selector",
        start,
        FieldValue::U32(vec![vertex_flags]),
    );
    f.floats("triangle_vertices", count * 3)?;
    f.require(0x30000002)?;
    f.require(count as u32)?;
    if f.r.u32()? < count as u32 {
        return Err(Error("saved triangle index capacity below count".into()));
    }
    let start = f.r.pos;
    f.require(index_flags)?;
    f.add(
        "triangle_index_flags_unresolved",
        start,
        FieldValue::U32(vec![index_flags]),
    );
    rse::charge(f.work, count)?;
    let start = f.r.pos;
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        let index = f.r.u32()?;
        if index >= count as u32 {
            return Err(Error("saved triangle index out of range".into()));
        }
        indices.push(index);
    }
    f.add("triangle_indices", start, FieldValue::U32(indices));
    for expected in [0x30000002, 0, 0x30000002, 0, 0x105, 0x30000002, 0, 0, 0] {
        f.require(expected)?;
    }
    f.r.finish()
}

pub(super) fn points(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.require(0x30000002)?;
    let n = f.r.count(65536)?;
    if n == 0 || f.r.u32()? < n as u32 {
        return Err(Error("unsupported drawing point list".into()));
    }
    f.point_type()?;
    rse::charge(f.work, n * 3)?;
    let start = f.r.pos;
    let mut values = Vec::with_capacity(n * 3);
    for _ in 0..n * 3 {
        let v = f32::from_le_bytes(f.r.take(4)?.try_into().unwrap());
        if !v.is_finite() {
            return Err(Error("non-finite drawing point field".into()));
        }
        values.push(v);
    }
    f.add("xyz_points_candidate", start, FieldValue::F32(values));
    if f.r.u8()? != 0 {
        return Err(Error("unknown drawing point list suffix".into()));
    }
    f.r.finish()
}

pub(super) fn group(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.references("child_references_unresolved")?;
    let start = f.r.pos;
    let branch = f.r.u8()?;
    f.add("transform_branch", start, FieldValue::U8(branch));
    match branch {
        0 => (), // Preserve absence; do not substitute an identity transform.
        1 => f.compact()?,
        _ => return Err(Error("unknown drawing group transform branch".into())),
    }
    f.r.finish()
}

pub(super) fn line(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.doubles("line_endpoints", 6)?;
    f.r.finish()
}
pub(super) fn circle(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.doubles("circle_center_normal_radius", 7)?;
    if f.r.u8()? != 0 {
        return Err(Error("unknown circle suffix".into()));
    }
    f.r.finish()
}
pub(super) fn arc(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.doubles("arc_center_normal_axis_radius_angles", 12)?;
    if f.r.u8()? != 0 {
        return Err(Error("unknown arc suffix".into()));
    }
    f.r.finish()
}

pub(super) fn ellipse(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.doubles("ellipse_center_radii_axes_angles", 13)?;
    if f.r.u8()? != 0 {
        return Err(Error("unknown ellipse suffix".into()));
    }
    f.r.finish()
}
