use super::*;
use crate::{property::guid, rse, stream, Error, Limits, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

fn span(id: &str, path: &str, start: usize, end: usize, inflated: bool) -> SourceSpan {
    let mut result = SourceSpan::stream(id, path, start, end);
    if inflated {
        result.byte_domain = "inflated_stream";
    }
    result
}
fn diagnostic(code: &str, message: impl Into<String>, source: Option<SourceSpan>) -> Diagnostic {
    Diagnostic::new(code, "warning", message, source)
}
fn opaque(source: SourceSpan, reason: &'static str) -> OpaqueRegion {
    OpaqueRegion { source, reason }
}

/// Inventory framing from one uniquely identified schema-31 database. All
/// payloads retain opaque coverage; typed field roles remain unqualified.
/// A record count is never a sheet/entity count.
/// No external references are followed. Original metadata remains available for
/// unsupported majors and ambiguous databases.
pub fn inspect(data: &[u8], source_id: &str, limits: &Limits) -> Result<DrawingInventory> {
    let summary = crate::inspect(data, source_id, limits)?.summary;
    let mut out = DrawingInventory {
        api_version: 1,
        source_id: source_id.into(),
        source_sha256: format!("{:x}", Sha256::digest(data)),
        status: "identity_only",
        drawing_semantics: "not_decoded",
        sheet_count: None,
        metadata: summary.document,
        segments: vec![],
        identities: vec![],
        unclaimed_streams: vec![],
        usage: Usage::default(),
        diagnostics: vec![],
    };
    if out.metadata.identification.kind != "drawing"
        || out.metadata.identification.status == "conflicting"
    {
        out.diagnostics.push(diagnostic(
            "drawing.document_kind",
            "identified IDW document required",
            None,
        ));
        return Ok(out);
    }
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| Error(e.to_string()))?;
    let mut work = limits.max_records;
    let mut expanded = limits.max_total_inflated_bytes;
    let result = scan(
        &mut file,
        &summary.streams,
        limits,
        &mut work,
        &mut expanded,
        &mut out,
    );
    out.usage.work_items = limits.max_records - work;
    out.usage.expanded_bytes = limits.max_total_inflated_bytes - expanded;
    if let Err(e) = result {
        out.diagnostics.push(diagnostic(
            "drawing.inventory_unavailable",
            e.to_string(),
            None,
        ));
    }
    let framed = out.segments.iter().filter(|s| s.status == "framed").count();
    if framed > 0 {
        out.status = if out.diagnostics.is_empty() && framed == out.segments.len() {
            "framed_subset"
        } else {
            "partial"
        };
    }
    Ok(out)
}

type File<'a> = cfb::CompoundFile<Cursor<&'a [u8]>>;

