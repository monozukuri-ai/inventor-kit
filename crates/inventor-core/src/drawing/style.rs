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
