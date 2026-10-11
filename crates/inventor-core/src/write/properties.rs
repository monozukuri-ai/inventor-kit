use super::document::Graph;
use super::*;
use std::collections::BTreeMap;
const CREATION: [u16; 4] = [10, 16415, 1, 27000];
const SAVE: [u16; 4] = [0, 16415, 0, 15500];

fn versions(w: &mut Wire, a: &[u16]) {
    for &v in a {
        w.u16(v);
    }
}
fn revision_info(g: &Graph<'_>) -> Result<Vec<u8>> {
    let mut w = Wire::default();
    w.words(&[3, 565]);
    for row in array(&g.profile.0, "protocols")? {
        let id = guid(string(row, "guid")?)?;
        if id == g.birth {
            return Err(Error("birth collides with protocol".into()));
        }
        w.id(&id);
        w.u32(number(&row["flags"])?);
        w.u16(number(&row["kind"])? as u16);
        let v = &row["versions"];
        match string(v, "mode")? {
            "none" => {
                if row["kind"] == 65535 {
                    return Err(Error("missing protocol versions".into()));
                }
            }
            "protocol_version" => {
                if row["kind"] != 65535 {
                    return Err(Error("unexpected protocol versions".into()));
                }
                w.u8(1);
                for v in array(v, "protocol")? {
                    w.u16(number(v)? as u16);
                }
            }
            "creation_save_versions" => {
                w.u8(0);
                for a in ["creation", "save"] {
                    for v in array(v, a)? {
                        w.u16(number(v)? as u16);
                    }
                }
            }
            _ => return Err(Error("unknown registration mode".into())),
        }
    }
    w.id(&g.birth);
    w.u32(0x841);
    w.u16(65535);
    w.u8(0);
    versions(&mut w, &CREATION);
    versions(&mut w, &SAVE);
    Ok(w.0)
}
fn private_properties() -> Result<Vec<u8>> {
    let terminated = |text: &str| {
        let mut w = Wire::default();
        w.u32(text.encode_utf16().count() as u32 + 1);
        for v in text.encode_utf16() {
            w.u16(v);
        }
        w.u16(0);
        w.0
    };
    let mut rows = Vec::new();
    let mut w = Wire::default();
    w.u32(2);
    w.u16(1200);
    w.u16(0);
    rows.push((1, w.0));
    let mut w = Wire::default();
    w.words(&[19, 1033]);
    rows.push((0x80000000, w.0));
    for (pid, text) in [(14, "1"), (255, "Design Tracking Control")] {
        let mut w = Wire::default();
        w.u32(31);
        w.0.extend(terminated(text));
        rows.push((pid, w.0));
    }
    let mut w = Wire::default();
    w.words(&[1, 255]);
    w.0.extend(terminated("Property Set Name"));
    rows.push((0, w.0));
    let start = 8 + rows.len() * 8 + 8;
    let mut table = Wire::default();
    let mut values = Vec::new();
    for (pid, value) in rows {
        table.words(&[pid, (start + values.len()) as u32]);
        values.extend(&value);
        values.resize(values.len() + ((4 - value.len() % 4) % 4), 0);
    }
    let mut section = Wire::default();
    section.words(&[(start + values.len()) as u32, 5]);
    section.0.extend(table.0);
    section.u64(0);
    section.0.extend(values);
    let mut header = Wire::default();
    header.u16(0xfffe);
    header.u16(0);
    header.u32(131082);
    header.id(&guid("b6b5dc40-96e3-11d2-b774-0060b0f159ef")?);
    header.u32(1);
    header.id(&guid("d861fb30-3136-11d1-9e92-0060b03c1ca6")?);
    header.u32(48);
    header.0.extend(section.0);
    Ok(header.0)
}
fn interfaces() -> Result<Vec<u8>> {
    let mut w = Wire::default();
    w.id(&guid("74e51a55-4b2f-5e18-2b5f-2ba095d9e9f7")?);
    w.u16(1);
    for (id, name, wide) in [
        (
            "53ab4df9-11d4-619c-6000-faab3a39fbb0",
            "HasInterfaces",
            false,
        ),
        (
            "ffbee006-11d4-58fe-1000-be969bba47b5",
            "DocumentHasPatients",
            false,
        ),
        (
            "0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e",
            "DerivedAssembly",
            false,
        ),
        ("0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e", "DerivedPart", false),
        (
            "0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e",
            "DerivedAlias",
            false,
        ),
        ("0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e", "Holes", true),
        ("0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e", "Patterns", true),
        ("0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e", "Threads", false),
        (
            "0c9e7e18-4f8b-54c8-2ec6-369eb6e1956e",
            "BreakLinkMember",
            false,
        ),
        ("195efb1d-4358-0040-0d2f-058e3248b9f6", "Desel", false),
        ("195efb1d-4358-0040-0d2f-058e3248b9f6", "Sick", false),
        (
            "195efb1d-4358-0040-0d2f-058e3248b9f6",
            "HasCrossPartRefs",
            false,
        ),
        ("29468546-4c35-102a-8284-29af96ff63b0", "LicenseType", true),
    ] {
        w.id(&guid(id)?);
        let padding = if name == "LicenseType" { 7 } else { 0 };
        w.u32((name.len() + padding) as u32);
        w.0.extend(name.as_bytes());
        w.0.resize(w.0.len() + padding, 0);
        w.u32(if wide { 4 } else { 1 });
        if wide {
            w.u32(0);
        } else {
            w.u8(0);
        }
    }
    w.id(&guid("11e46e2b-452e-ba86-fdbe-889e85cdb5a3")?);
    Ok(w.0)
}
fn ufrx(g: &Graph<'_>) -> Vec<u8> {
    let sections = [
        31, 32, 18, 28, 3, 12, 7, 4, 1, 5, 1, 2, 7, 2, 3, 7, 0, 5, 5, 0, 1, 0, 1, 0, 3, 2, 2,
    ];
    let mut w = Wire::default();
    w.u16(15);
    w.u16(sections.len() as u16);
    versions(&mut w, &sections);
    versions(&mut w, &SAVE);
    w.0.resize(w.0.len() + 24, 0);
    w.text("");
    w.0.resize(w.0.len() + 32, 0);
    w.id(&g.revision);
    w.u32(0x8000);
    w.id(&g.birth);
    w.text("");
    w.u16(0);
    w.words(&[1, 2]);
    w.text("");
    w.words(&[0, 0, 0]);
    versions(&mut w, &[2, 0]);
    w.text("");
    versions(&mut w, &[2, 0]);
    w.words(&[2, 0]);
    versions(&mut w, &[0, 1]);
    w.0.resize(w.0.len() + 40, 0);
    w.u32(1);
    w.u8(0);
    w.text("");
    versions(&mut w, &[2, 0]);
    w.words(&[0, 0]);
    let mut suffix = vec![0; 77];
    let mut version = Wire::default();
    // M1 V24's accepted constructor uses the canonical version in these
    // empty suffix branches; the save version belongs to the prefix only.
    versions(&mut version, &[0, 16415, 0, 0]);
    for start in [11, 27, 59] {
        suffix[start..start + 8].copy_from_slice(&version.0);
    }
    w.0.extend(suffix);
    w.u32(0);
    w.text("");
    w.u32(0);
    w.u8(0);
    w.u32(0);
    w.u8(0);
    w.words(&[0, 0, 0, 28]);
    w.u8(4);
    w.text("");
    versions(&mut w, &[15, 16]);
    for (tag, count) in [(17, 2), (18, 1), (19, 2), (21, 1)] {
        w.u8(tag);
        w.0.resize(w.0.len() + 16 * count, 0);
    }
    // Selected M1 V24 accepted empty-branch reservation: 433 zero bytes.
    w.0.resize(w.0.len() + 433, 0);
    w.0
}
fn text_property_set(fmtid: &str, pid: u32, text: &str, set_name: &str) -> Result<Vec<u8>> {
    let wide = |text: &str| {
        let mut w = Wire::default();
        w.u32(31);
        w.u32(text.encode_utf16().count() as u32 + 1);
        for c in text.encode_utf16() {
            w.u16(c);
        }
        w.u16(0);
        while w.0.len() % 4 != 0 {
            w.u8(0);
        }
        w.0
    };
    let mut cp = Wire::default();
    cp.words(&[2, 1200]);
    let rows = [(1, cp.0), (pid, wide(text)), (255, wide(set_name))];
    let mut section = Wire::default();
    let start = 8 + rows.len() * 8;
    section.words(&[
        (start + rows.iter().map(|r| r.1.len()).sum::<usize>()) as u32,
        rows.len() as u32,
    ]);
    let mut offset = start;
    for (id, value) in &rows {
        section.words(&[*id, offset as u32]);
        offset += value.len();
    }
    for (_, value) in rows {
        section.0.extend(value);
    }
    let mut header = Wire::default();
    header.u16(0xfffe);
    header.u16(0);
    header.u32(131082);
    header.id(&guid("b6b5dc40-96e3-11d2-b774-0060b0f159ef")?);
    header.u32(1);
    header.id(&guid(fmtid)?);
    header.u32(48);
    header.0.extend(section.0);
    Ok(header.0)
}
pub(super) fn streams(g: &Graph<'_>) -> Result<BTreeMap<String, Vec<u8>>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?;
    let time = (now.as_secs() + 11_644_473_600) * 10_000_000 + now.subsec_nanos() as u64 / 100;
    let mut db = Wire::default();
    db.id(&g.birth);
    db.u32(31);
    versions(&mut db, &CREATION);
    db.u64(time);
    versions(&mut db, &SAVE);
    db.u64(time);
    db.utf16("");
    let mut streams = BTreeMap::from([
        ("/RSeStorage/V1/RSeDb".into(), db.0),
        ("/RSeStorage/RSeDbRevisionInfo".into(), revision_info(g)?),
        ("/UFRxDoc".into(), ufrx(g)),
        (
            "/\u{5}Qz4dgm1gRjudbpksAayal4qdGf".into(),
            private_properties()?,
        ),
        (
            "/RSeStorage/RSeEmbeddings/DatabaseInterfaces".into(),
            interfaces()?,
        ),
    ]);
    if let Some((title, part_number)) = &g.metadata {
        streams.insert(
            "/\u{5}SummaryInformation".into(),
            text_property_set(
                "f29f85e0-4ff9-1068-ab91-08002b27b3d9",
                2,
                title,
                "Summary Information",
            )?,
        );
        streams.insert(
            "/\u{5}PypkizqiUjudbposAayal4qdGf".into(),
            text_property_set(
                "32853f0f-3444-11d1-9e93-0060b03c1ca6",
                5,
                part_number,
                "Design Tracking Properties",
            )?,
        );
    }
    Ok(streams)
}