fn scan(
    file: &mut File<'_>,
    streams: &[crate::StreamInfo],
    limits: &Limits,
    work: &mut usize,
    expanded: &mut usize,
    out: &mut DrawingInventory,
) -> Result<()> {
    let id = out.source_id.clone();
    // Account even ignored Templates/unknown RSe streams before opening any.
    for s in streams
        .iter()
        .filter(|s| s.path.starts_with("/RSeStorage/"))
    {
        out.usage.encoded_bytes = out
            .usage
            .encoded_bytes
            .checked_add(s.bytes as usize)
            .filter(|n| *n <= limits.max_file_bytes)
            .ok_or_else(|| Error("drawing aggregate encoded byte limit exceeded".into()))?;
        if s.bytes > limits.max_stream_bytes as u64 {
            return Err(Error(format!(
                "drawing RSe stream byte limit exceeded: {}",
                s.path
            )));
        }
    }
    if out.metadata.databases.len() != 1
        || out.metadata.databases[0].schema != Some(31)
        || out.metadata.databases[0].status != "decoded"
    {
        out.diagnostics.push(diagnostic(
            "drawing.ambiguous_owner",
            "requires exactly one decoded schema-31 database; no database selected",
            None,
        ));
        for s in streams
            .iter()
            .filter(|s| s.path.starts_with("/RSeStorage/"))
        {
            out.unclaimed_streams.push(opaque(
                span(&id, &s.path, 0, s.bytes as usize, false),
                "database_owner_unresolved",
            ));
        }
        return Ok(());
    }
    let registry_path = "/RSeStorage/RSeSegInfo";
    let registry =
        rse::registry_budgeted(&stream(file, registry_path, limits.max_stream_bytes)?, work)?;
    let mut registry_ids = BTreeMap::<[u8; 16], Vec<usize>>::new();
    for (index, reg) in registry.iter().enumerate() {
        registry_ids.entry(reg.id).or_default().push(index);
        out.segments.push(SegmentInventory {
            registry_index: index,
            registry: SegmentInfo {
                name: reg.name.clone(),
                kind: reg.kind.clone(),
                id: guid(&reg.id),
                major: reg.major,
                source: span(&id, registry_path, reg.start_offset, reg.end_offset, false),
            },
            status: "identity_only",
            profile: None,
            meta: None,
            bulk: None,
            records: vec![],
            observations: vec![],
            opaque_regions: vec![],
            diagnostics: vec![],
        });
    }
    // Identify all document M streams before admitting any record. Templates
    // have a different, unqualified Meta header and are never document owners.
    // No scan-order winner for duplicate IDs; unidentifiable document M data
    // also prevents a uniqueness claim. Never traverse payload references.
    let sizes: BTreeMap<&str, usize> = streams
        .iter()
        .map(|s| (s.path.as_str(), s.bytes as usize))
        .collect();
    let mut owners = BTreeMap::<[u8; 16], Vec<usize>>::new();
    let mut encoded = Vec::<Option<Vec<u8>>>::new();
    let mut complete = true;
    for s in streams
        .iter()
        .filter(|s| s.path.starts_with("/RSeStorage/"))
    {
        let (parent, base) = s.path.rsplit_once('/').unwrap();
        if !base.starts_with('M') || base.len() <= 1 {
            continue;
        }
        rse::charge(work, 1)?;
        let scope = if parent == "/RSeStorage" {
            "document"
        } else if parent == "/RSeStorage/Templates" {
            "template"
        } else {
            "other"
        };
        let bpath = format!("{parent}/B{}", &base[1..]);
        let mut entry = MetaIdentity {
            source: span(&id, &s.path, 0, s.bytes as usize, false),
            bulk_source: sizes
                .get(bpath.as_str())
                .map(|n| span(&id, &bpath, 0, *n, false)),
            scope,
            id: None,
            name: None,
            registry_indices: vec![],
            status: "unavailable",
        };
        if scope != "document" {
            entry.status = "opaque_namespace";
            encoded.push(None);
            out.identities.push(entry);
            continue;
        }
        let result = stream(file, &s.path, limits.max_stream_bytes).and_then(|bytes| {
            let (key, name) = rse::meta_identity(&bytes)?;
            entry.id = Some(guid(&key));
            entry.name = Some(name);
            entry.registry_indices = registry_ids.get(&key).cloned().unwrap_or_default();
            entry.status = if entry.registry_indices.is_empty() {
                "unbound"
            } else {
                "identified"
            };
            owners.entry(key).or_default().push(out.identities.len());
            Ok(bytes)
        });
        match result {
            Ok(bytes) => encoded.push(Some(bytes)),
            Err(e) => {
                complete = false;
                encoded.push(None);
                out.diagnostics.push(diagnostic(
                    "drawing.meta_identity_unavailable",
                    e.to_string(),
                    Some(entry.source.clone()),
                ));
            }
        }
        out.identities.push(entry);
    }
    let mut claimed = BTreeSet::new();
    for (index, reg) in registry.iter().enumerate() {
        let segment = &mut out.segments[index];
        let matches = owners.get(&reg.id).map(Vec::as_slice).unwrap_or(&[]);
        let mut code = "drawing.segment_unavailable";
        let mut failure_source = segment.registry.source.clone();
        let result: Result<()> = (|| {
            if !complete || registry_ids[&reg.id].len() != 1 || matches.len() > 1 {
                code = "drawing.ambiguous_owner";
                segment.status = "ambiguous_owner";
                return Err(Error(
                    "registry/Meta identity is not unique or identity inventory is incomplete"
                        .into(),
                ));
            }
            let &[mi] = matches else {
                code = "drawing.missing_meta";
                segment.status = "missing_meta";
                return Err(Error("registry entry has no metadata owner".into()));
            };
            let owner = &mut out.identities[mi];
            if owner.scope != "document" || owner.name.as_deref() != Some(reg.name.as_str()) {
                code = "drawing.ambiguous_owner";
                segment.status = "ambiguous_owner";
                return Err(Error("registry and metadata name/scope disagree".into()));
            }
            owner.status = "unique_owner";
            if !super::profile::admits(&reg.kind, reg.major) {
                code = "drawing.unsupported_profile";
                segment.status = "unsupported_profile";
                return Err(Error(format!(
                    "no framing profile for {} / major {}",
                    reg.kind, reg.major
                )));
            }
            segment.profile = Some(super::profile::NAME);
            let bulk = owner.bulk_source.as_ref().ok_or_else(|| {
                code = "drawing.missing_bulk";
                segment.status = "missing_bulk";
                Error("matched metadata has no B stream in its own namespace".into())
            })?;
            let bytes = encoded[mi].take().unwrap();
            code = "drawing.meta_framing_unavailable";
            failure_source = owner.source.clone();
            let meta = rse::meta_layout_budgeted(
                &bytes,
                limits,
                expanded,
                work,
                super::profile::meta_layout(&reg.kind),
            )?;
            let mpath = owner.source.stream.as_str();
            let bpath = bulk.stream.as_str();
            let mut known = vec![(
                meta.block_table_offset,
                meta.block_table_offset + meta.blocks.len() * 4,
            )];
            let types = meta
                .types
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let offset = meta.type_table_offset + i * 28;
                    known.push((offset, offset + 16));
                    TypeEntry {
                        index: i,
                        type_id: guid(t),
                        source: span(&id, mpath, offset, offset + 16, true),
                    }
                })
                .collect();
            known.sort_unstable();
            let mut start = 0;
            for (a, b) in known {
                if a > start {
                    segment.opaque_regions.push(opaque(
                        span(&id, mpath, start, a, true),
                        "meta_fields_not_interpreted",
                    ));
                }
                start = b;
            }
            if start < meta.inflated_bytes {
                segment.opaque_regions.push(opaque(
                    span(&id, mpath, start, meta.inflated_bytes, true),
                    "meta_fields_not_interpreted",
                ));
            }
            segment.meta = Some(MetaInventory {
                source: owner.source.clone(),
                compressed_source: span(&id, mpath, meta.compressed_offset, bytes.len(), false),
                codec: meta.codec,
                expanded_bytes: meta.inflated_bytes,
                state_words: meta.state_words,
                block_words: meta.blocks.clone(),
                block_table_source: span(
                    &id,
                    mpath,
                    meta.block_table_offset,
                    meta.block_table_offset + meta.blocks.len() * 4,
                    true,
                ),
                types,
                reference_tables: meta
                    .reference_sections
                    .iter()
                    .map(|t| MetaReferenceTable {
                        section: t.number,
                        count: t.count,
                        bytes: t.bytes.clone(),
                        source: span(&id, mpath, t.offset, t.offset + t.bytes.len(), true),
                    })
                    .collect(),
            });
            // The fixed envelope and codec are admitted before decompression.
            code = "drawing.bulk_framing_unavailable";
            failure_source = bulk.clone();
            let bytes = stream(file, bpath, limits.max_stream_bytes)?;
            let compressed = super::profile::bulk(&bytes)?;
            let (body, codec) =
                rse::inflate_budgeted(compressed, limits.max_inflated_bytes, expanded)?;
            let table = rse::record_table(&body, &meta, reg.major, work, true)?;
            segment.bulk = Some(BulkInventory {
                source: bulk.clone(),
                compressed_source: span(&id, bpath, 18, bytes.len(), false),
                codec,
                expanded_bytes: body.len(),
                terminal_source: span(&id, bpath, table.terminal_offset, table.opaque_start, true),
            });
            for record in table.records {
                let payload = span(&id, bpath, record.start, record.end, true);
                match super::fields::decode(
                    &reg.kind,
                    &guid(&record.kind),
                    record.ordinal,
                    &body[record.start..record.end],
                    payload.clone(),
                    work,
                ) {
                    Ok(Some(observation)) => segment.observations.push(observation),
                    Ok(None) => (),
                    Err(e) => segment.diagnostics.push(diagnostic(
                        "drawing.fields_unavailable",
                        e.to_string(),
                        Some(payload.clone()),
                    )),
                }
                let trailer = span(&id, bpath, record.end, record.frame_end, true);
                segment
                    .opaque_regions
                    .push(opaque(payload.clone(), "drawing_payload_not_decoded"));
                segment
                    .opaque_regions
                    .push(opaque(trailer.clone(), "trailer_semantics_not_decoded"));
                segment.records.push(RecordInventory {
                    ordinal: record.ordinal,
                    type_id: guid(&record.kind),
                    selector_word: record.selector,
                    type_index: record.selector as u8 as usize,
                    meta_block_source: span(
                        &id,
                        mpath,
                        meta.block_table_offset + record.ordinal * 4,
                        meta.block_table_offset + (record.ordinal + 1) * 4,
                        true,
                    ),
                    source: payload,
                    selector_source: span(&id, bpath, record.start - 4, record.start, true),
                    trailer_source: trailer,
                    references: record
                        .references
                        .into_iter()
                        .map(|r| ReferenceCandidate {
                            basis: "framed_trailer_name_value",
                            raw_name: r.name,
                            raw_value: r.value,
                            status: "unresolved_not_followed",
                            source: span(&id, bpath, r.start, r.end, true),
                        })
                        .collect(),
                });
            }
            if table.opaque_start < body.len() {
                segment.opaque_regions.push(opaque(
                    span(&id, bpath, table.opaque_start, body.len(), true),
                    "bulk_suffix_not_interpreted",
                ));
            }
            claimed.insert(mpath.to_string());
            claimed.insert(bpath.to_string());
            segment.status = "framed";
            Ok(())
        })();
        if let Err(e) = result {
            if segment.status == "identity_only" {
                segment.status = "unavailable";
            }
            segment
                .diagnostics
                .push(diagnostic(code, e.to_string(), Some(failure_source)));
        }
    }
    for s in streams
        .iter()
        .filter(|s| s.path.starts_with("/RSeStorage/") && !claimed.contains(&s.path))
    {
        if s.path == registry_path || s.path == out.metadata.databases[0].stream {
            continue;
        }
        out.unclaimed_streams.push(opaque(
            span(&id, &s.path, 0, s.bytes as usize, false),
            if s.path.starts_with("/RSeStorage/Templates/") {
                "template_not_admitted"
            } else {
                "stream_not_framed"
            },
        ));
    }
    Ok(())
}
