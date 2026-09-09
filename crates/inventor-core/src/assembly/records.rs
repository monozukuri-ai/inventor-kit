use super::{
    matrix::affine,
    ufrx::{peek, require},
    AssemblyDocument, Matrix, Occurrence,
};
use crate::{
    document::{Diagnostic, SourceSpan},
    property::guid,
    read::Reader,
    rse, stream, Error, Limits, Result, Summary,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Cursor,
};

const DC: [u8; 16] = [
    0x60, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0, 8, 0xca, 0xbc, 6, 0x63, 0xdc, 9,
];
const GR: [u8; 16] = [
    0xa2, 0x63, 0x71, 0xca, 0xd0, 0x11, 0xb2, 0xd3, 0, 8, 0xbf, 0xbb, 0x21, 0xed, 0xdc, 9,
];
const GR_B9: [u8; 16] = [
    7, 0xd0, 0xd0, 0xb9, 0xd4, 0x11, 0x2d, 0x5f, 0x60, 0, 0xf8, 0x83, 0x0e, 0x73, 0xfc, 0xb0,
];

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub segment_id: String,
    pub segment_major: u8,
    pub meta_state_words: [u32; 3],
    pub record_ordinal: usize,
    pub record_type: String,
    pub source: SourceSpan,
    pub compressed_source: SourceSpan,
}
#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub occurrence_id: u32,
    pub header_value: u32,
    pub header_id: u16,
    pub next_reference: u32,
    pub flags: u32,
    pub header_extension: u32,
    pub owner_reference: u32,
    pub node_index: u32,
    pub state: [i32; 2],
    pub ordinal_key: u32,
    pub related_metadata: Option<[u32; 2]>,
    pub related_references: Vec<u32>,
    pub child_reference: u32,
    pub evidence: Evidence,
}
#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    pub occurrence_id: u32,
    pub header_id: u16,
    pub owner_reference: u32,
    pub attribute_reference: u32,
    pub state: u8,
    pub transform_prefix: bool,
    pub encoding: [u16; 2],
    pub transform_cm: Matrix,
    pub branch: u8,
    pub graphics_state: u8,
    pub graphics_index: u32,
    pub object_reference: u32,
    pub suffix: SourceSpan,
    pub evidence: Evidence,
}

fn identity(data: &[u8], evidence: Evidence, max: usize) -> Result<Identity> {
    let mut r = Reader::new(data);
    let header_value = r.u32()?;
    let header_id = r.u16()?;
    let next_reference = r.u32()?;
    let flags = r.u32()?;
    let header_extension = r.u32()?;
    let owner_reference = r.u32()?;
    let node_index = r.u32()?;
    let state = [r.u32()? as i32, r.u32()? as i32];
    require(r.u32()?, 0x30000002)?;
    require(r.u32()?, 0)?;
    let ordinal_key = r.u32()?;
    require(r.u32()?, 0x30000002)?;
    let n = r.count(max.min(65536))?;
    let related_metadata = if n != 0 {
        Some([r.u32()?, r.u32()?])
    } else {
        None
    };
    let mut related_references = vec![];
    for _ in 0..n {
        related_references.push(r.u32()?);
    }
    let child_reference = r.u32()?;
    require(r.u16()? as u32, 0x200)?;
    let occurrence_id = r.u32()?;
    if r.utf16()? != "DCx" {
        return Err(Error("invalid occurrence identity label".into()));
    }
    require(r.u16()? as u32, 1)?;
    r.finish()?;
    Ok(Identity {
        occurrence_id,
        header_value,
        header_id,
        next_reference,
        flags,
        header_extension,
        owner_reference,
        node_index,
        state,
        ordinal_key,
        related_metadata,
        related_references,
        child_reference,
        evidence,
    })
}

pub(super) fn compact(r: &mut Reader<'_>) -> Result<(bool, [u16; 2], Matrix)> {
    let prefixed = peek(r)? == 0x203;
    if prefixed {
        r.u32()?;
    }
    let set = r.u16()?;
    let zero = r.u16()?;
    let mut m = [[0.; 4]; 4];
    for (i, x) in m.iter_mut().flatten().enumerate() {
        let bit = 1 << i;
        *x = match (set & bit != 0, zero & bit != 0) {
            (false, false) => f64::from_le_bytes(r.take(8)?.try_into().unwrap()),
            (true, false) => 1.,
            (false, true) => 0.,
            (true, true) => -1.,
        };
    }
    affine(&m)?;
    Ok((prefixed, [set, zero], m))
}

