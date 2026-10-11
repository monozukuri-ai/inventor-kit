use super::document::Graph;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const VERSION: [u16; 4] = [0, 16415, 0, 15500];
const TERMINAL: &str = "9744e6a4-11d1-8dd8-0008-2998bedddc09";
pub(super) fn token(id: &[u8; 16]) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz123456";
    let words: Vec<_> = id
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let mut s = String::new();
    for word in &words {
        for shift in (0..6).rev() {
            s.push(alphabet[((word >> (shift * 5)) & 31) as usize] as char);
        }
    }
    for p in words.chunks_exact(2) {
        s.push(alphabet[(((p[0] >> 30) << 2) | (p[1] >> 30)) as usize] as char);
    }
    s
}
fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    // Complete RSe frames follow the native profile's declared content size.
    // The streaming encoder omits it without a pledged input length.
    zstd::bulk::compress(bytes, 3).map_err(err)
}
fn endpoints<'a>(g: &'a Graph<'_>, owner: &str) -> Result<Vec<&'a Value>> {
    Ok(array(&g.profile.0, "endpoints")?
        .iter()
        .filter(|e| e["owner"] == owner)
        .collect())
}
fn key(g: &Graph<'_>, e: &Value) -> Result<u16> {
    let owner = string(e, "target_owner")?;
    g.get(owner, string(e, "target_role")?)?
        .key
        .ok_or_else(|| Error("registry points at keyless object".into()))
}
fn object_slot(owner: &Value, target: &str) -> Result<usize> {
    array(owner, "object_owners")?
        .iter()
        .position(|o| o == target)
        .ok_or_else(|| Error("missing object slot".into()))
}
fn maximum_key(g: &Graph<'_>, owner: &str) -> Result<u32> {
    Ok(g.records(owner)?
        .iter()
        .filter_map(|r| g.get(owner, r["role"].as_str()?).ok()?.key)
        .max()
        .ok_or_else(|| Error("owner has no keyed record".into()))? as u32)
}
fn allocator(g: &Graph<'_>, owner: &Value) -> Result<u32> {
    let offset = number(&owner["allocator_offset"])?;
    if offset > 1 {
        return Err(Error("unqualified allocator offset".into()));
    }
    Ok(maximum_key(g, string(owner, "name")?)? + offset)
}
fn meta(g: &Graph<'_>, owner: &Value, payloads: &[Vec<u8>]) -> Result<Vec<u8>> {
    let name = string(owner, "name")?;
    let classes = array(owner, "classes")?;
    let eps = endpoints(g, name)?;
    let alloc = allocator(g, owner)?;
    let mut m = Wire::default();
    m.text("RSe Meta Stream Version 8");
    m.u16(8);
    for v in array(owner, "header_words")? {
        m.u16(number(v)? as u16);
    }
    m.utf16(name);
    m.id(&g.ids[name]);
    m.words(&[alloc, 0, if name == "PmBRepSegment" { 80 } else { 16 }]);
    m.text("");
    m.text("");
    m.u8(2);
    let offset_alias = u32::try_from(m.0.len() + 10).map_err(err)?;
    let mut sections: Vec<Vec<u8>> = vec![Vec::new(); 12];
    let mut counts = [0u32; 12];
    let mut block = Wire::default();
    for p in payloads {
        block.u32(0x80000000 | p.len() as u32);
    }
    counts[1] = payloads.len() as u32;
    sections[1] = block.0;
    // M1's accepted compact form retains a zero anchor before the current
    // creation revision. The anchor is not saved source revision history.
    let mut rev = Wire::default();
    rev.words(&[0, 0]);
    rev.u16(0);
    rev.u32(564);
    rev.u32(alloc);
    rev.u16(0);
    counts[2] = 2;
    sections[2] = rev.0;
    let mut types = Wire::default();
    let mut index = BTreeMap::new();
    for (i, c) in classes.iter().enumerate() {
        let class = string(c, "guid")?;
        if index.insert(class, i as u16).is_some() {
            return Err(Error("duplicate class GUID".into()));
        }
        types.id(&guid(class)?);
        types.u16(number(&c["word0"])? as u16);
        types.u32(array(c, "dependencies")?.len() as u32);
        types.u16(number(&c["word2"])? as u16);
        types.u32(number(&c["word3"])?);
    }
    counts[4] = classes.len() as u32;
    sections[4] = types.0;
    let mut deps = Wire::default();
    let mut seen = BTreeSet::new();
    for role in array(owner, "dependency_order")? {
        let class = role
            .as_str()
            .ok_or_else(|| Error("invalid dependency role".into()))?;
        let c = &classes[*index
            .get(class)
            .ok_or_else(|| Error("absent dependency class".into()))?
            as usize];
        deps.u16(index[class]);
        for dep in array(c, "dependencies")? {
            let dep = dep
                .as_str()
                .ok_or_else(|| Error("invalid dependency class".into()))?;
            if !seen.contains(dep) {
                return Err(Error(
                    "class dependency is not topologically ordered".into(),
                ));
            }
            deps.u16(index[dep]);
        }
        seen.insert(class);
    }
    if deps.0.len() < 4 {
        return Err(Error("class dependency table too short".into()));
    }
    counts[5] = u32::from_le_bytes(deps.0[..4].try_into().unwrap());
    sections[5] = deps.0[4..].to_vec();
    let mut six = Wire::default();
    if let Some(prefix) = owner["meta6_reserved_prefix"].as_u64() {
        if prefix != 8 {
            return Err(Error("unqualified Meta6 reserved prefix".into()));
        }
        // The two cache owners reserve eight initial slots independently
        // of their keyed count in this bounded native box profile.
        six.u8(1);
        six.u16(0);
        six.words(&[0, 1, 0]);
    }
    six.u8(0);
    six.u16(0);
    six.u32(alloc);
    six.u16(array(owner, "contexts")?.len() as u16);
    for c in array(owner, "contexts")? {
        six.id(&guid(string(c, "guid")?)?);
        six.u32(number(&c["index"])?);
    }
    six.u16(array(owner, "meta6_terminal")?.len() as u16);
    for row in array(owner, "meta6_terminal")? {
        six.u32(number(&row["word"])?);
        six.u16(if let Some(role) = row["key_role"].as_str() {
            g.get(name, role)?
                .key
                .ok_or_else(|| Error("Meta6 terminal key absent".into()))?
        } else {
            number(&row["short"])? as u16
        });
    }
    counts[6] = u32::from_le_bytes(six.0[..4].try_into().unwrap());
    sections[6] = six.0[4..].to_vec();
    let mut seven = Wire::default();
    for target in array(owner, "object_owners")? {
        let target = target
            .as_str()
            .ok_or_else(|| Error("invalid object owner".into()))?;
        seven.id(&g.ids[target]);
        seven.id(&g.revision);
    }
    counts[7] = array(owner, "object_owners")?.len() as u32;
    sections[7] = seven.0;
    let mut eight = Wire::default();
    for (i, row) in array(owner, "interfaces")?.iter().enumerate() {
        let id = string(row, "guid")?;
        eight.id(&if id == "birth" { g.birth } else { guid(id)? });
        let uses = eps.iter().filter(|e| e["interface_index"] == i).count() as u32;
        eight.u32((uses << 16) | number(&row["object_slot"])?);
    }
    counts[8] = array(owner, "interfaces")?.len() as u32;
    sections[8] = eight.0;
    if name == "PmDCSegment" {
        let mut nine = Wire::default();
        nine.u16(1);
        nine.u8(1);
        // The DC label context uses the label protocol identifier, shared
        // with DC label records, rather than a per-document birth identifier.
        nine.id(&guid("0ca9112d-11d0-838b-0008-19b00524dc09")?);
        counts[9] = 1;
        sections[9] = nine.0;
    }
    let mut ten = Wire::default();
    for e in &eps {
        ten.u16(number(&e["interface_index"])? as u16);
        ten.u16(0);
        ten.u16(0);
        ten.u16(key(g, e)?);
    }
    counts[10] = eps.len() as u32;
    sections[10] = ten.0;
    // Declared neutral initial statistics. Native normalization is a M3 probe,
    // not inferred from the old saved counters or serialized statistics blob.
    counts[11] = 18;
    sections[11] = vec![0; 18 * 4];
    let mut body = Wire::default();
    let reservation = match name {
        "PmBRepSegment" | "PmDCSegment" | "PmAppSegment" => 1,
        "PmBrowserSegment" => 2,
        _ => 0,
    };
    body.u32(u32::try_from(payloads.len() + reservation).map_err(err)?);
    // App's implicit roots include a keyless parameter collection. An empty
    // suffix lets native SaveAs discard that record (V43). Resolve these roots
    // symbolically and keep the Meta alias aligned with the emitted B suffix.
    // Cache owners retain their separate, previously observed alias layout.
    body.u16(if name == "PmAppSegment" {
        let roots = array(owner, "root_roles")?;
        if roots.len() != 8 {
            return Err(Error("unknown App root table length".into()));
        }
        u16::try_from(roots.len()).map_err(err)?
    } else {
        0
    });
    body.u32(
        u32::try_from(if owner["bulk_index_roles"].is_array() {
            array(owner, "bulk_index_roles")?.len()
        } else {
            eps.len() + 1
        })
        .map_err(err)?,
    );
    body.u32(offset_alias);
    for i in 1..=4 {
        body.u32(counts[i]);
        body.0.extend(&sections[i]);
        if i != 4 {
            body.u32(4 + sections[i].len() as u32);
        }
    }
    for i in 5..=11 {
        body.u32(4 + sections[i - 1].len() as u32);
        body.u32(counts[i]);
        body.0.extend(&sections[i]);
    }
    body.id(&guid(TERMINAL)?);
    m.0.extend(compress(&body.0)?);
    Ok(m.0)
}
fn registry(g: &Graph<'_>) -> Result<Vec<u8>> {
    let mut classes = BTreeSet::new();
    let mut contexts = BTreeSet::new();
    for e in array(&g.profile.0, "endpoints")? {
        classes.insert(string(e, "target_class")?.to_owned());
        contexts.insert(string(e, "context_guid")?.to_owned());
    }
    let classes: Vec<_> = classes.into_iter().collect();
    let contexts: Vec<_> = contexts.into_iter().collect();
    if classes.len() != 19 || contexts.len() != 13 {
        return Err(Error("global registry GUID role census differs".into()));
    }
    let owners = array(&g.profile.0, "owners")?;
    let mut w = Wire::default();
    w.u32(owners.len() as u32);
    for owner in owners {
        let name = string(owner, "name")?;
        let eps = endpoints(g, name)?;
        let objects = array(owner, "object_owners")?;
        w.utf16(name);
        w.id(&g.ids[name]);
        w.id(&g.revision);
        w.u32(0);
        w.u32(objects.len() as u32);
        for (i, v) in array(owner, "registry_state")?.iter().enumerate() {
            w.u32(match i {
                3 => g.records(name)?.len() as u32,
                4 => maximum_key(g, name)?,
                5 => eps.len() as u32 + 1,
                _ => number(v)?,
            });
        }
        w.utf16(string(owner, "kind")?);
        for v in array(owner, "type_words")? {
            w.u32(number(v)?);
        }
        for v in VERSION {
            w.u16(v);
        }
        // The retained native profile distinguishes owners with external
        // object rows (0) from owners without them (1). Do not use a universal
        // initial state: our reader accepts it, but native sources do not emit
        // it for Graphics/DC/Browser/App.
        w.u32(u32::from(objects.is_empty()));
        for (slot, target) in objects.iter().enumerate() {
            let target = target
                .as_str()
                .ok_or_else(|| Error("object owner absent".into()))?;
            let uses = eps.iter().filter(|e| e["target_owner"] == target).count() as u32;
            w.id(&g.revision);
            w.u32(0);
            w.u8(0);
            w.u32(0);
            w.id(&g.ids[target]);
            w.u32(uses);
            w.u32(if slot + 1 == objects.len() {
                eps.len() as u32 + 1
            } else {
                0
            });
        }
        for e in eps {
            let slot = object_slot(owner, string(e, "target_owner")?)?;
            w.u32((slot as u32) << 16 | number(&e["interface_index"])?);
            w.u16(
                classes
                    .iter()
                    .position(|s| e["target_class"] == *s)
                    .unwrap() as u16,
            );
            w.u16(
                contexts
                    .iter()
                    .position(|s| e["context_guid"] == *s)
                    .unwrap() as u16,
            );
            w.u64(0);
            w.u32(0);
            w.u16(key(g, e)?);
        }
    }
    w.u32(0);
    for table in [&classes, &contexts] {
        w.u32(table.len() as u32);
        for id in table {
            w.id(&guid(id)?);
        }
    }
    Ok(w.0)
}
pub(super) fn streams(g: &Graph<'_>) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut streams = properties::streams(g)?;
    for owner in array(&g.profile.0, "owners")? {
        let name = string(owner, "name")?;
        let mut payloads = Vec::new();
        let mut bulk = Wire::default();
        let classes = array(owner, "classes")?;
        for rec in g.records(name)? {
            let payload = if rec["recipe"].is_object() {
                g.recipe(rec)?
            } else {
                match string(rec, "generation_contract")? {
                    "graphics-curves" | "graphics-entity" => geometry::custom(g, rec)?,
                    "brep-shell" | "source-carrier" | "result-carrier" => carrier::record(g, rec)?,
                    other => return Err(Error(format!("missing constructor {other}"))),
                }
            };
            let selector = classes
                .iter()
                .position(|c| c["guid"] == rec["class"])
                .ok_or_else(|| Error("record class missing from descriptor table".into()))?;
            if selector > 255 || payload.is_empty() || payload.len() > 1_000_000 {
                return Err(Error("record encoding budget exceeded".into()));
            }
            bulk.u32(0x100 | selector as u32);
            bulk.0.extend(&payload);
            bulk.u32(payload.len() as u32);
            bulk.u8(0);
            payloads.push(payload);
        }
        bulk.u32(u32::MAX);
        let index_roles = if name == "PmAppSegment" {
            Some(array(owner, "root_roles")?)
        } else {
            owner["bulk_index_roles"].as_array().map(Vec::as_slice)
        };
        bulk.u16(index_roles.map_or(0, |roles| roles.len()) as u16);
        if let Some(roles) = index_roles {
            for role in roles {
                bulk.u32(if let Some(role) = role.as_str() {
                    g.reference(name, role, "local_bare")?
                } else if role.is_null() {
                    0
                } else {
                    return Err(Error("invalid bulk index role".into()));
                });
            }
        }
        bulk.u32(0);
        let mut envelope = Wire::default();
        envelope.id(&guid("a42bc29e-11d4-01e8-6000-2db3ee29fbb0")?);
        envelope.u16(516);
        envelope.0.extend(compress(&bulk.0)?);
        let token = token(&g.ids[name]);
        streams.insert(format!("/RSeStorage/M{token}"), meta(g, owner, &payloads)?);
        streams.insert(format!("/RSeStorage/B{token}"), envelope.0);
    }
    streams.insert("/RSeStorage/RSeSegInfo".into(), registry(g)?);
    if streams.len() != 22 + if g.metadata.is_some() { 2 } else { 0 } {
        return Err(Error("stream census differs".into()));
    }
    Ok(streams)
}
