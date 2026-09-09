//! Stored-table inventory and explicit selection. No Model State evaluator.
use crate::{
    document::{Diagnostic, SourceSpan},
    property,
    read::Reader,
    rse, stream, CarrierInfo, Error, Limits, Result, Summary,
};
use acis_core::sab::{parse_sab, SabDocument, SabLimits};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::Cursor,
};

const KERNEL: [u8; 16] = [
    0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47, 0xb5,
];

#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    pub candidate_id: Option<String>,
    /// Fail closed until a source/state binding has been independently qualified.
    pub require_current_state: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    pub policy: &'static str,
    pub requested_id: Option<String>,
    pub selected_id: Option<String>,
    pub status: &'static str,
    pub basis: Option<&'static str>,
    pub state_verification: &'static str,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct SegmentInventory {
    pub token: String,
    pub meta_source: Option<SourceSpan>,
    pub bulk_source: Option<SourceSpan>,
    pub segment_id: Option<String>,
    pub name: Option<String>,
    pub registry_indices: Vec<usize>,
    pub status: &'static str,
    pub meta_compressed_source: Option<SourceSpan>,
    pub meta_codec: Option<&'static str>,
    pub meta_expanded_bytes: Option<usize>,
    pub meta_state_words: Option<[u32; 3]>,
    pub record_count: Option<usize>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CarrierHeader {
    pub state: u32,
    pub kind: u16,
    pub value: u32,
    pub schema: u32,
    pub source: SourceSpan,
    pub interpretation: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct KernelCandidate {
    pub id: String,
    pub segment_index: usize,
    pub registry_index: usize,
    pub segment_id: String,
    pub record_ordinal: usize,
    pub record_type: String,
    pub record_source: SourceSpan,
    pub compressed_source: SourceSpan,
    pub kernel_source: Option<SourceSpan>,
    pub kernel_relative_source: Option<SourceSpan>,
    pub kernel_sha256: Option<String>,
    pub header: Option<CarrierHeader>,
    pub footer_source: Option<SourceSpan>,
    pub carrier: Option<CarrierInfo>,
    pub table_status: &'static str,
    pub profile_status: &'static str,
    pub solved_records: Option<usize>,
    pub solved_source: Option<SourceSpan>,
    pub history_source: Option<SourceSpan>,
    pub database_binding: &'static str,
    pub state_binding: &'static str,
    pub suppression: &'static str,
    pub history_replay: &'static str,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Serialize)]
pub struct GeometryInventory {
    pub api_version: u32,
    pub source_sha256: Option<String>,
    pub status: &'static str,
    pub segments: Vec<SegmentInventory>,
    pub candidates: Vec<KernelCandidate>,
    pub selection: Selection,
    pub diagnostics: Vec<Diagnostic>,
}
impl Default for GeometryInventory {
    fn default() -> Self {
        Self {
            api_version: 1,
            source_sha256: None,
            status: "not_scanned",
            segments: vec![],
            candidates: vec![],
            selection: Selection {
                policy: "not_requested",
                requested_id: None,
                selected_id: None,
                status: "not_requested",
                basis: None,
                state_verification: "unverified",
                reason: None,
            },
            diagnostics: vec![],
        }
    }
}
pub(crate) struct Prepared {
    pub kernel: Vec<u8>,
    pub parsed: Option<SabDocument>,
}
pub(crate) struct Scan {
    pub info: GeometryInventory,
    pub prepared: HashMap<String, Prepared>,
}
pub(crate) fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn span(
    summary: &Summary,
    path: &str,
    start: usize,
    end: usize,
    domain: &'static str,
) -> SourceSpan {
    let mut s = SourceSpan::stream(&summary.source_id, path, start, end);
    s.byte_domain = domain;
    s
}
fn warning(code: &str, message: impl Into<String>, source: Option<SourceSpan>) -> Diagnostic {
    Diagnostic::new(code, "warning", message, source)
}
fn allowed(major: u8, version: u32) -> bool {
    matches!(
        (major, version),
        (19, 22000) | (25, 22600) | (26, 22700) | (28, 22900) | (31, 23200)
    )
}
fn unpack(
    expanded: &[u8],
    record: &rse::Record,
    major: u8,
    summary: &Summary,
    path: &str,
    codec: &str,
    meta_bytes: usize,
) -> Result<(CarrierInfo, CarrierHeader)> {
    let footer_len = if major >= 23 { 18 } else { 17 };
    if record.end - record.start < 14 + footer_len {
        return Err(Error("truncated typed kernel carrier".into()));
    }
    let kernel_offset = record.start + 14;
    let kernel_end = record.end - footer_len;
    let mut h = Reader::new(&expanded[record.start..kernel_offset]);
    let header = CarrierHeader {
        state: h.u32()?,
        kind: h.u16()?,
        value: h.u32()?,
        schema: h.u32()?,
        source: span(
            summary,
            path,
            record.start,
            kernel_offset,
            "inflated_stream",
        ),
        interpretation: "unresolved",
    };
    h.finish()?;
    let mut f = Reader::new(&expanded[kernel_end..record.end]);
    let selected_key = f.u32()?;
    let enabled = match f.u8()? {
        0 => false,
        1 => true,
        _ => return Err(Error("invalid carrier Boolean".into())),
    };
    let delta_state = f.u32()? as i32;
    if major >= 23 && f.u8()? != 0 {
        return Err(Error("invalid carrier versioned padding".into()));
    }
    let history_reference = f.u32()?;
    if f.u32()? != u32::MAX {
        return Err(Error("invalid carrier footer".into()));
    }
    f.finish()?;
    Ok((
        CarrierInfo {
            stream: path.into(),
            source_id: format!("{}:{path}:inflated", summary.source_id),
            record_ordinal: record.ordinal,
            segment_major: major,
            kernel_offset,
            kernel_end,
            codec: codec.into(),
            expanded_bytes: expanded.len(),
            meta_expanded_bytes: meta_bytes,
            selected_key,
            enabled,
            delta_state,
            history_reference,
            save_version: None,
            integer_width: None,
            header_scale: None,
            has_history: None,
        },
        header,
    ))
}

pub(crate) fn scan(
    file: &mut cfb::CompoundFile<Cursor<&[u8]>>,
    data: &[u8],
    summary: &Summary,
    registry: &[rse::Segment],
    limits: &Limits,
) -> Scan {
    let digest = sha256(data);
    let mut scan = Scan {
        info: GeometryInventory {
            source_sha256: Some(digest.clone()),
            status: "complete",
            ..Default::default()
        },
        prepared: HashMap::new(),
    };
    let mut pairs =
        BTreeMap::<String, (Option<&crate::StreamInfo>, Option<&crate::StreamInfo>)>::new();
    for s in &summary.streams {
        if let Some(name) = s
            .path
            .strip_prefix("/RSeStorage/")
            .filter(|n| !n.contains('/') && n.len() > 1)
        {
            if let Some(token) = name.strip_prefix('M') {
                pairs.entry(token.into()).or_default().0 = Some(s);
            }
            if let Some(token) = name.strip_prefix('B') {
                pairs.entry(token.into()).or_default().1 = Some(s);
            }
        }
    }
    let mut joins = vec![0usize; registry.len()];
    let mut expanded_budget = limits.max_total_inflated_bytes;
    let mut record_budget = limits.max_records;
    let mut encoded_budget = limits.max_file_bytes;
    for (token, (m, b)) in pairs {
        let index = scan.info.segments.len();
        let mut entry = SegmentInventory {
            token,
            meta_source: m.map(|s| span(summary, &s.path, 0, s.bytes as usize, "cfb_stream")),
            bulk_source: b.map(|s| span(summary, &s.path, 0, s.bytes as usize, "cfb_stream")),
            segment_id: None,
            name: None,
            registry_indices: vec![],
            status: "bulk_only",
            meta_compressed_source: None,
            meta_codec: None,
            meta_expanded_bytes: None,
            meta_state_words: None,
            record_count: None,
        };
        let inspected: Result<()> = (|| {
            let Some(m) = m else {
                return Ok(());
            };
            entry.status = "unavailable";
            encoded_budget = encoded_budget
                .checked_sub(m.bytes as usize)
                .ok_or_else(|| Error("aggregate encoded metadata byte limit exceeded".into()))?;
            let bytes = stream(file, &m.path, limits.max_stream_bytes)?;
            let (id, name) = rse::meta_identity(&bytes)?;
            entry.segment_id = Some(property::guid(&id));
            entry.name = Some(name.clone());
            entry.registry_indices = registry
                .iter()
                .enumerate()
                .filter(|(_, r)| r.id == id)
                .map(|(i, _)| i)
                .collect();
            for &i in &entry.registry_indices {
                joins[i] += 1;
            }
            if entry.registry_indices.is_empty() {
                entry.status = "unregistered";
                return Err(Error(
                    "metadata identity has no decoded registry owner".into(),
                ));
            }
            if entry.registry_indices.len() != 1 {
                entry.status = "ambiguous_owner";
                return Err(Error(
                    "duplicate registry identities; metadata owner is ambiguous".into(),
                ));
            }
            let ri = entry.registry_indices[0];
            let reg = &registry[ri];
            if reg.name != name {
                entry.status = "identity_conflict";
                return Err(Error("registry and metadata names disagree".into()));
            }
            if reg.kind != "PmBrepSegmentType" {
                entry.status = "identity_only";
                return Ok(());
            }
            if !matches!(reg.major, 19 | 25 | 26 | 28 | 31) {
                entry.status = "unsupported";
                return Err(Error(format!(
                    "unsupported PmBRep segment major {}",
                    reg.major
                )));
            }
            let bounded = Limits {
                max_inflated_bytes: limits.max_inflated_bytes.min(expanded_budget),
                max_records: record_budget,
                ..limits.clone()
            };
            let meta = rse::meta(&bytes, &bounded)?;
            debug_assert!(meta.id == id && meta.name == name);
            expanded_budget -= meta.inflated_bytes;
            record_budget = record_budget
                .checked_sub(meta.blocks.len())
                .ok_or_else(|| Error("aggregate record limit exceeded".into()))?;
            entry.meta_compressed_source = Some(span(
                summary,
                &m.path,
                meta.compressed_offset,
                bytes.len(),
                "cfb_stream",
            ));
            entry.meta_codec = Some(meta.codec);
            entry.meta_expanded_bytes = Some(meta.inflated_bytes);
            entry.meta_state_words = Some(meta.state_words);
            let b =
                b.ok_or_else(|| Error("matched PmBRep metadata has no paired B stream".into()))?;
            let bytes = stream(file, &b.path, limits.max_stream_bytes)?;
            let compressed = bytes
                .get(18..)
                .ok_or_else(|| Error("truncated B-stream prefix".into()))?;
            let (expanded, codec) =
                rse::inflate(compressed, limits.max_inflated_bytes.min(expanded_budget))?;
            expanded_budget -= expanded.len();
            let records = rse::records(&expanded, &meta, reg.major)?;
            entry.record_count = Some(records.len());
            for record in records.iter().filter(|r| r.kind == KERNEL) {
                if scan.info.candidates.len() >= limits.max_candidates {
                    return Err(Error("kernel candidate count limit exceeded".into()));
                }
                let id = format!("{digest}:{}:{}", entry.token, record.ordinal);
                let mut candidate = KernelCandidate {
                    id: id.clone(),
                    segment_index: index,
                    registry_index: ri,
                    segment_id: property::guid(&reg.id),
                    record_ordinal: record.ordinal,
                    record_type: property::guid(&record.kind),
                    record_source: span(
                        summary,
                        &b.path,
                        record.start,
                        record.end,
                        "inflated_stream",
                    ),
                    compressed_source: span(summary, &b.path, 18, bytes.len(), "cfb_stream"),
                    kernel_source: None,
                    kernel_relative_source: None,
                    kernel_sha256: None,
                    header: None,
                    footer_source: None,
                    carrier: None,
                    table_status: "unavailable",
                    profile_status: "unverified",
                    solved_records: None,
                    solved_source: None,
                    history_source: None,
                    database_binding: "unresolved",
                    state_binding: "unresolved",
                    suppression: "unresolved",
                    history_replay: "not_attempted",
                    diagnostics: vec![],
                };
                match unpack(
                    &expanded,
                    record,
                    reg.major,
                    summary,
                    &b.path,
                    codec,
                    meta.inflated_bytes,
                ) {
                    Err(e) => candidate.diagnostics.push(warning(
                        "candidate.carrier_invalid",
                        e.to_string(),
                        Some(candidate.record_source.clone()),
                    )),
                    Ok((mut carrier, header)) => {
                        let start = carrier.kernel_offset;
                        let end = carrier.kernel_end;
                        let kernel = expanded[start..end].to_vec();
                        candidate.header = Some(header);
                        candidate.footer_source =
                            Some(span(summary, &b.path, end, record.end, "inflated_stream"));
                        candidate.kernel_source =
                            Some(span(summary, &b.path, start, end, "inflated_stream"));
                        candidate.kernel_relative_source = Some(SourceSpan {
                            source_id: id.clone(),
                            stream: b.path.clone(),
                            byte_domain: "kernel",
                            start_offset: 0,
                            end_offset: kernel.len(),
                        });
                        candidate.kernel_sha256 = Some(sha256(&kernel));
                        let mut parsed = None;
                        let sab_limits = SabLimits {
                            max_bytes: limits.max_inflated_bytes,
                            max_records: record_budget,
                            ..Default::default()
                        };
                        match parse_sab(&kernel, &carrier.source_id, &sab_limits) {
                            Err(e) => {
                                candidate.table_status = "unavailable";
                                // acis-core 0.1.0 validates the header and reports
                                // its exact first-record offset on this error.
                                // Recognize only a delta_state token AT that
                                // boundary; never search for a history signature.
                                // No history contents or header facts are decoded
                                // by this compatibility classification.
                                if e.message == "expected asmheader record zero"
                                    && kernel
                                        .get(e.offset..)
                                        .is_some_and(|b| b.starts_with(b"\x0d\x0bdelta_state"))
                                {
                                    candidate.table_status = "history_only";
                                    candidate.solved_records = Some(0);
                                    candidate.history_source = Some(span(
                                        summary,
                                        &b.path,
                                        start + e.offset,
                                        end,
                                        "inflated_stream",
                                    ));
                                    candidate.history_replay = "required_unimplemented";
                                    candidate.diagnostics.push(warning("candidate.history_replay_required", "A delta_state starts at the shared decoder's first-record boundary; no solved table is available and replay is not implemented", candidate.history_source.clone()));
                                }
                                candidate.diagnostics.push(warning(
                                    "candidate.sab_unavailable",
                                    e.to_string(),
                                    candidate.kernel_source.clone(),
                                ));
                            }
                            Ok(p) => {
                                record_budget -= p.model.entities().len();
                                carrier.save_version = Some(p.header.save_version);
                                carrier.integer_width = Some(p.header.integer_width);
                                carrier.header_scale = Some(p.header.scale);
                                carrier.has_history = Some(p.history.is_some());
                                let solved =
                                    p.model.entities().len() - usize::from(p.history.is_some());
                                candidate.solved_records = Some(solved);
                                let solved_end = p
                                    .model
                                    .entities()
                                    .iter()
                                    .take(solved)
                                    .filter_map(|e| e.raw().source.as_ref().map(|s| s.end_offset))
                                    .max()
                                    .unwrap_or(p.header.data_offset);
                                candidate.solved_source = Some(span(
                                    summary,
                                    &b.path,
                                    start + p.header.data_offset,
                                    start + solved_end,
                                    "inflated_stream",
                                ));
                                candidate.history_source = p.history.as_ref().map(|h| {
                                    span(
                                        summary,
                                        &b.path,
                                        start + h.start_offset,
                                        start + h.end_offset,
                                        "inflated_stream",
                                    )
                                });
                                candidate.table_status = if solved == 0 && p.history.is_some() {
                                    "history_only"
                                } else {
                                    "solved_table"
                                };
                                candidate.profile_status =
                                    if allowed(reg.major, p.header.save_version) {
                                        "supported_stored_table"
                                    } else {
                                        "unsupported"
                                    };
                                if candidate.profile_status == "unsupported" {
                                    candidate.diagnostics.push(warning(
                                        "candidate.profile_unsupported",
                                        format!(
                                            "unsupported segment/SAB profile {}/{}",
                                            reg.major, p.header.save_version
                                        ),
                                        candidate.kernel_source.clone(),
                                    ));
                                }
                                if candidate.table_status == "history_only" {
                                    candidate.history_replay = "required_unimplemented";
                                    candidate.diagnostics.push(warning("candidate.history_replay_required","No solved records precede the history partition; replay is not implemented",candidate.history_source.clone()));
                                }
                                parsed = Some(p);
                            }
                        }
                        candidate.carrier = Some(carrier);
                        scan.prepared.insert(id, Prepared { kernel, parsed });
                    }
                }
                scan.info.candidates.push(candidate);
            }
            entry.status = "framed";
            Ok(())
        })();
        if let Err(e) = inspected {
            scan.info.status = "partial";
            scan.info.diagnostics.push(warning(
                "candidate.segment_unavailable",
                e.to_string(),
                entry
                    .meta_source
                    .clone()
                    .or_else(|| entry.bulk_source.clone()),
            ));
        }
        scan.info.segments.push(entry);
    }
    for (i, r) in registry
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == "PmBrepSegmentType")
    {
        if joins[i] != 1 {
            scan.info.status = "partial";
            scan.info.diagnostics.push(warning(
                "candidate.metadata_join_unresolved",
                format!(
                    "PmBRep {}: expected one metadata segment-id match, found {}",
                    property::guid(&r.id),
                    joins[i]
                ),
                Some(span(
                    summary,
                    "/RSeStorage/RSeSegInfo",
                    r.start_offset,
                    r.end_offset,
                    "cfb_stream",
                )),
            ));
        }
    }
    if summary.document.stages.registry != "decoded" {
        scan.info.status = "partial";
    }
    scan
}

pub(crate) fn select(
    info: &mut GeometryInventory,
    summary: &Summary,
    options: &ReadOptions,
) -> Result<usize> {
    info.selection.policy = if options.require_current_state {
        "require_current_state"
    } else if options.candidate_id.is_some() {
        "explicit_candidate"
    } else {
        "sole_stored_table"
    };
    info.selection.requested_id = options.candidate_id.clone();
    let choice: Result<usize> = (|| {
        if summary.kind != "part" {
            return Err(Error(
                "only part geometry is admitted; assembly/drawing inventory is retained".into(),
            ));
        }
        if options.require_current_state {
            info.selection.status = "state_unverified";
            return Err(Error("No profile has a vendor-qualified current Model State binding; stored-table selection cannot satisfy require_current_state".into()));
        }
        if let Some(id) = &options.candidate_id {
            let i = info
                .candidates
                .iter()
                .position(|c| &c.id == id)
                .ok_or_else(|| {
                    info.selection.status = "not_found";
                    Error(
                        "candidate_id is absent from this exact input; no fallback selection"
                            .into(),
                    )
                })?;
            info.selection.basis = Some("user_requested_stored_candidate");
            return Ok(i);
        }
        if summary.document.databases.len() != 1
            || summary.document.databases[0].status != "decoded"
        {
            info.selection.status = "database_unresolved";
            return Err(Error("automatic selection requires one decoded RSeDb; database identity/state remains unresolved".into()));
        }
        if info.status != "complete" {
            info.selection.status = "incomplete";
            return Err(Error(format!(
                "candidate inventory incomplete: {}",
                info.diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        if info.candidates.len() != 1 {
            info.selection.status = if info.candidates.is_empty() {
                "unavailable"
            } else {
                "ambiguous"
            };
            return Err(Error(format!(
                "expected one typed kernel carrier, found {}; no active-state selection rule",
                info.candidates.len()
            )));
        }
        if summary
            .document
            .segments
            .iter()
            .filter(|s| s.kind == "PmBrepSegmentType")
            .count()
            != 1
        {
            info.selection.status = "ambiguous";
            return Err(Error(
                "multiple PmBRep segments require explicit stored-candidate selection".into(),
            ));
        }
        // An enabled flag is not a suppression oracle. A changed/disabled flag
        // requires explicit choice instead of silently applying the sole-table rule.
        if info.candidates[0]
            .carrier
            .as_ref()
            .is_some_and(|c| !c.enabled)
        {
            info.selection.status = "state_unverified";
            return Err(Error("carrier enabled=false is outside automatic selection; suppression semantics are unresolved".into()));
        }
        info.selection.basis = Some("sole_structurally_located_stored_candidate");
        Ok(0)
    })();
    match choice {
        Ok(i) => {
            info.selection.status = "selected";
            info.selection.selected_id = Some(info.candidates[i].id.clone());
            Ok(i)
        }
        Err(e) => {
            if info.selection.status == "not_requested" {
                info.selection.status = "unavailable";
            }
            info.selection.reason = Some(e.to_string());
            Err(e)
        }
    }
}