fn placement(data: &[u8], evidence: Evidence) -> Result<Placement> {
    let mut r = Reader::new(data);
    require(r.u32()?, 0)?;
    let header_id = r.u16()?;
    let owner_reference = r.u32()?;
    let attribute_reference = r.u32()?;
    let state = r.u8()?;
    let (transform_prefix, encoding, transform_cm) = compact(&mut r)?;
    let branch = r.u8()?;
    let graphics_state = r.u8()?;
    let occurrence_id = r.u32()?;
    if r.utf16()? != "GRx" {
        return Err(Error("invalid placement identity label".into()));
    }
    require(r.u16()? as u32, 1)?;
    let graphics_index = r.u32()?;
    let object_reference = r.u32()?;
    require(r.u32()?, occurrence_id)?;
    let mut suffix = evidence.source.clone();
    suffix.start_offset += r.pos;
    Ok(Placement {
        occurrence_id,
        header_id,
        owner_reference,
        attribute_reference,
        state,
        transform_prefix,
        encoding,
        transform_cm,
        branch,
        graphics_state,
        graphics_index,
        object_reference,
        suffix,
        evidence,
    })
}

pub(super) fn scan(
    file: &mut cfb::CompoundFile<Cursor<&[u8]>>,
    summary: &Summary,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Vec<Identity>, Vec<Placement>)> {
    if summary.document.databases.len() != 1 || summary.document.databases[0].schema != Some(31) {
        return Err(Error(
            "assembly requires one schema-31 database; binding is unresolved".into(),
        ));
    }
    let registry = rse::registry(&stream(
        file,
        "/RSeStorage/RSeSegInfo",
        limits.max_stream_bytes,
    )?)?;
    // Identify every M stream first: a duplicate ID cannot win by scan order.
    let mut owners = BTreeMap::<[u8; 16], Vec<(String, String, Vec<u8>)>>::new();
    let mut encoded = limits.max_file_bytes;
    for s in &summary.streams {
        let Some(token) = s
            .path
            .strip_prefix("/RSeStorage/M")
            .filter(|t| !t.is_empty() && !t.contains('/'))
        else {
            continue;
        };
        encoded = encoded
            .checked_sub(s.bytes as usize)
            .ok_or_else(|| Error("assembly metadata byte limit exceeded".into()))?;
        let bytes = stream(file, &s.path, limits.max_stream_bytes)?;
        match rse::meta_identity(&bytes) {
            Ok((id, name)) => owners
                .entry(id)
                .or_default()
                .push((token.into(), name, bytes)),
            Err(e) => diagnostics.push(Diagnostic::new(
                "assembly.meta_identity",
                "warning",
                e.to_string(),
                Some(SourceSpan::stream(
                    &summary.source_id,
                    &s.path,
                    0,
                    s.bytes as usize,
                )),
            )),
        }
    }
    let mut identities = vec![];
    let mut placements = vec![];
    let mut expanded_budget = limits.max_total_inflated_bytes;
    let mut record_budget = limits.max_records;
    for reg in registry
        .iter()
        .filter(|r| matches!(r.kind.as_str(), "AmDcSegmentType" | "AmGRxSegmentType"))
    {
        let result: Result<()> = (|| {
            require(reg.major as u32, 31)?;
            let matches = owners
                .get(&reg.id)
                .ok_or_else(|| Error("assembly segment has no metadata owner".into()))?;
            if matches.len() != 1 || registry.iter().filter(|r| r.id == reg.id).count() != 1 {
                return Err(Error("ambiguous assembly segment identity".into()));
            }
            let (token, name, bytes) = &matches[0];
            if name != &reg.name {
                return Err(Error("assembly registry/metadata name conflict".into()));
            }
            let meta = rse::meta(
                bytes,
                &Limits {
                    max_inflated_bytes: limits.max_inflated_bytes.min(expanded_budget),
                    max_records: record_budget,
                    ..limits.clone()
                },
            )?;
            expanded_budget -= meta.inflated_bytes;
            record_budget = record_budget
                .checked_sub(meta.blocks.len())
                .ok_or_else(|| Error("assembly record budget exceeded".into()))?;
            let path = format!("/RSeStorage/B{token}");
            let bytes = stream(file, &path, limits.max_stream_bytes)?;
            encoded = encoded
                .checked_sub(bytes.len())
                .ok_or_else(|| Error("assembly encoded byte limit exceeded".into()))?;
            let compressed = bytes
                .get(18..)
                .ok_or_else(|| Error("truncated assembly B prefix".into()))?;
            let (expanded, _) =
                rse::inflate(compressed, limits.max_inflated_bytes.min(expanded_budget))?;
            expanded_budget -= expanded.len();
            for record in rse::records(&expanded, &meta, reg.major)? {
                let is_dc = reg.kind == "AmDcSegmentType" && record.kind == DC;
                let is_gr = reg.kind == "AmGRxSegmentType" && matches!(record.kind, GR | GR_B9);
                if !is_dc && !is_gr {
                    continue;
                }
                let mut source =
                    SourceSpan::stream(&summary.source_id, &path, record.start, record.end);
                source.byte_domain = "inflated_stream";
                let evidence = Evidence {
                    segment_id: guid(&reg.id),
                    segment_major: reg.major,
                    meta_state_words: meta.state_words,
                    record_ordinal: record.ordinal,
                    record_type: guid(&record.kind),
                    source: source.clone(),
                    compressed_source: SourceSpan::stream(
                        &summary.source_id,
                        &path,
                        18,
                        bytes.len(),
                    ),
                };
                let data = &expanded[record.start..record.end];
                let decoded = if is_dc {
                    identity(data, evidence, limits.max_records).map(|r| identities.push(r))
                } else {
                    placement(data, evidence).map(|r| placements.push(r))
                };
                if let Err(e) = decoded {
                    let mut d = Diagnostic::new(
                        "assembly.record_unsupported",
                        "warning",
                        e.to_string(),
                        Some(source),
                    );
                    d.record = Some(record.ordinal);
                    diagnostics.push(d);
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            diagnostics.push(Diagnostic::new(
                "assembly.segment_unavailable",
                "warning",
                e.to_string(),
                Some(SourceSpan::stream(
                    &summary.source_id,
                    "/RSeStorage/RSeSegInfo",
                    reg.start_offset,
                    reg.end_offset,
                )),
            ));
        }
    }
    Ok((identities, placements))
}

fn unique<T>(items: &[T], key: impl Fn(&T) -> u32) -> HashMap<u32, &T> {
    let mut result = HashMap::new();
    let mut duplicates = HashSet::new();
    for item in items {
        let k = key(item);
        if result.insert(k, item).is_some() {
            duplicates.insert(k);
        }
    }
    for k in duplicates {
        result.remove(&k);
    }
    result
}

pub(super) fn join(
    doc: &mut AssemblyDocument,
    identities: Vec<Identity>,
    placements: Vec<Placement>,
) {
    let refs = unique(&doc.ufrx.references, |r| r.reference_id);
    let occurrences = unique(&doc.ufrx.occurrences, |o| o.occurrence_id);
    let dc = unique(&identities, |o| o.occurrence_id);
    let gr = unique(&placements, |p| p.occurrence_id);
    for source in &doc.ufrx.occurrences {
        let reference = refs.get(&source.reference_id);
        let identity = dc.get(&source.occurrence_id).copied();
        let placement = gr.get(&source.occurrence_id).copied();
        let status = if !occurrences.contains_key(&source.occurrence_id) {
            "ambiguous_occurrence"
        } else if reference.is_none() {
            "unresolved_reference"
        } else if identity.is_none() {
            "unresolved_identity"
        } else if placement.is_none() {
            "unresolved_placement"
        } else {
            "resolved"
        };
        let transform = if status == "resolved" {
            placement.and_then(|p| {
                let mut m = p.transform_cm;
                for row in m.iter_mut().take(3) {
                    row[3] *= 10.;
                }
                affine(&m).ok().map(|_| m)
            })
        } else {
            None
        };
        doc.occurrences.push(Occurrence {
            occurrence_id: source.occurrence_id,
            reference_id: source.reference_id,
            name: source.title.clone(),
            suppressed: reference.map(|r| r.state[0] & 0x2000 != 0),
            visible: None,
            substitute: None,
            status: if status == "resolved" && transform.is_none() {
                "invalid_transform"
            } else {
                status
            }
            .into(),
            local_transform_mm: transform,
            source: source.source.clone(),
            identity: identity.cloned(),
            placement: placement.cloned(),
        });
    }
    let mut issues = vec![];
    for r in &doc.ufrx.references {
        if !refs.contains_key(&r.reference_id)
            || doc
                .ufrx
                .occurrences
                .iter()
                .filter(|o| o.reference_id == r.reference_id)
                .count()
                != r.occurrence_count as usize
        {
            issues.push((
                "assembly.reference_count",
                format!(
                    "reference {} is duplicated or occurrence count disagrees",
                    r.reference_id
                ),
                Some(r.source.clone()),
            ));
        }
    }
    for (kind, ids) in [
        (
            "AmDc",
            identities
                .iter()
                .map(|r| r.occurrence_id)
                .collect::<Vec<_>>(),
        ),
        (
            "AmGraphics",
            placements.iter().map(|r| r.occurrence_id).collect(),
        ),
    ] {
        for id in ids {
            if !occurrences.contains_key(&id) {
                issues.push((
                    "assembly.unjoined_record",
                    format!("{kind} occurrence {id} has no unique UFRx owner"),
                    None,
                ));
            }
        }
    }
    for (code, message, source) in issues {
        doc.issue(code, message, source);
    }
}
