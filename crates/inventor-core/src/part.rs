//! Bounded part wire inventory for native-writer research. No writer or history evaluator.
//! The sole admitted envelope is the independently observed M0 schema31/major31 band.
use crate::{
    document::{Diagnostic, SourceSpan},
    property::{guid, Binary},
    read::Reader,
    rse, stream, Error, Limits, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};
mod records;

#[derive(Debug, Serialize)]
pub struct PartInventory {
    pub api_version: u32,
    pub source_id: String,
    pub source_sha256: String,
    pub status: &'static str,
    pub profile: Option<&'static str>,
    pub write_profile_qualified: bool,
    pub cfb_header: Binary,
    pub root_clsid: String,
    pub hierarchy: Vec<ContainerEntry>,
    pub database_fields: Vec<WireField>,
    pub revisions: Vec<Revision>,
    pub registry_tail: Vec<WireField>,
    pub segments: Vec<PartSegment>,
    pub unclaimed_streams: Vec<SourceSpan>,
    pub diagnostics: Vec<Diagnostic>,
    pub expanded_bytes: usize,
    pub work_items: usize,
}
#[derive(Debug, Serialize)]
pub struct ContainerEntry {
    pub path: String,
    pub kind: &'static str,
    pub clsid: String,
    pub bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct PartSegment {
    pub name: String,
    pub kind: String,
    pub id: String,
    pub major: u8,
    pub stream_token_observed: Option<String>,
    pub stream_token_matches_id: Option<bool>,
    pub registry_source: SourceSpan,
    pub registry_fields: Vec<WireField>,
    pub status: &'static str,
    pub meta_fields: Vec<WireField>,
    pub meta_sections: Vec<Section>,
    pub type_descriptors: Vec<TypeDescriptor>,
    pub meta_codec: Option<&'static str>,
    pub bulk_codec: Option<&'static str>,
    pub bulk_envelope: Vec<WireField>,
    pub records: Vec<PartRecord>,
    pub bulk_suffix: Option<WireField>,
    pub bulk_suffix_fields: Vec<WireField>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Serialize)]
