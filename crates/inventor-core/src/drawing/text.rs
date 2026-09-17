use super::{fields::Fields, FieldValue};
use crate::Result;

pub(super) fn fields(f: &mut Fields<'_, '_>) -> Result<()> {
    f.display_header()?;
    f.text("text")?;
    f.doubles("position_and_direction_candidate", 6)?;
    let start = f.r.pos;
    let value = f.r.u16()?;
    f.add("raw_text_flags", start, FieldValue::U16(value));
    let start = f.r.pos;
    let value = f.r.u32()?;
    f.add("style_index_candidate", start, FieldValue::U32(vec![value]));
    if f.r.u8()? != 0 {
        return Err(crate::Error("unknown drawing text suffix".into()));
    }
    f.r.finish()
}
