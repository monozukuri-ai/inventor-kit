use super::fields::Fields;
use crate::{Error, Result};

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
    // Known record shape only; the following references/state remain opaque.
    f.r.skip(134)?;
    f.r.finish()
}

pub(super) fn links(f: &mut Fields<'_, '_>) -> Result<()> {
    f.r.skip(34)?;
    f.references("references_unresolved")?;
    f.r.skip(11)?;
    for name in ["dc_segment_name", "dl_segment_name", "sm_segment_name"] {
        f.text(name)?;
    }
    // No name-based owner resolution. The remaining state/units are not decoded.
    f.r.skip(180)?;
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