pub struct WireField {
    pub name: String,
    pub value: WireValue,
    pub source: SourceSpan,
    /// Encoding is known; the semantic role is not promoted by this inventory.
    pub interpretation: &'static str,
}
#[derive(Debug, Serialize)]
#[serde(tag = "encoding", content = "raw")]
pub enum WireValue {
    #[serde(rename = "bytes")]
    Bytes(Binary),
    #[serde(rename = "u8")]
    U8(u8),
    #[serde(rename = "u16le")]
    U16(u16),
    #[serde(rename = "u32le")]
    U32(u32),
    #[serde(rename = "u64le")]
    U64(u64),
    #[serde(rename = "f64le")]
    F64(f64),
    #[serde(rename = "guid_mixed_endian")]
    Guid(String),
    #[serde(rename = "utf16le_counted")]
    Utf16(String),
    #[serde(rename = "utf8_counted")]
    Utf8(String),
}
#[derive(Debug, Serialize)]
pub struct Revision {
    pub id: String,
    pub flags: u32,
    pub kind: u16,
    pub payload: Option<Binary>,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct Section {
    pub number: u8,
    /// Section 5's word is a discriminator, not an item count.
    pub count_or_discriminator: usize,
    pub bytes: Binary,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct TypeDescriptor {
    pub index: usize,
    pub id: String,
    pub field_pairs: [(u16, u32); 2],
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct PartRecord {
    /// Ordinal in the complete block table, including unstored blocks.
    pub ordinal: usize,
    pub selector_word: u32,
    pub type_index: usize,
    pub type_id: String,
    pub block_word: u32,
    pub source: SourceSpan,
    pub frame_source: SourceSpan,
    pub payload_sha256: String,
    pub payload: Binary,
    pub trailer: Binary,
    pub named_references: Vec<WireField>,
    pub observation_status: &'static str,
    pub fields: Vec<WireField>,
    pub references: Vec<RecordReference>,
    pub kernel: Option<KernelObservation>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Serialize)]
pub struct RecordReference {
    pub field: String,
    pub raw: u32,
    pub qualifier_bit: bool,
    pub target_ordinal: Option<usize>,
    pub target_type_id: Option<String>,
    pub status: &'static str,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct KernelObservation {
    pub sha256: String,
    pub source: SourceSpan,
    pub save_version: u32,
    pub integer_width: u8,
    pub scale: f64,
    pub resabs: f64,
    pub resnor: f64,
    pub product_id: String,
    pub modeler_version: String,
    pub creation_date: String,
    pub active_table_source: SourceSpan,
    pub active_table_sha256: String,
    pub type_counts: BTreeMap<String, usize>,
    pub bodies: Vec<KernelBody>,
    pub entity_identifiers: Vec<KernelEntityIdentity>,
    pub custom_attributes: Vec<KernelAttribute>,
    pub history: Option<SourceSpan>,
}
#[derive(Debug, Serialize)]
pub struct KernelBody {
    pub ordinal: usize,
    pub entity_id: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct KernelEntityIdentity {
    pub ordinal: usize,
    pub type_name: String,
    pub entity_id: Option<String>,
    pub source: SourceSpan,
}
#[derive(Debug, Serialize)]
pub struct KernelAttribute {
    pub ordinal: usize,
    pub entity_id: Option<String>,
    pub source: SourceSpan,
    pub bytes: Binary,
    pub strings: Vec<String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn span(id: &str, path: &str, start: usize, end: usize, inflated: bool) -> SourceSpan {
    let mut out = SourceSpan::stream(id, path, start, end);
    if inflated {
        out.byte_domain = "inflated_stream";
    }
    out
}
fn field(name: impl Into<String>, value: WireValue, source: SourceSpan) -> WireField {
    WireField {
        name: name.into(),
        value,
        source,
        interpretation: "wire_observation_not_write_semantics",
    }
}
fn warning(code: &str, error: impl Into<String>, source: Option<SourceSpan>) -> Diagnostic {
    Diagnostic::new(code, "warning", error, source)
}

fn database(bytes: &[u8], id: &str, path: &str) -> Result<Vec<WireField>> {
    let mut r = Reader::new(bytes);
    let mut out = vec![field(
        "database_id",
        WireValue::Guid(guid(&r.id()?)),
        span(id, path, 0, 16, false),
    )];
    let schema = r.u32()?;
    out.push(field(
        "schema",
        WireValue::U32(schema),
        span(id, path, 16, 20, false),
    ));
    if schema != 31 {
        return Err(Error("unsupported part database schema".into()));
    }
    for name in ["creation", "save"] {
        let start = r.pos;
        out.push(field(
            format!("{name}_version_tuple"),
            WireValue::Bytes(Binary(r.take(8)?.to_vec())),
            span(id, path, start, r.pos, false),
        ));
        let start = r.pos;
        out.push(field(
            format!("{name}_filetime"),
            WireValue::U64(r.u64()?),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    out.push(field(
        "note",
        WireValue::Utf16(r.utf16()?),
        span(id, path, start, r.pos, false),
    ));
    r.finish()?;
    Ok(out)
}

fn revisions(bytes: &[u8], id: &str, path: &str, work: &mut usize) -> Result<Vec<Revision>> {
    let mut r = Reader::new(bytes);
    if r.u32()? != 3 {
        return Err(Error("unsupported part revision table version".into()));
    }
    let count = r.count(1_000_000)?;
    rse::charge(work, count)?;
    let mut out = Vec::new();
    let mut identifiers = BTreeSet::new();
    for _ in 0..count {
        let start = r.pos;
        let identity = guid(&r.id()?);
        if !identifiers.insert(identity.clone()) {
            return Err(Error("duplicate part revision identity".into()));
        }
        let flags = r.u32()?;
        let kind = r.u16()?;
        let payload = if kind == u16::MAX {
            let start = r.pos;
            let length = match r.u8()? {
                0 => 16,
                1 => 8,
                _ => return Err(Error("unsupported revision payload selector".into())),
            };
            r.skip(length)?;
            Some(Binary(bytes[start..r.pos].to_vec()))
        } else {
            None
        };
        out.push(Revision {
            id: identity,
            flags,
            kind,
            payload,
            source: span(id, path, start, r.pos, false),
        });
    }
    r.finish()?;
    Ok(out)
}

fn registry_fields(
    reg: &rse::Segment,
    bytes: &[u8],
    id: &str,
    path: &str,
) -> Result<Vec<WireField>> {
    let mut r = Reader::new(bytes);
    r.pos = reg.start_offset;
    let mut out = Vec::new();
    let start = r.pos;
    out.push(field(
        "display_name",
        WireValue::Utf16(r.utf16()?),
        span(id, path, start, r.pos, false),
    ));
    for (name, value) in [("segment_id", reg.id), ("revision_id", reg.revision_id)] {
        let start = r.pos;
        r.skip(16)?;
        out.push(field(
            name,
            WireValue::Guid(guid(&value)),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    r.skip(4)?;
    out.push(field(
        "state",
        WireValue::U32(reg.state),
        span(id, path, start, r.pos, false),
    ));
    let start = r.pos;
    let count = r.u32()?;
    out.push(field(
        "object_count",
        WireValue::U32(count),
        span(id, path, start, r.pos, false),
    ));
    for (index, value) in reg.state_words.iter().enumerate() {
        let start = r.pos;
        r.skip(4)?;
        out.push(field(
            format!("state_word_{index}"),
            WireValue::U32(*value),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    out.push(field(
        "type_name",
        WireValue::Utf16(r.utf16()?),
        span(id, path, start, r.pos, false),
    ));
    for (index, value) in reg.type_words.iter().enumerate() {
        let start = r.pos;
        r.skip(4)?;
        out.push(field(
            format!("type_word_{index}"),
            WireValue::U32(*value),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    r.skip(8)?;
    out.push(field(
        "version_tuple",
        WireValue::Bytes(Binary(reg.version.to_vec())),
        span(id, path, start, r.pos, false),
    ));
    let start = r.pos;
    r.skip(4)?;
    out.push(field(
        "version_state",
        WireValue::U32(reg.version_state),
        span(id, path, start, r.pos, false),
    ));
    for (index, object) in reg.objects.iter().enumerate() {
        let start = object.start;
        let value = [
            object.revision_id.as_slice(),
            object.flags.as_slice(),
            object.segment_id.as_slice(),
            &object.state.to_le_bytes(),
            &(object.node_count as u32).to_le_bytes(),
        ]
        .concat();
        out.push(field(
            format!("object_{index}"),
            WireValue::Bytes(Binary(value)),
            span(id, path, start, object.end, false),
        ));
    }
    for (index, node) in reg.nodes.iter().enumerate() {
        let value = [
            node.index.to_le_bytes().as_slice(),
            node.list_index.to_le_bytes().as_slice(),
            node.id.as_slice(),
        ]
        .concat();
        out.push(field(
            format!("node_{index}"),
            WireValue::Bytes(Binary(value)),
            span(id, path, node.start, node.end, false),
        ));
    }
    Ok(out)
}

fn meta_fields(meta: &rse::Meta, bytes: &[u8], id: &str, path: &str) -> Result<Vec<WireField>> {
    let mut r = Reader::new(bytes);
    let mut out = Vec::new();
    let start = r.pos;
    out.push(field(
        "marker",
        WireValue::Utf8(r.text()?),
        span(id, path, start, r.pos, false),
    ));
    let start = r.pos;
    out.push(field(
        "version",
        WireValue::U16(r.u16()?),
        span(id, path, start, r.pos, false),
    ));
    for (index, value) in meta.header_words.iter().enumerate() {
        let start = r.pos;
        r.skip(2)?;
        out.push(field(
            format!("header_word_{index}"),
            WireValue::U16(*value),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    out.push(field(
        "display_name",
        WireValue::Utf16(r.utf16()?),
        span(id, path, start, r.pos, false),
    ));
    let start = r.pos;
    r.skip(16)?;
    out.push(field(
        "segment_id",
        WireValue::Guid(guid(&meta.id)),
        span(id, path, start, r.pos, false),
    ));
    for (index, value) in meta.state_words.iter().enumerate() {
        let start = r.pos;
        r.skip(4)?;
        out.push(field(
            format!("state_word_{index}"),
            WireValue::U32(*value),
            span(id, path, start, r.pos, false),
        ));
    }
    for (index, value) in meta.timestamps.iter().enumerate() {
        let start = r.pos;
        let decoded = r.text()?;
        if &decoded != value {
            return Err(Error("Meta timestamp observation mismatch".into()));
        }
        out.push(field(
            format!("timestamp_{index}"),
            WireValue::Utf8(decoded),
            span(id, path, start, r.pos, false),
        ));
    }
    let start = r.pos;
    r.skip(1)?;
    out.push(field(
        "body_form",
        WireValue::U8(meta.body_form),
        span(id, path, start, r.pos, false),
    ));
    if r.pos != meta.compressed_offset {
        return Err(Error("Meta header span mismatch".into()));
    }
    for (index, value) in meta.body_header.iter().enumerate() {
        out.push(field(
            format!("body_header_word_{index}"),
            WireValue::U16(*value),
            span(id, path, index * 2, index * 2 + 2, true),
        ));
    }
    out.push(field(
        "terminal_id",
        WireValue::Guid(guid(&meta.terminal_id)),
        span(
            id,
            path,
            meta.inflated_bytes - 16,
            meta.inflated_bytes,
            true,
        ),
    ));
    Ok(out)
}

// Observed major31 post-terminal arrays. Entries remain wire observations:
// their ordering/role must not be promoted to generated document semantics.
fn bulk_suffix_fields(
    bytes: &[u8],
    source: &SourceSpan,
    work: &mut usize,
) -> Result<Vec<WireField>> {
    let mut r = Reader::new(bytes);
    let mut fields = Vec::new();
    let mut source_field = |name: String, value: WireValue, start: usize, end: usize| {
        let mut span = source.clone();
        span.start_offset += start;
        span.end_offset = source.start_offset + end;
        fields.push(field(name, value, span));
    };
    let n = r.u16()? as usize;
    rse::charge(work, n)?;
    source_field("index_count".into(), WireValue::U16(n as u16), 0, r.pos);
    for index in 0..n {
        let start = r.pos;
        let value = r.u32()?;
        source_field(
            format!("index_{index}"),
            WireValue::U32(value),
            start,
            r.pos,
        );
    }
    let start = r.pos;
    let n = r.count(*work)?;
    rse::charge(work, n)?;
    source_field("pair_count".into(), WireValue::U32(n as u32), start, r.pos);
    for index in 0..n {
        let start = r.pos;
        let value = r.u32()?;
        source_field(
            format!("pair_{index}_word"),
            WireValue::U32(value),
            start,
            r.pos,
        );
        let start = r.pos;
        let value = r.u16()?;
        source_field(
            format!("pair_{index}_small"),
            WireValue::U16(value),
            start,
            r.pos,
        );
    }
    r.finish()?;
    Ok(fields)
}

/// Inspect the hash-bound M0 part envelope without selecting a feature graph,
/// effective Model State, required streams, or a writable native profile.
pub fn inspect(data: &[u8], source_id: &str, limits: &Limits) -> Result<PartInventory> {
    let summary = crate::inspect(data, source_id, limits)?.summary;
    let mut out = PartInventory {
        api_version: 1,
        source_id: source_id.into(),
        source_sha256: digest(data),
        status: "identity_only",
        profile: None,
        write_profile_qualified: false,
        cfb_header: Binary(data[..128].to_vec()),
        root_clsid: summary.root_clsid.clone(),
        hierarchy: vec![],
        database_fields: vec![],
        revisions: vec![],
        registry_tail: vec![],
        segments: vec![],
        unclaimed_streams: vec![],
        diagnostics: vec![],
        expanded_bytes: 0,
        work_items: 0,
    };
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| Error(e.to_string()))?;
    for entry in file.walk() {
        if out.hierarchy.len() >= limits.max_streams {
            return Err(Error("part hierarchy entry limit exceeded".into()));
        }
        out.hierarchy.push(ContainerEntry {
            path: entry.path().to_string_lossy().replace('\\', "/"),
            kind: if entry.is_stream() {
                "stream"
            } else {
                "storage"
            },
            clsid: entry.clsid().to_string(),
            bytes: entry.len(),
        });
    }
    let dbs = &summary.document.databases;
    if summary.kind != "part"
        || summary.root_clsid != "4d29b490-49b2-11d0-93c3-7e0706000000"
        || u16::from_le_bytes([data[26], data[27]]) != 3
        || dbs.len() != 1
        || dbs[0].schema != Some(31)
        || dbs[0].status != "decoded"
    {
        out.diagnostics.push(warning(
            "part.profile_unavailable",
            "Unique CFB3/schema31 part database required",
            None,
        ));
        return Ok(out);
    }
    let mut work = limits.max_records;
    let mut expanded = limits.max_total_inflated_bytes;
    let result: Result<()> = (|| {
        let db = &dbs[0].stream;
        out.database_fields = database(
            &stream(&mut file, db, limits.max_stream_bytes)?,
            source_id,
            db,
        )?;
        let rpath = "/RSeStorage/RSeDbRevisionInfo";
        out.revisions = revisions(
            &stream(&mut file, rpath, limits.max_stream_bytes)?,
            source_id,
            rpath,
            &mut work,
        )?;
        let path = "/RSeStorage/RSeSegInfo";
        let registry_bytes = stream(&mut file, path, limits.max_stream_bytes)?;
        let registry = rse::registry_table_budgeted(&registry_bytes, &mut work, true)?;
        out.registry_tail.push(field(
            "tail_word",
            WireValue::U32(registry.tail_word),
            span(
                source_id,
                path,
                registry.tail_offset,
                registry.tail_offset + 4,
                false,
            ),
        ));
        let mut offset = registry.tail_offset + 4;
        for (index, ids) in registry.identifier_lists.iter().enumerate() {
            offset += 4;
            for (slot, value) in ids.iter().enumerate() {
                out.registry_tail.push(field(
                    format!("identifier_list_{index}_{slot}"),
                    WireValue::Guid(guid(value)),
                    span(source_id, path, offset, offset + 16, false),
                ));
                offset += 16;
            }
        }
        let mut registry_ids = BTreeMap::<[u8; 16], usize>::new();
        for reg in &registry.segments {
            *registry_ids.entry(reg.id).or_default() += 1;
        }
        let sizes: BTreeMap<_, _> = summary
            .streams
            .iter()
            .map(|s| (s.path.clone(), s.bytes as usize))
            .collect();
        let mut owners = BTreeMap::<[u8; 16], Vec<(String, Vec<u8>, String)>>::new();
        for s in summary.streams.iter().filter(|s| {
            s.path
                .strip_prefix("/RSeStorage/")
                .is_some_and(|name| !name.contains('/') && name.starts_with('M') && name.len() > 1)
        }) {
            rse::charge(&mut work, 1)?;
            let bytes = stream(&mut file, &s.path, limits.max_stream_bytes)?;
            let (identity, name) = rse::meta_identity(&bytes)?;
            owners
                .entry(identity)
                .or_default()
                .push((s.path.clone(), bytes, name));
        }
        let mut claimed = BTreeSet::from([db.clone(), path.to_owned(), rpath.to_owned()]);
        for reg in registry.segments {
            let mut segment = PartSegment {
                name: reg.name.clone(),
                kind: reg.kind.clone(),
                id: guid(&reg.id),
                major: reg.major,
                stream_token_observed: None,
                stream_token_matches_id: None,
                registry_source: span(source_id, path, reg.start_offset, reg.end_offset, false),
                registry_fields: registry_fields(&reg, &registry_bytes, source_id, path)?,
                status: "identity_only",
                meta_fields: vec![],
                meta_sections: vec![],
                type_descriptors: vec![],
                meta_codec: None,
                bulk_codec: None,
                bulk_envelope: vec![],
                records: vec![],
                bulk_suffix: None,
                bulk_suffix_fields: vec![],
                diagnostics: vec![],
            };
            let framed: Result<()> = (|| {
                if reg.major != 31
                    || !matches!(
                        reg.kind.as_str(),
                        "PmGRxSegmentType"
                            | "PmBrepSegmentType"
                            | "PmDcSegmentType"
                            | "PmBRxSegmentType"
                            | "PmAppSegmentType"
                            | "NotebookSegmentType"
                            | "PmResultSegmentType"
                            | "FWxDesignViewManagerType"
                    )
                {
                    return Err(Error(
                        "Unsupported part segment profile; no version fallback".into(),
                    ));
                }
                if registry_ids[&reg.id] != 1 {
                    return Err(Error("duplicate registry identity".into()));
                }
                let mut matches = owners.remove(&reg.id).unwrap_or_default();
                if matches.len() != 1 {
                    return Err(Error("Meta owner is missing or ambiguous".into()));
                }
                let (mpath, encoded, name) = matches.pop().unwrap();
                if name != reg.name {
                    return Err(Error("Meta/registry display names disagree".into()));
                }
                let bpath = format!(
                    "/RSeStorage/B{}",
                    mpath.strip_prefix("/RSeStorage/M").unwrap()
                );
                let token = mpath.strip_prefix("/RSeStorage/M").unwrap();
                segment.stream_token_matches_id =
                    Some(token == records::observed_stream_token(&reg.id));
                segment.stream_token_observed = Some(token.into());
                if !sizes.contains_key(&bpath) {
                    return Err(Error("Missing paired B stream".into()));
                }
                let meta = rse::meta_layout_budgeted(
                    &encoded,
                    limits,
                    &mut expanded,
                    &mut work,
                    rse::MetaLayout::PART31,
                )?;
                if meta.codec != "zstd" || meta.body_form != 2 {
                    return Err(Error("major31 Meta body form/codec mismatch".into()));
                }
                segment.meta_codec = Some(meta.codec);
                segment.meta_fields = meta_fields(&meta, &encoded, source_id, &mpath)?;
                segment.type_descriptors = meta
                    .types
                    .iter()
                    .enumerate()
                    .map(|(index, value)| TypeDescriptor {
                        index,
                        id: guid(value),
                        field_pairs: meta.type_fields[index],
                        source: span(
                            source_id,
                            &mpath,
                            meta.type_table_offset + index * 28,
                            meta.type_table_offset + (index + 1) * 28,
                            true,
                        ),
                    })
                    .collect();
                segment.meta_sections = meta
                    .sections
                    .iter()
                    .map(|section| Section {
                        number: section.number,
                        count_or_discriminator: section.count,
                        bytes: Binary(section.bytes.clone()),
                        source: span(
                            source_id,
                            &mpath,
                            section.offset,
                            section.offset + section.bytes.len(),
                            true,
                        ),
                    })
                    .collect();
                let encoded_bulk = stream(&mut file, &bpath, limits.max_stream_bytes)?;
                records::check_bulk_envelope(&encoded_bulk)?;
                segment.bulk_envelope.push(field(
                    "prefix",
                    WireValue::Bytes(Binary(encoded_bulk[..16].to_vec())),
                    span(source_id, &bpath, 0, 16, false),
                ));
                segment.bulk_envelope.push(field(
                    "form",
                    WireValue::U16(u16::from_le_bytes(encoded_bulk[16..18].try_into().unwrap())),
                    span(source_id, &bpath, 16, 18, false),
                ));
                let (body, codec) = rse::inflate_budgeted(
                    &encoded_bulk[18..],
                    limits.max_inflated_bytes,
                    &mut expanded,
                )?;
                segment.bulk_codec = Some(codec);
                let table = rse::record_table(&body, &meta, reg.major, &mut work, true)?;
                for record in table.records {
                    let payload = &body[record.start..record.end];
                    segment.records.push(PartRecord {
                        ordinal: record.ordinal,
                        selector_word: record.selector,
                        type_index: record.selector as u8 as usize,
                        type_id: guid(&record.kind),
                        block_word: meta.blocks[record.ordinal],
                        source: span(source_id, &bpath, record.start, record.end, true),
                        frame_source: span(
                            source_id,
                            &bpath,
                            record.start - 4,
                            record.frame_end,
                            true,
                        ),
                        payload_sha256: digest(payload),
                        payload: Binary(payload.to_vec()),
                        trailer: Binary(body[record.end..record.frame_end].to_vec()),
                        named_references: record
                            .references
                            .into_iter()
                            .map(|reference| {
                                field(
                                    reference.name,
                                    WireValue::U32(reference.value),
                                    span(source_id, &bpath, reference.start, reference.end, true),
                                )
                            })
                            .collect(),
                        observation_status: "unsupported",
                        fields: vec![],
                        references: vec![],
                        kernel: None,
                        diagnostics: vec![],
                    });
                }
                records::observe(&mut segment, limits, &mut work)?;
                segment.bulk_suffix = Some(field(
                    "suffix_after_terminal",
                    WireValue::Bytes(Binary(body[table.opaque_start..].to_vec())),
                    span(source_id, &bpath, table.opaque_start, body.len(), true),
                ));
                segment.bulk_suffix_fields = bulk_suffix_fields(
                    &body[table.opaque_start..],
                    &span(source_id, &bpath, table.opaque_start, body.len(), true),
                    &mut work,
                )?;
                claimed.insert(mpath);
                claimed.insert(bpath);
                segment.status = "framed";
                Ok(())
            })();
            if let Err(error) = framed {
                segment.status = "unavailable";
                segment.diagnostics.push(warning(
                    "part.segment_unavailable",
                    error.to_string(),
                    Some(segment.registry_source.clone()),
                ));
            }
            out.segments.push(segment);
        }
        for s in &summary.streams {
            if !claimed.contains(&s.path) {
                out.unclaimed_streams
                    .push(span(source_id, &s.path, 0, s.bytes as usize, false));
            }
        }
        Ok(())
    })();
    out.expanded_bytes = limits.max_total_inflated_bytes - expanded;
    out.work_items = limits.max_records - work;
    if let Err(error) = result {
        out.diagnostics.push(warning(
            "part.inventory_unavailable",
            error.to_string(),
            None,
        ));
    }
    if !out.segments.is_empty() {
        out.status =
            if out.diagnostics.is_empty() && out.segments.iter().all(|s| s.status == "framed") {
                "framed"
            } else {
                "partial"
            };
        out.profile = Some("ipt-cfb3-rse31-meta8-major31-framing-v1");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postterminal_arrays_retain_exact_fields_and_source_offsets() {
        let mut bytes = 2u16.to_le_bytes().to_vec();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(294u16.to_le_bytes());
        let source = span("fixture", "/B", 100, 120, true);
        let mut work = 3;
        let fields = bulk_suffix_fields(&bytes, &source, &mut work).unwrap();
        assert_eq!(fields.len(), 6);
        assert_eq!(work, 0);
        assert_eq!(fields[5].source.start_offset, 118);
        assert_eq!(fields[5].source.end_offset, 120);
        assert_eq!(fields[5].source.byte_domain, "inflated_stream");
        assert!(matches!(fields[5].value, WireValue::U16(294)));
    }
    #[test]
    fn postterminal_arrays_reject_missing_tail_truncation_and_extra_bytes() {
        let mut bytes = 1u16.to_le_bytes().to_vec();
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        let source = span("fixture", "/B", 0, 10, true);
        for end in 0..bytes.len() {
            assert!(bulk_suffix_fields(&bytes[..end], &source, &mut 10).is_err());
        }
        assert!(bulk_suffix_fields(&bytes, &source, &mut 10).is_ok());
        bytes.push(0);
        assert!(bulk_suffix_fields(&bytes, &source, &mut 10).is_err());
    }
    #[test]
    fn postterminal_arrays_share_the_work_budget_and_bound_counts() {
        let source = span("fixture", "/B", 0, 10, true);
        let mut bytes = 2u16.to_le_bytes().to_vec();
        bytes.extend([0; 8]);
        bytes.extend(0u32.to_le_bytes());
        assert!(bulk_suffix_fields(&bytes, &source, &mut 1).is_err());
        let mut bytes = 0u16.to_le_bytes().to_vec();
        bytes.extend(u32::MAX.to_le_bytes());
        assert!(bulk_suffix_fields(&bytes, &source, &mut 100).is_err());
    }
    #[test]
    fn hierarchy_storage_entries_spend_the_same_entry_ceiling_as_streams() {
        let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        cfb.set_storage_clsid("/", "4d29b490-49b2-11d0-93c3-7e0706000000".parse().unwrap())
            .unwrap();
        for i in 0..4 {
            cfb.create_storage(format!("/storage{i}")).unwrap();
        }
        let data = cfb.into_inner().into_inner();
        let limits = Limits {
            max_streams: 3,
            ..Default::default()
        };
        assert!(inspect(&data, "storage-budget", &limits)
            .err()
            .unwrap()
            .0
            .contains("hierarchy"));
        assert!(
            !inspect(&data, "unqualified", &Limits::default())
                .unwrap()
                .write_profile_qualified
        );
    }
    #[test]
    fn database_keeps_identity_time_and_note_and_rejects_extra_or_future_bytes() {
        let mut b = vec![1; 16];
        b.extend(31u32.to_le_bytes());
        b.extend([0; 32]);
        b.extend(0u32.to_le_bytes());
        let fields = database(&b, "source", "/RSeStorage/V1/RSeDb").unwrap();
        assert_eq!(fields.len(), 7);
        assert!(
            matches!(&fields[0].value, WireValue::Guid(s) if s == "01010101-0101-0101-0101-010101010101")
        );
        assert_eq!(fields[3].source.start_offset, 28);
        for end in 0..b.len() {
            assert!(database(&b[..end], "s", "/db").is_err());
        }
        let mut future = b.clone();
        future[16..20].copy_from_slice(&32u32.to_le_bytes());
        assert!(database(&future, "s", "/db").is_err());
        b.push(0);
        assert!(database(&b, "s", "/db").is_err());
    }
    #[test]
    fn revision_duplicates_unknown_payloads_and_budgets_are_not_discarded() {
        let mut b = 3u32.to_le_bytes().to_vec();
        b.extend(1u32.to_le_bytes());
        b.extend([9; 16]);
        b.extend(0x1234u32.to_le_bytes());
        b.extend(u16::MAX.to_le_bytes());
        b.push(1);
        b.extend([7; 8]);
        let revisions = revisions(&b, "s", "/revision", &mut 10).unwrap();
        assert_eq!(revisions[0].flags, 0x1234);
        assert_eq!(
            revisions[0].payload.as_ref().unwrap().0,
            [1, 7, 7, 7, 7, 7, 7, 7, 7]
        );
        assert!(super::revisions(&b, "s", "/revision", &mut 0).is_err());
        for end in 0..b.len() {
            assert!(super::revisions(&b[..end], "s", "/revision", &mut 10).is_err());
        }
        let mut invalid = b.clone();
        invalid[30] = 2;
        assert!(super::revisions(&invalid, "s", "/revision", &mut 10).is_err());
        let mut duplicate = b.clone();
        duplicate[4..8].copy_from_slice(&2u32.to_le_bytes());
        duplicate.extend(&b[8..]);
        assert!(super::revisions(&duplicate, "s", "/revision", &mut 10).is_err());
    }
}
