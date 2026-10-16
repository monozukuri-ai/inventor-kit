//! Typed CFB/RSe31 serialization for the fixed assembly profile.
use super::super::{array, err, guid, number, string, Wire};
use super::*;
use serde_json::{json, Value};

pub(super) const ANCHORS: [&str; 8] = [
    "ae26ac7d-cd33-11d3-b589-00902707c6f2",
    "ae26ac83-cd33-11d3-b589-00902707c6f2",
    "ae26ac85-cd33-11d3-b589-00902707c6f2",
    "ae26ac87-cd33-11d3-b589-00902707c6f2",
    "ae26ac89-cd33-11d3-b589-00902707c6f2",
    "901b8282-3e43-11d5-8dce-00b0d03d3835",
    "38a42066-1ce6-11d6-8e0b-00b0d03d3835",
    "d25c16f6-229d-4df5-8539-a7c5f63edb55",
];
fn shorts(w: &mut Wire, values: &[u16]) {
    for &v in values {
        w.u16(v);
    }
}
fn words(w: &mut Wire, v: &Value) -> Result<()> {
    for n in v
        .as_array()
        .ok_or_else(|| Error("missing IAM words".into()))?
    {
        w.u32(number(n)?);
    }
    Ok(())
}
fn short_values(w: &mut Wire, v: &Value) -> Result<()> {
    for n in v
        .as_array()
        .ok_or_else(|| Error("missing IAM shorts".into()))?
    {
        w.u16(number(n)?.try_into().map_err(err)?);
    }
    Ok(())
}
fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::bulk::compress(bytes, 3).map_err(err)
}
fn state(doc: &PreparedAssembly, label: &str) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::parse_str(&doc.document_id).unwrap(),
        label.as_bytes(),
    )
    .to_string()
}
fn scalar(w: &mut Wire, kind: &str, value: &Value) -> Result<()> {
    if kind == "u48" {
        let n = value
            .as_u64()
            .filter(|&n| n < 1 << 48)
            .ok_or_else(|| Error("invalid IAM u48".into()))?;
        w.0.extend(&n.to_le_bytes()[..6]);
        Ok(())
    } else {
        w.field(kind, value)
    }
}
fn asm_name(w: &mut Wire, name: &str) {
    let parts: Vec<_> = name.split('-').collect();
    for (i, p) in parts.iter().enumerate() {
        w.u8(if i + 1 == parts.len() { 13 } else { 14 });
        w.u8(p.len() as u8);
        w.0.extend(p.as_bytes());
    }
}
fn asm_text(w: &mut Wire, text: &str) {
    w.u8(7);
    w.u8(text.len() as u8);
    w.0.extend(text.as_bytes());
}
fn asm_tokens(w: &mut Wire, rows: &[(u8, i32)]) {
    for &(tag, v) in rows {
        w.u8(tag);
        if tag != 11 {
            w.0.extend(v.to_le_bytes());
        }
    }
}
fn carrier(
    owner: &str,
    key: u16,
    ordinal: usize,
    helper: Option<usize>,
    target: TargetVersion,
) -> Result<Vec<u8>> {
    let dc = owner == allocation::DC;
    if !dc && owner != "AmBREPSegment" {
        return Err(Error("unknown IAM carrier owner".into()));
    }
    let mut w = Wire::default();
    w.u32(0);
    w.u16(key);
    w.words(&[if dc { 0x357 } else { 0x31d }, 44]);
    w.0.extend(b"ASM BinaryFile4");
    w.words(&[target.sab_version(), 0, 1, 3]);
    for text in [
        "inventor-kit",
        &format!("ASM {} NT", target.asm_version()),
        "",
    ] {
        asm_text(&mut w, text);
    }
    for v in [10., 1e-6, 1e-10] {
        w.u8(6);
        w.f64(v);
    }
    asm_name(&mut w, "asmheader");
    asm_tokens(&mut w, &[(12, -1), (4, -1)]);
    asm_text(&mut w, target.asm_version());
    w.u8(17);
    asm_name(&mut w, "Begin-of-ASM-History-Data");
    asm_name(&mut w, "history_stream");
    asm_tokens(
        &mut w,
        &[
            (4, 2),
            (4, 2),
            (4, 0),
            (4, 0),
            (12, -1),
            (12, 0),
            (12, 0),
            (12, -1),
        ],
    );
    w.u8(17);
    asm_name(&mut w, "delta_state");
    asm_tokens(
        &mut w,
        &[
            (4, 2),
            (4, 1),
            (4, 0),
            (12, -1),
            (12, -1),
            (12, 0),
            (12, -1),
            (12, 0),
            (11, 0),
        ],
    );
    asm_tokens(
        &mut w,
        if dc {
            &[(4, 1), (12, 0), (4, 0), (4, 0), (4, 0), (4, 0)]
        } else {
            &[(4, 0), (4, 0)]
        },
    );
    w.u8(17);
    asm_name(&mut w, "End-of-ASM-History-Section");
    asm_name(&mut w, "End-of-ASM-data");
    w.u32(0x80000000 | (ordinal + 1) as u32);
    w.u8(1);
    w.u32(2);
    w.u8(0);
    w.u32(helper.map_or(0, |i| 0x80000000 | (i + 1) as u32));
    w.u32(u32::MAX);
    Ok(w.0)
}
fn graph(plan: &Value, doc: &PreparedAssembly) -> Result<BTreeMap<String, Vec<Vec<u8>>>> {
    let graph = &plan["graph"];
    let mut out = BTreeMap::new();
    for (owner, rows) in graph["owners"].as_object().unwrap() {
        let rows = rows.as_array().unwrap();
        let mut roles = BTreeMap::new();
        let mut keys = BTreeSet::new();
        for (i, r) in rows.iter().enumerate() {
            if roles.insert(string(r, "role")?, i).is_some() {
                return Err(Error("duplicate IAM role".into()));
            }
            if !r["assigned_key"].is_null() {
                let k = number(&r["assigned_key"])?;
                if k == 0 || k >= 65535 || !keys.insert(k) {
                    return Err(Error("invalid IAM key allocation".into()));
                }
            }
        }
        let nodes = array(&graph["registry_nodes"], owner)?;
        let mut payloads = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let recipe = &r["recipe"];
            if recipe["kind"] == "empty-asm-carrier" {
                let helper = recipe["helper_role"]
                    .as_str()
                    .map(|s| {
                        roles
                            .get(s)
                            .copied()
                            .ok_or_else(|| Error("unresolved carrier helper".into()))
                    })
                    .transpose()?;
                payloads.push(carrier(
                    owner,
                    number(&r["assigned_key"])? as u16,
                    i,
                    helper,
                    doc.target()?,
                )?);
                continue;
            }
            if recipe["kind"] != "fields" {
                return Err(Error("unknown IAM record recipe".into()));
            }
            let mut w = Wire::default();
            for f in array(recipe, "fields")? {
                let value = match string(f, "policy")? {
                    "fixed" => f["value"].clone(),
                    "assigned-key" => r["assigned_key"].clone(),
                    "document-revision" => json!(doc.creation_revision_id),
                    "document-state" => json!(state(doc, string(f, "state_role")?)),
                    "local-reference" => {
                        let i = *roles
                            .get(string(f, "target_role")?)
                            .ok_or_else(|| Error("unresolved local IAM reference".into()))?;
                        if rows[i]["class"] != f["target_class"] {
                            return Err(Error("IAM target class differs".into()));
                        }
                        let namespace = match string(f, "namespace")? {
                            "local_bare" => 0,
                            "local_qualified" => 0x80000000,
                            _ => return Err(Error("invalid IAM reference namespace".into())),
                        };
                        json!(namespace | (i + 1) as u32)
                    }
                    "registry-reference" => json!(
                        nodes
                            .iter()
                            .position(|n| *n == f["target_node"])
                            .ok_or_else(|| Error("unresolved IAM registry reference".into()))?
                            + 1
                    ),
                    _ => return Err(Error("unknown IAM field policy".into())),
                };
                scalar(&mut w, string(f, "kind")?, &value)?;
            }
            if w.0.is_empty() || w.0.len() > 1_000_000 {
                return Err(Error("IAM record budget exceeded".into()));
            }
            payloads.push(w.0);
        }
        out.insert(owner.clone(), payloads);
    }
    Ok(out)
}
fn bulk(owner: &Value, records: &[Value], payloads: &[Vec<u8>]) -> Result<Vec<u8>> {
    let classes = array(owner, "classes")?;
    let mut w = Wire::default();
    for (record, payload) in records.iter().zip(payloads) {
        let i = classes
            .iter()
            .position(|c| c["guid"] == record["class"])
            .ok_or_else(|| Error("unregistered IAM class".into()))?;
        if i > 255 {
            return Err(Error("IAM class budget exceeded".into()));
        }
        w.u32(0x100 | i as u32);
        w.0.extend(payload);
        w.u32(payload.len() as u32);
        let extension = match string(record, "class")? {
            "6759d870-11d2-7838-6000-94b70b02ecb0" => Some(("EnabledExtraAlpha", 1, 3)),
            "1e5cbb86-11d2-b0cd-6000-aab70b02ecb0" => Some(("Exposure", 10, 6)),
            _ => None,
        };
        if let Some((name, tag, width)) = extension {
            w.u8(1);
            w.u32(1);
            w.text(name);
            w.u32(tag);
            w.0.resize(w.0.len() + width, 0);
            w.words(&[0x30000006, 0]);
        } else {
            w.u8(0);
        }
    }
    w.u32(u32::MAX);
    w.u16(array(owner, "roots")?.len() as u16);
    words(&mut w, &owner["roots"])?;
    w.u32(array(owner, "root_pairs")?.len() as u32);
    for pair in array(owner, "root_pairs")? {
        w.u32(number(&pair["word"])?);
        w.u16(number(&pair["short"])? as u16);
    }
    let mut out = Wire::default();
    out.id(&guid("a42bc29e-11d4-01e8-6000-2db3ee29fbb0")?);
    out.u16(516);
    out.0.extend(compress(&w.0)?);
    Ok(out.0)
}
fn meta6(p: &Value) -> Result<Vec<u8>> {
    let mut w = Wire::default();
    for block in array(p, "blocks")? {
        let kind = number(&block["kind"])?;
        let start = number(&block["start"])?;
        let end = number(&block["end"])?;
        let selected: BTreeSet<u32> = array(block, "selected_ids")?
            .iter()
            .map(number)
            .collect::<Result<_>>()?;
        if start % 256 != 0 || end > 65536 || selected.iter().any(|&i| i < start || i >= end) {
            return Err(Error("invalid IAM bitmap extent".into()));
        }
        w.u8(kind as u8);
        w.u16(0);
        w.u32(start);
        if kind == 2 {
            w.u16(0);
            w.u32(end);
        } else if kind == 1 && end == start + 256 {
            let mut occupied = 0u32;
            let mut mixed = 0u32;
            let mut values = Vec::new();
            for byte in 0..32 {
                let value = (0..8)
                    .filter(|bit| selected.contains(&(start + byte * 8 + bit)))
                    .fold(0u8, |acc, bit| acc | 1 << bit);
                if value > 0 {
                    occupied |= 1 << byte;
                }
                if value != 0 && value != 255 {
                    mixed |= 1 << byte;
                    values.push(value);
                }
            }
            w.words(&[occupied, mixed]);
            w.0.extend(values);
        } else {
            return Err(Error("invalid IAM bitmap kind".into()));
        }
    }
    w.u8(0);
    w.u16(0);
    w.u32(number(&p["allocator"])?);
    w.u16(array(p, "contexts")?.len() as u16);
    for c in array(p, "contexts")? {
        w.id(&guid(string(c, "guid")?)?);
        w.u32(number(&c["index"])?);
    }
    w.u16(array(p, "terminal")?.len() as u16);
    for t in array(p, "terminal")? {
        w.u32(number(&t["word"])?);
        w.u16(number(&t["short"])? as u16);
    }
    Ok(w.0)
}
fn segment_id(doc: &PreparedAssembly, name: &str) -> Result<[u8; 16]> {
    guid(
        &doc.segments
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| Error("missing IAM segment".into()))?
            .id,
    )
}
fn meta(p: &Value, doc: &PreparedAssembly, index: usize, payloads: &[Vec<u8>]) -> Result<Vec<u8>> {
    let name = string(p, "name")?;
    let allocator = number(&p["meta6"]["allocator"])?;
    let mut header = Wire::default();
    header.text("RSe Meta Stream Version 8");
    header.u16(8);
    short_values(&mut header, &p["header_words"])?;
    header.utf16(name);
    header.id(&segment_id(doc, name)?);
    header.words(&[allocator, 0, 16]);
    header.text("");
    header.text("");
    header.u8(2);
    let mut counts = [0u32; 12];
    let mut data: Vec<Vec<u8>> = vec![Vec::new(); 12];
    let mut w = Wire::default();
    for payload in payloads {
        w.u32(0x80000000 | payload.len() as u32);
    }
    counts[1] = payloads.len() as u32;
    data[1] = w.0;
    let mut w = Wire::default();
    w.words(&[index as u32, 0]);
    w.u16(0);
    w.words(&[8, allocator]);
    w.u16(0);
    counts[2] = 2;
    data[2] = w.0;
    let classes = array(p, "classes")?;
    let mut w = Wire::default();
    for c in classes {
        w.id(&guid(string(c, "guid")?)?);
        w.u16(number(&c["word0"])? as u16);
        w.u32(array(c, "dependencies")?.len() as u32);
        w.u16(number(&c["word2"])? as u16);
        w.u32(number(&c["word3"])?);
    }
    counts[4] = classes.len() as u32;
    data[4] = w.0;
    let mut w = Wire::default();
    let mut seen = BTreeSet::new();
    for id in array(p, "dependency_order")? {
        let i = classes
            .iter()
            .position(|c| c["guid"] == *id)
            .ok_or_else(|| Error("missing IAM dependency class".into()))?;
        w.u16(i as u16);
        for dep in array(&classes[i], "dependencies")? {
            if !seen.contains(dep.as_str().unwrap()) {
                return Err(Error("IAM dependency order differs".into()));
            }
            w.u16(classes.iter().position(|c| c["guid"] == *dep).unwrap() as u16);
        }
        seen.insert(id.as_str().unwrap());
    }
    counts[5] = u32::from_le_bytes(w.0[..4].try_into().unwrap());
    data[5] = w.0[4..].to_vec();
    let six = meta6(&p["meta6"])?;
    counts[6] = u32::from_le_bytes(six[..4].try_into().unwrap());
    data[6] = six[4..].to_vec();
    let revision = guid(&doc.creation_revision_id)?;
    let mut w = Wire::default();
    for target in array(p, "object_owners")? {
        w.id(&segment_id(doc, target.as_str().unwrap())?);
        w.id(&revision);
    }
    counts[7] = array(p, "object_owners")?.len() as u32;
    data[7] = w.0;
    let nodes = array(p, "nodes")?;
    let mut w = Wire::default();
    for (i, interface) in array(p, "interfaces")?.iter().enumerate() {
        let uses = nodes.iter().filter(|n| n["interface"] == i).count() as u32;
        w.id(&guid(string(interface, "guid")?)?);
        w.u32(uses << 16 | number(&interface["object_slot"])?);
    }
    counts[8] = array(p, "interfaces")?.len() as u32;
    data[8] = w.0;
    let mut w = Wire::default();
    for n in nodes {
        shorts(
            &mut w,
            &[
                number(&n["interface"])? as u16,
                0,
                0,
                number(&n["key"])? as u16,
            ],
        );
    }
    counts[10] = nodes.len() as u32;
    data[10] = w.0;
    counts[11] = 18;
    data[11] = vec![0; 72];
    let mut body = Wire::default();
    body.u32(payloads.len() as u32 + number(&p["record_reservation"])?);
    body.u16(array(p, "roots")?.len() as u16);
    body.words(&[nodes.len() as u32 + 1, header.0.len() as u32 + 10]);
    for i in 1..=4 {
        body.u32(counts[i]);
        body.0.extend(&data[i]);
        if i != 4 {
            body.u32(4 + data[i].len() as u32);
        }
    }
    for i in 5..=11 {
        body.u32(4 + data[i - 1].len() as u32);
        body.u32(counts[i]);
        body.0.extend(&data[i]);
    }
    body.id(&guid("9744e6a4-11d1-8dd8-0008-2998bedddc09")?);
    header.0.extend(compress(&body.0)?);
    Ok(header.0)
}
fn registry(table: &Value, doc: &PreparedAssembly) -> Result<Vec<u8>> {
    let owners = array(table, "owners")?;
    let mut w = Wire::default();
    w.u32(8);
    let revision = guid(&doc.creation_revision_id)?;
    for p in owners {
        let name = string(p, "name")?;
        let objects = array(p, "object_owners")?;
        let nodes = array(p, "nodes")?;
        w.utf16(name);
        w.id(&segment_id(doc, name)?);
        w.id(&revision);
        w.words(&[0, objects.len() as u32]);
        words(&mut w, &p["registry_state"])?;
        w.utf16(string(p, "kind")?);
        words(&mut w, &p["type_words"])?;
        shorts(&mut w, &doc.target()?.save());
        w.u32(u32::from(objects.is_empty()));
        for (slot, target) in objects.iter().enumerate() {
            w.id(&revision);
            w.u32(0);
            w.u8(0);
            w.u32(0);
            w.id(&segment_id(doc, target.as_str().unwrap())?);
            w.u32(nodes.iter().filter(|n| n["object_slot"] == slot).count() as u32);
            w.u32(if slot + 1 == objects.len() {
                nodes.len() as u32 + 1
            } else {
                0
            });
        }
        for n in nodes {
            w.u32(number(&n["object_slot"])? << 16 | number(&n["interface"])?);
            for (table_name, key) in [
                ("registry_classes", "class"),
                ("registry_contexts", "context"),
            ] {
                w.u16(
                    array(table, table_name)?
                        .iter()
                        .position(|v| *v == n[key])
                        .ok_or_else(|| Error("unregistered IAM endpoint".into()))?
                        as u16,
                );
            }
            w.u64(0);
            w.u32(0);
            w.u16(number(&n["key"])? as u16);
        }
    }
    w.u32(0x810);
    for key in ["registry_classes", "registry_contexts"] {
        let a = array(table, key)?;
        w.u32(a.len() as u32);
        for v in a {
            w.id(&guid(v.as_str().unwrap())?);
        }
    }
    Ok(w.0)
}
fn native_id(id: &str) -> Result<[u8; 16]> {
    let bytes = guid(id)?;
    let v: Vec<_> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    guid(&format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:04x}{:08x}",
        v[0],
        v[1] >> 16,
        v[1] & 65535,
        v[2] >> 16,
        v[2] & 65535,
        v[3]
    ))
}
fn text_property(value: &str) -> Vec<u8> {
    let mut w = Wire::default();
    w.u32(31);
    w.utf16(&format!("{value}\0"));
    w.0
}
fn properties(doc: &PreparedAssembly) -> Result<Vec<u8>> {
    let mut codepage = Wire::default();
    codepage.u32(2);
    codepage.u16(1200);
    codepage.u16(0);
    let mut locale = Wire::default();
    locale.words(&[19, 1033]);
    let mut rows = vec![
        (1, codepage.0),
        (0x80000000, locale.0),
        (14, text_property("1")),
    ];
    for (pid, id) in [(18, &doc.creation_revision_id), (21, &doc.document_id)] {
        let mut w = Wire::default();
        w.u32(72);
        w.id(&native_id(id)?);
        rows.push((pid, w.0));
    }
    rows.push((255, text_property("Design Tracking Control")));
    let mut dictionary = Wire::default();
    dictionary.words(&[1, 255]);
    dictionary.utf16("Property Set Name\0");
    rows.push((0, dictionary.0));
    let start = 8 + rows.len() * 8 + 8;
    let mut table = Wire::default();
    let mut values = Vec::new();
    for (pid, bytes) in &rows {
        table.words(&[*pid, (start + values.len()) as u32]);
        values.extend(bytes);
        values.resize(values.len() + (4 - bytes.len() % 4) % 4, 0);
    }
    let mut section = Wire::default();
    section.words(&[(start + values.len()) as u32, rows.len() as u32]);
    section.0.extend(table.0);
    section.u64(0);
    section.0.extend(values);
    let mut w = Wire::default();
    shorts(&mut w, &[0xfffe, 0]);
    w.u32(131082);
    w.id(&guid("b6b5dc40-96e3-11d2-b774-0060b0f159ef")?);
    w.u32(1);
    w.id(&guid("d861fb30-3136-11d1-9e92-0060b03c1ca6")?);
    w.u32(48);
    w.0.extend(section.0);
    Ok(w.0)
}
fn interfaces() -> Result<Vec<u8>> {
    let mut w = Wire::default();
    w.id(&guid("74e51a55-4b2f-5e18-2b5f-2ba095d9e9f7")?);
    w.u16(1);
    for (id, name, width) in [
        ("53ab4df9-11d4-619c-6000-faab3a39fbb0", "HasInterfaces", 1),
        (
            "ffbee006-11d4-58fe-1000-be969bba47b5",
            "DocumentHasPatients",
            1,
        ),
        (
            "7529e419-4454-3fee-ceb7-b98e746d29fb",
            "ActiveFeatureGroupGUID",
            16,
        ),
        ("195efb1d-4358-0040-0d2f-058e3248b9f6", "Desel", 1),
        ("195efb1d-4358-0040-0d2f-058e3248b9f6", "Sick", 1),
        (
            "195efb1d-4358-0040-0d2f-058e3248b9f6",
            "HasCrossPartRefs",
            1,
        ),
        ("29468546-4c35-102a-8284-29af96ff63b0", "LicenseType", 4),
    ] {
        w.id(&guid(id)?);
        let padding = if name == "LicenseType" { 8 } else { 0 };
        w.u32((name.len() + padding) as u32);
        w.0.extend(name.as_bytes());
        w.0.resize(w.0.len() + padding, 0);
        w.u32(width);
        w.0.resize(w.0.len() + width as usize, 0);
    }
    w.id(&guid("11e46e2b-452e-ba86-fdbe-889e85cdb5a3")?);
    Ok(w.0)
}
pub(super) fn streams(plan: &Value, doc: &PreparedAssembly) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let payloads = graph(plan, doc)?;
    let table = &plan["tables"];
    for owner in array(table, "owners")? {
        let name = string(owner, "name")?;
        let rows = array(&plan["graph"]["owners"], name)?;
        let allocator = number(&owner["meta6"]["allocator"])?;
        let mut keys = BTreeSet::new();
        if number(&owner["registry_state"][3])? as usize != rows.len() {
            return Err(Error(format!("IAM record census differs: {name}")));
        }
        for record in rows {
            if !record["assigned_key"].is_null() {
                let key = number(&record["assigned_key"])?;
                if key == 0 || key > allocator || !keys.insert(key) {
                    return Err(Error(format!(
                        "IAM key allocation exceeds or conflicts with owner: {name}"
                    )));
                }
            }
        }
        if name == "AmAppSegment" && number(&owner["registry_state"][4])? as usize != keys.len() {
            return Err(Error("IAM App keyed record census differs".into()));
        }
    }
    let mut db = Wire::default();
    db.id(&guid(&doc.document_id)?);
    db.u32(31);
    shorts(&mut db, &doc.target()?.creation());
    db.u64(0);
    shorts(&mut db, &doc.target()?.save());
    db.u64(0);
    db.u32(0);
    out.insert("/RSeStorage/V1/RSeDb".into(), db.0);
    let mut revisions = Wire::default();
    revisions.words(&[3, 9]);
    for (i, id) in ANCHORS.iter().enumerate() {
        revisions.id(&guid(id)?);
        revisions.u32(0);
        let version = match i {
            0 => Some([0, 16384, 0, 0]),
            7 => Some([0, 16400, 9000, 6800]),
            _ => None,
        };
        revisions.u16(if version.is_some() { 65535 } else { 0 });
        if let Some(v) = version {
            revisions.u8(1);
            shorts(&mut revisions, &v);
        }
    }
    revisions.id(&guid(&doc.creation_revision_id)?);
    revisions.u32(0x810);
    revisions.u16(65535);
    revisions.u8(0);
    shorts(&mut revisions, &doc.target()?.creation());
    shorts(&mut revisions, &doc.target()?.save());
    out.insert("/RSeStorage/RSeDbRevisionInfo".into(), revisions.0);
    out.insert("/RSeStorage/RSeSegInfo".into(), registry(table, doc)?);
    out.insert("/\u{5}Qz4dgm1gRjudbpksAayal4qdGf".into(), properties(doc)?);
    out.insert(
        "/RSeStorage/RSeEmbeddings/DatabaseInterfaces".into(),
        interfaces()?,
    );
    let mut uf = Wire::default();
    for f in array(plan, "ufrx_fields")? {
        let value = match string(f, "policy")? {
            "fixed" => f["value"].clone(),
            "document_id" => json!(doc.document_id),
            "revision_id" => json!(doc.creation_revision_id),
            "document-state" => json!(state(doc, string(f, "state_role")?)),
            "part-path" => json!(
                doc.definitions[number(&f["reference_index"])? as usize]
                    .part
                    .relative_path
            ),
            _ => return Err(Error("unknown IAM UFRx field policy".into())),
        };
        scalar(&mut uf, string(f, "kind")?, &value)?;
    }
    out.insert("/UFRxDoc".into(), uf.0);
    for (i, p) in array(table, "owners")?.iter().enumerate() {
        let name = string(p, "name")?;
        let token = super::super::rse::token(&segment_id(doc, name)?);
        out.insert(
            format!("/RSeStorage/M{token}"),
            meta(p, doc, i, &payloads[name])?,
        );
        out.insert(
            format!("/RSeStorage/B{token}"),
            bulk(p, array(&plan["graph"]["owners"], name)?, &payloads[name])?,
        );
    }
    if out.len() != 22 {
        return Err(Error("IAM stream census differs".into()));
    }
    out.insert(
        "/\u{5}PypkizqiUjudbposAayal4qdGf".into(),
        super::super::properties::text_property_set(
            "32853f0f-3444-11d1-9e93-0060b03c1ca6",
            5,
            &doc.part_number,
            "Design Tracking Properties",
        )?,
    );
    Ok(out)
}
