use super::fields::Fields;
use crate::{rse, Error, Result};

pub(super) fn fonts(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(6)?;
    f.require(0x30000002)?;
    let n = f.r.count(4096)?;
    rse::charge(f.work, n)?;
    if n == 0 || f.r.u32()? < n as u32 {
        return Err(Error("unsupported drawing font table".into()));
    }
    f.require(0x102)?;
    for _ in 0..n {
        f.word("font_id")?;
        f.short("font_tag")?;
        f.short("font_weight")?;
        f.short("font_flags")?;
        f.floats("font_size_parameters", 2)?;
        f.text("font_name")?;
        f.floats("font_tail_parameters", 3)?;
    }
    f.word("font_next_id")?;
    f.r.finish()
}

pub(super) fn attributes(f: &mut Fields<'_, '_>) -> Result<()> {
    f.require(0x30000002)?;
    let n = f.r.count(65536)?;
    rse::charge(f.work, n)?;
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
    f.word("attribute_mask")?;
    f.byte("attribute_boolean")?;
    f.r.finish()
}

pub(super) fn layer_binding(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("layer_binding_mask")?;
    f.word("layer_reference")?;
    f.word("color_override")?;
    f.floats("stroke_scale", 1)?;
    f.byte("layer_binding_flags")?;
    f.r.finish()
}
pub(super) fn layer(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("header_flags")?;
    f.short("object_id")?;
    f.r.skip(21)?;
    f.text("layer_name")?;
    f.r.skip(6)?;
    f.text("layer_origin")?;
    f.doubles("layer_width", 1)?;
    f.word("line_pattern")?;
    f.floats("layer_rgba", 4)?;
    f.byte("layer_color_mode")?;
    f.doubles("layer_scale", 1)?;
    f.byte("layer_flag_a")?;
    f.byte("layer_flag_b")?;
    f.byte("layer_flag_c")?;
    f.r.finish()
}
pub(super) fn stroke(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("stroke_mask")?;
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
    f.word("stroke_pattern_copy")?;
    f.word("stroke_kind")?;
    f.r.finish()
}
pub(super) fn color(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("color_mask")?;
    f.floats("color_rgba_parameters", 21)?;
    f.byte("color_flags")?;
    f.r.finish()
}
