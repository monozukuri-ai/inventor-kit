use super::document::Graph;
use super::kernel_labels::History;
use super::*;

pub(super) fn record(g: &Graph<'_>, r: &Value) -> Result<Vec<u8>> {
    let owner = string(r, "owner")?;
    let role = string(r, "role")?;
    let kind = string(r, "generation_contract")?;
    let a = g.get(owner, role)?;
    let mut w = Wire::default();
    let q = |role| g.reference(owner, role, "local_qualified");
    if matches!(kind, "source-carrier" | "result-carrier") {
        let result = kind == "result-carrier";
        w.u32(0);
        w.u16(a.key.ok_or_else(|| Error("carrier key required".into()))?);
        w.words(&[if result { 0x31d } else { 0x357 }, 44]);
        let sab = if let Some(kernels) = &g.kernels {
            kernels[usize::from(result)].clone()
        } else {
            super::kernel_labels::encode(
                &g.shape,
                if result {
                    History::BaseFeature
                } else {
                    History::Source
                },
            )?
        };
        w.0.extend(sab);
        w.u32(q(role)?);
        w.u8(1);
        w.u32(if result {
            BASE_FEATURE_STATE
        } else {
            SOURCE_STATE
        });
        w.u8(0);
        w.u32(if result { q("history_helper")? } else { 0 });
        w.u32(u32::MAX);
        return Ok(w.0);
    }
    if kind != "brep-shell" {
        return Err(Error("unknown carrier/shell constructor".into()));
    }
    if let Some(key) = a.key {
        w.u32(0);
        w.u16(key);
    }
    match role {
        "geometry_list" => w.words(&[0x30000008, 1, 16, q("geometry")?]),
        "empty_list" => w.words(&[0x30000008, 0]),
        "feature_map" => w.words(&[
            0x30000006,
            1,
            8,
            14,
            13,
            g.reference(owner, "feature_token", "local_bare")?,
            0,
            0,
        ]),
        "feature_token" => {
            w.u32(13);
            w.u16(269);
            w.words(&[0x30000006, 0]);
        }
        "geometry" => {
            w.u32(2);
            w.u8(0);
            w.words(&[q("carrier")?, 0, 44]);
        }
        "history_helper" => w.words(&[
            0,
            q("carrier")?,
            0x30000002,
            2,
            2,
            4,
            SOURCE_STATE,
            BASE_FEATURE_STATE,
            0x30000006,
            2,
            4,
            14,
            SOURCE_STATE,
            g.reference(owner, "previous_child", "local_bare")?,
            BASE_FEATURE_STATE,
            g.reference(owner, "current_child", "local_bare")?,
            g.reference(owner, "terminal_child", "local_bare")?,
        ]),
        "previous_child" | "current_child" | "terminal_child" => w.words(&[
            q("history_helper")?,
            match role {
                "previous_child" => SOURCE_STATE,
                "current_child" => BASE_FEATURE_STATE,
                _ => u32::MAX,
            },
            0x30000002,
            0,
        ]),
        "binding" => w.words(&[0, q("unit")?, 0, 0, 0, 0]),
        "unit" => w.words(&[0, 0, 0]),
        _ => return Err(Error("unknown BRep shell role".into())),
    }
    Ok(w.0)
}
