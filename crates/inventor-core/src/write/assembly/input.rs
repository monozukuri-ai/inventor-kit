//! Strict initial-state part binding. Sidecars are not trusted as identities.
use super::super::{array, number, string, Wire};
use super::*;
use crate::part::WireValue;
use crate::property::PropertyValue;
use serde_json::json;
use std::io::Cursor;

pub(super) fn inspect(bytes: &[u8], part: &PartDefinition) -> Result<(String, [f64; 6])> {
    let deny = || Error("IAM input is outside the initial single-solid IPT writer profile".into());
    if bytes.len() > 64 * 1024 * 1024 || part.document_id != part.database_revision_id {
        return Err(deny());
    }
    let target = TargetVersion::from_major((part.target_year - 1996) as u8)?;
    let limits = Limits::default();
    let doc = crate::read(bytes, "IAM-part-binding", &limits)?;
    let summary = &doc.summary;
    if summary.kind != "part" || !summary.diagnostics.is_empty() {
        return Err(deny());
    }
    let model = doc.model.as_ref().ok_or_else(deny)?;
    if model.bodies().count() != 1 {
        return Err(deny());
    }
    let databases = &summary.document.databases;
    if databases.len() != 1
        || databases[0].stream != "/RSeStorage/V1/RSeDb"
        || databases[0].schema != Some(31)
    {
        return Err(deny());
    }
    let mut cfb = ::cfb::CompoundFile::open(Cursor::new(bytes)).map_err(super::super::err)?;
    let db = crate::stream(&mut cfb, "/RSeStorage/V1/RSeDb", 1024)?;
    if db.len() != 56 || db[..16] != super::super::guid(&part.document_id)? {
        return Err(deny());
    }
    let sets: Vec<_> = summary
        .document
        .property_sets
        .iter()
        .filter(|s| s.stream == "/\u{5}Qz4dgm1gRjudbpksAayal4qdGf")
        .collect();
    if sets.len() != 1 || sets[0].status != "decoded" {
        return Err(deny());
    }
    let properties = &sets[0].properties;
    if properties.len() != 5
        || properties.iter().any(|p| p.pid == 18 || p.pid == 21)
        || properties
            .iter()
            .filter(|p| p.pid == 14 && p.value == Some(PropertyValue::Text { value: "1".into() }))
            .count()
            != 1
    {
        return Err(deny());
    }
    let inventory = crate::part::inspect(bytes, "IAM-part-profile", &limits)?;
    let expected = [
        "PmDCSegment",
        "PmGraphicsSegment",
        "PmBrowserSegment",
        "PmAppSegment",
        "NBNotebookSegment",
        "PmResultSegment",
        "PmBRepSegment",
        "DesignViewSegment",
    ];
    let names: BTreeSet<_> = inventory.segments.iter().map(|s| s.name.as_str()).collect();
    if names != expected.into_iter().collect()
        || inventory.segments.len() != 8
        || inventory.revisions.len() != target.protocols() + 1
        || inventory
            .revisions
            .last()
            .is_none_or(|r| r.id != part.document_id || r.flags != 0x841)
    {
        return Err(deny());
    }
    let dc = inventory
        .segments
        .iter()
        .find(|s| s.name == "PmDCSegment")
        .ok_or_else(deny)?;
    let roots: Vec<_> = dc
        .records
        .iter()
        .filter(|r| r.type_id == "90874d16-11d0-d1f8-0008-cabc0663dc09")
        .collect();
    if roots.len() != 1 {
        return Err(deny());
    }
    let name = roots[0]
        .fields
        .iter()
        .find_map(|f| {
            if f.name == "document_name" {
                if let WireValue::Utf16(s) = &f.value {
                    Some(s.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .ok_or_else(deny)?;
    validate_name(&name)?;
    let result = inventory
        .segments
        .iter()
        .find(|s| s.name == "PmResultSegment")
        .ok_or_else(deny)?;
    let roots: Vec<_> = result
        .records
        .iter()
        .filter(|r| r.type_id == "128aaf24-465e-df32-e3f0-509dc05b0cca")
        .collect();
    if roots.len() != 1 {
        return Err(deny());
    }
    let payload = &roots[0].payload.0;
    let profile = super::super::profile::Profile::load_for(target)?;
    let recipe = array(&profile.0, "records")?
        .iter()
        .find(|r| {
            r["owner"] == "PmResultSegment" && r["class"] == "128aaf24-465e-df32-e3f0-509dc05b0cca"
        })
        .ok_or_else(deny)?;
    let mut bounds = [0.; 6];
    let mut encoded = Wire::default();
    for f in array(&recipe["recipe"], "fields")? {
        let value = match string(f, "policy")? {
            "fixed" => f["value"].clone(),
            "assigned_key" => json!(1),
            "body-bounds" => {
                let index = number(&f["coordinate"])? as usize;
                let raw = payload
                    .get(encoded.0.len()..encoded.0.len() + 8)
                    .ok_or_else(deny)?;
                bounds[index] = f64::from_le_bytes(raw.try_into().unwrap());
                json!(bounds[index])
            }
            _ => return Err(deny()),
        };
        encoded.field(string(f, "kind")?, &value)?;
    }
    if encoded.0 != *payload
        || bounds.iter().any(|x| !x.is_finite() || x.abs() > 100_000.)
        || (0..3).any(|i| bounds[i] >= bounds[i + 3])
    {
        return Err(deny());
    }
    let carrier = summary.carrier.as_ref().ok_or_else(deny)?;
    if carrier.delta_state != 3
        || carrier.header_scale != Some(10.)
        || carrier.segment_major != target.major()
    {
        return Err(deny());
    }
    Ok((name, bounds))
}
