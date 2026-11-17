use super::{fields::Fields, FieldValue};
use crate::{rse, Error, Result};

pub(super) fn fonts(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(6)?;
    f.layer()?;
    f.require(0x30000002)?;
    let n = f.r.count(4096)?;
    rse::charge(f.work, n)?;
    if n == 0 || f.r.u32()? < n as u32 {
        return Err(Error("unsupported drawing font table".into()));
    }
    f.require(0x102)?;
    for _ in 0..n {
        // Meta6/7 store each entry, and the entry after its identifier, as
        // nested sections.
        let entry = f.open();
        f.word("font_id")?;
        let font = f.open();
        f.short("font_tag")?;
        f.short("font_weight")?;
        f.short("font_flags")?;
        f.floats("font_size_parameters", 2)?;
        f.text("font_name")?;
        let (floats, flags) = f.profile.font_tail();
        f.floats("font_tail_parameters", floats)?;
        for _ in 0..flags {
            f.byte("font_tail_flag_unresolved")?;
        }
        f.close(font)?;
        f.close(entry)?;
    }
    f.layer()?;
    f.word("font_next_id")?;
    f.r.finish()
}

pub(super) fn attributes(f: &mut Fields<'_, '_>) -> Result<()> {
    // Meta6/7 start every attribute record with an empty base layer.
    f.layer()?;
    f.require(0x30000002)?;
    let n = f.r.count(65536)?;
    rse::charge(f.work, n)?;
    // Empty lists were first observed on major23/26/28/29 and later on
    // production major31 drawings (Inventor 2025/2026 saves); finish() still
    // rejects any trailing bytes, so admitting them for every profile is safe.
    if n == 0 {
        return f.r.finish();
    }
    if n == 0 || f.r.u32()? < n as u32 {
        return Err(Error("invalid display attribute list".into()));
    }
    f.require(0x10000000)?;
    for _ in 0..n {
        f.word("attribute_entry")?;
    }
    f.r.finish()
}
pub(super) fn boolean(f: &mut Fields<'_, '_>) -> Result<()> {
    f.layer()?;
    f.word("attribute_mask")?;
    f.layer()?;
    f.byte("attribute_boolean")?;
    f.r.finish()
}

pub(super) fn layer_binding(f: &mut Fields<'_, '_>) -> Result<()> {
    f.layer()?;
    f.word("layer_binding_mask")?;
    f.layer()?;
    f.word("layer_reference")?;
    f.word("color_override")?;
    f.floats("stroke_scale", 1)?;
    if f.profile.layer_binding_flags() {
        f.byte("layer_binding_flags")?;
    }
    f.r.finish()
}
pub(super) fn layer(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("header_flags")?;
    f.short("object_id")?;
    f.layer()?;
    f.r.skip(f.profile.layer_prefix)?;
    f.text("layer_name")?;
    f.r.skip(6)?;
    f.text("layer_origin")?;
    f.layer()?;
    f.doubles("layer_width", 1)?;
    f.word("line_pattern")?;
    let rgba = f.open();
    f.floats("layer_rgba", 4)?;
    f.close(rgba)?;
    f.byte("layer_color_mode")?;
    f.doubles("layer_scale", 1)?;
    f.byte("layer_flag_a")?;
    if f.profile.layer_flags() == 3 {
        f.byte("layer_flag_b")?;
        f.byte("layer_flag_c")?;
    }
    f.r.finish()
}
pub(super) fn stroke(f: &mut Fields<'_, '_>) -> Result<()> {
    f.layer()?;
    f.word("stroke_mask")?;
    f.layer()?;
    // Meta6/7 store the stroke from its flags to its parameters as a section.
    let section = f.open();
    f.short("stroke_flags")?;
    f.floats("stroke_width", 1)?;
    f.short("stroke_mode")?;
    f.short("stroke_pattern")?;
    f.byte("stroke_byte")?;
    f.short("stroke_list_type")?;
    let n = f.r.u16()? as usize;
    if n > 256 {
        return Err(Error("stroke pattern count limit".into()));
    }
    f.doubles("stroke_dashes", n)?;
    f.floats("stroke_parameters", 3)?;
    f.close(section)?;
    f.word("stroke_pattern_copy")?;
    f.word("stroke_kind")?;
    f.r.finish()
}
pub(super) fn color(f: &mut Fields<'_, '_>) -> Result<()> {
    f.layer()?;
    f.word("color_mask")?;
    f.layer()?;
    if f.profile.layered {
        // Meta6/7 nest the same 21 values: one RGBA section, then a section
        // holding four RGBA sections and the last value.
        layered_color(f)?;
    } else {
        f.floats("color_rgba_parameters", 21)?;
    }
    f.byte("color_flags")?;
    f.r.finish()
}

fn layered_rgba(f: &mut Fields<'_, '_>, values: &mut Vec<f32>) -> Result<()> {
    let section = f.open();
    for chunk in f.r.take(16)?.chunks_exact(4) {
        values.push(f32::from_le_bytes(chunk.try_into().unwrap()));
    }
    f.close(section)
}

fn layered_color(f: &mut Fields<'_, '_>) -> Result<()> {
    rse::charge(f.work, 21)?;
    let start = f.r.pos;
    let mut values = Vec::with_capacity(21);
    layered_rgba(f, &mut values)?;
    let rest = f.open();
    for _ in 0..4 {
        layered_rgba(f, &mut values)?;
    }
    values.push(f32::from_le_bytes(f.r.take(4)?.try_into().unwrap()));
    f.close(rest)?;
    if values.iter().any(|v| !v.is_finite()) {
        return Err(Error("non-finite drawing field".into()));
    }
    f.add("color_rgba_parameters", start, FieldValue::F32(values));
    Ok(())
}
