use super::fields::Fields;
use crate::{Error, Result};

// Observed on annotation endpoints and table corners. Keep its position and
// flags inspectable, but do not infer a printed point or a nonprinting grip.
pub(super) fn point_marker(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("header_flags")?;
    f.short("object_id")?;
    f.word("marker_flags_unresolved")?;
    f.word("attribute_reference")?;
    f.r.skip(4)?;
    f.word("owner_reference")?;
    f.r.skip(4)?;
    f.doubles("marker_position", 3)?;
    f.floats("marker_parameters_unresolved", 3)?;
    f.short("marker_suffix_unresolved")?;
    f.r.finish()
}

pub(super) fn document(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(34)?;
    f.text("document_name")?;
    f.references("preceding_references_unresolved")?;
    f.r.skip(36)?;
    f.references("sheet_references_candidate")?;
    // Deliberately a prefix observation: the document's remaining collections
    // and active-state selection are still covered by the opaque payload span.
    Ok(())
}

pub(super) fn name(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(34)?;
    f.text("name")?;
    // Prefix observation, like the document name. The following collections
    // vary with sheet contents and remain covered by the opaque payload span.
    Ok(())
}

pub(super) fn links(f: &mut Fields<'_, '_>) -> Result<()> {
    // Full object key used by the SM definition reference, not a record ordinal.
    f.word("header_flags")?;
    f.short("object_id")?;
    f.r.skip(28)?;
    f.references("references_unresolved")?;
    f.r.skip(11)?;
    for name in ["dc_segment_name", "dl_segment_name", "sm_segment_name"] {
        f.text(name)?;
    }
    // No name-based owner resolution. The remaining state/units are not decoded.
    f.r.skip(28)?;
    f.references("sheet_table_references_unresolved")?;
    f.r.skip(8)?;
    f.references("sheet_auxiliary_references_unresolved")?;
    // This separate collection has a different tag; only the empty layout is
    // observed. Reject populated variants until their element grammar is known.
    f.require(0x30000006)?;
    f.require(0)?;
    f.r.skip(8)?;
    f.references("sheet_content_references_unresolved")?;
    f.r.skip(92)?;
    f.text("name")?;
    f.r.skip(8)?;
    f.r.finish()
}

pub(super) fn space(f: &mut Fields<'_, '_>) -> Result<()> {
    placement(f)?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown drawing space branch".into()));
    }
    f.require(2)?;
    f.doubles("origin_and_extent_candidate", 4)?;
    f.r.finish()
}

pub(super) fn placement(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(15)?;
    f.references("references_unresolved")?;
    f.word("display_reference")?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown drawing placement branch".into()));
    }
    f.word("definition_reference")?;
    f.compact()
}

pub(super) fn sketch_placement(f: &mut Fields<'_, '_>) -> Result<()> {
    placement(f)?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown sheet sketch suffix".into()));
    }
    f.word("sketch_suffix_reference_unresolved")?;
    f.r.finish()
}

fn local_display_prefix(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(15)?;
    f.references("references_unresolved")?;
    f.word("display_reference")?;
    f.word("definition_reference")?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown local annotation display branch".into()));
    }
    Ok(())
}

pub(super) fn local_display(f: &mut Fields<'_, '_>) -> Result<()> {
    local_display_prefix(f)?;
    f.r.finish()
}

pub(super) fn leader_display(f: &mut Fields<'_, '_>) -> Result<()> {
    local_display_prefix(f)?;
    f.word("leader_suffix_reference_unresolved")?;
    f.r.finish()
}

pub(super) fn table_display(f: &mut Fields<'_, '_>) -> Result<()> {
    placement(f)?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown table display suffix".into()));
    }
    f.word("table_suffix_reference_unresolved")?;
    f.r.finish()
}

pub(super) fn image(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(15)?;
    f.doubles("image_height_width", 2)?;
    f.word("image_handle")?;
    f.compact()?;
    f.word("image_color")?;
    f.byte("image_format")?;
    f.word("image_reference")?;
    f.r.finish()
}

// The observed view cache has a separate local bitmap reference. The display
// group referenced by the common prefix also contains interaction graphics;
// its rectangles must not be mistaken for the projected model edges.
pub(super) fn view_placement(f: &mut Fields<'_, '_>) -> Result<()> {
    placement(f)?;
    if f.r.u8()? != 1 {
        return Err(Error("unknown view placement suffix".into()));
    }
    f.word("view_definition_copy_unresolved")?;
    f.word("view_flags_unresolved")?;
    f.references("view_references_unresolved")?;
    f.references("view_auxiliary_references_unresolved")?;
    f.references("view_other_references_unresolved")?;
    f.text("view_name_unresolved")?;
    f.word("view_bitmap_reference")?;
    f.word("view_style_unresolved")?;
    f.doubles("view_bitmap_bounds", 6)?;
    f.r.skip(16)?; // Uninterpreted suffix GUID, retained in the payload source.
    f.r.finish()
}

pub(super) fn view_bitmap(f: &mut Fields<'_, '_>) -> Result<()> {
    f.word("header_flags")?;
    f.short("object_id")?;
    let start = f.r.pos;
    let height = f.r.u32()?;
    let width = f.r.u32()?;
    f.add(
        "bitmap_height_width",
        start,
        super::FieldValue::U32(vec![height, width]),
    );
    f.require(0)?;
    if f.r.u8()? != 0 {
        return Err(Error("unknown view bitmap layout".into()));
    }
    let pixels = (width as u64) * (height as u64);
    if width == 0 || height == 0 || pixels > 16_777_216 {
        return Err(Error("view bitmap dimensions exceed profile".into()));
    }
    crate::rse::charge(f.work, (pixels as usize).div_ceil(1024))?;
    // Pixels are read only for a reachable placement and under image budgets.
    f.r.skip(pixels as usize * 4)?;
    f.r.finish()
}
