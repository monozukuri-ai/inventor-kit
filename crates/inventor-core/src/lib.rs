//! Read-only Inventor part carrier extraction. CFB/RSe ownership is resolved
//! structurally; byte signatures are never used as record locators.
use acis_core::{AcisDiagnostic, AcisModel};
use serde::{Deserialize, Serialize};
use std::{
    error::Error as StdError,
    fmt,
    io::{Cursor, Read},
};
pub mod analysis;
pub mod assembly;
pub mod candidate;
pub mod document;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
pub mod property;
mod read;
mod rse;
pub mod thumbnail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl StdError for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_file_bytes: usize,
    pub max_stream_bytes: usize,
    pub max_inflated_bytes: usize,
    pub max_records: usize,
    pub max_streams: usize,
    pub max_property_bytes: usize,
    pub max_property_items: usize,
    pub max_property_depth: usize,
    pub max_total_inflated_bytes: usize,
    pub max_candidates: usize,
}
impl Limits {
    /// Limits may tighten the supported ceiling, never disable it. Zero denies
    /// that resource. Validate before arithmetic or filesystem access.
    pub fn validate(&self) -> Result<()> {
        let cap = Self::default();
        macro_rules! check {
            ($($field:ident),+ $(,)?) => { $(
                if self.$field > cap.$field {
                    return Err(Error(format!("{} exceeds hard limit {}", stringify!($field), cap.$field)));
                }
            )+ };
        }
        check!(
            max_file_bytes,
            max_stream_bytes,
            max_inflated_bytes,
            max_records,
            max_streams,
            max_property_bytes,
            max_property_items,
            max_property_depth,
            max_total_inflated_bytes,
            max_candidates
        );
        Ok(())
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: 128 * 1024 * 1024,
            max_stream_bytes: 64 * 1024 * 1024,
            max_inflated_bytes: 64 * 1024 * 1024,
            max_records: 500_000,
            max_streams: 65536,
            max_property_bytes: 16 * 1024 * 1024,
            max_property_items: 100_000,
            max_property_depth: 16,
            max_total_inflated_bytes: 128 * 1024 * 1024,
            max_candidates: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfo {
    pub path: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct CarrierInfo {
    pub stream: String,
    pub source_id: String,
    pub record_ordinal: usize,
    pub segment_major: u8,
    pub kernel_offset: usize,
    pub kernel_end: usize,
    pub codec: String,
    pub expanded_bytes: usize,
    pub meta_expanded_bytes: usize,
    pub selected_key: u32,
    pub enabled: bool,
    pub delta_state: i32,
    pub history_reference: u32,
    pub save_version: Option<u32>,
    pub integer_width: Option<u8>,
    pub header_scale: Option<f64>,
    pub has_history: Option<bool>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub source_id: String,
    pub root_clsid: String,
    pub kind: String,
    pub streams: Vec<StreamInfo>,
    pub status: String,
    pub diagnostics: Vec<String>,
    pub carrier: Option<CarrierInfo>,
    pub model_analysis: Option<analysis::ModelAnalysis>,
    pub document: document::DocumentInfo,
    pub geometry: candidate::GeometryInventory,
}
pub struct Document {
    pub summary: Summary,
    pub model: Option<AcisModel>,
    pub kernel_bytes: Option<Vec<u8>>,
}

fn stream(
    file: &mut cfb::CompoundFile<Cursor<&[u8]>>,
    path: &str,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut input = file
        .open_stream(path)
        .map_err(|e| Error(format!("{path}: {e}")))?;
    if input.len() > limit as u64 {
        return Err(Error(format!("stream byte limit: {path}")));
    }
    let mut bytes = Vec::new();
    (&mut input)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error(e.to_string()))?;
    if bytes.len() > limit {
        return Err(Error("stream byte limit exceeded".into()));
    }
    Ok(bytes)
}

/// Invalid CFB input is an error. Unsupported/ambiguous Inventor carriers return
/// an inventory with diagnostics and no model; they are never selected by guess.
pub fn read(data: &[u8], source_id: &str, limits: &Limits) -> Result<Document> {
    read_with_options(data, source_id, limits, &candidate::ReadOptions::default())
}

/// Read document metadata and registry without inflating or decoding B-rep streams.
pub fn inspect(data: &[u8], source_id: &str, limits: &Limits) -> Result<Document> {
    read_document(
        data,
        source_id,
        limits,
        false,
        false,
        &candidate::ReadOptions::default(),
    )
}

/// Enumerate stored kernel candidates without importing a Python geometry model.
/// Unlike inspect, this inflates PmBRep streams and validates their SAB tables.
pub fn inspect_candidates(data: &[u8], source_id: &str, limits: &Limits) -> Result<Document> {
    read_document(
        data,
        source_id,
        limits,
        false,
        true,
        &candidate::ReadOptions::default(),
    )
}

pub fn read_with_options(
    data: &[u8],
    source_id: &str,
    limits: &Limits,
    options: &candidate::ReadOptions,
) -> Result<Document> {
    read_document(data, source_id, limits, true, true, options)
}

fn read_document(
    data: &[u8],
    source_id: &str,
    limits: &Limits,
    geometry: bool,
    inventory: bool,
    options: &candidate::ReadOptions,
) -> Result<Document> {
    limits.validate()?;
    if source_id.is_empty() {
        return Err(Error("source identifier is empty".into()));
    }
    if data.len() > limits.max_file_bytes {
        return Err(Error("file byte limit exceeded".into()));
    }
    let mut file =
        cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| Error(format!("CFB: {e}")))?;
    let root_clsid = file.root_entry().clsid().to_string();
    let mut streams = Vec::new();
    for entry in file.walk() {
        if entry.is_stream() {
            streams.push(StreamInfo {
                path: entry.path().to_string_lossy().into_owned(),
                bytes: entry.len(),
            });
        }
        if streams.len() > limits.max_streams {
            return Err(Error("CFB stream count limit exceeded".into()));
        }
    }
    if !streams.iter().any(|s| s.path == "/RSeStorage/RSeSegInfo")
        && document::root_kind(&root_clsid).is_none()
    {
        return Err(Error("CFB has no Inventor segment registry".into()));
    }
    let (information, registry) =
        document::inspect(&mut file, source_id, &root_clsid, &streams, limits);
    let mut doc = Document {
        summary: Summary {
            source_id: source_id.into(),
            root_clsid,
            kind: information.identification.kind.clone(),
            streams,
            status: "geometry_unavailable".into(),
            diagnostics: Vec::new(),
            carrier: None,
            model_analysis: None,
            document: information,
            geometry: candidate::GeometryInventory::default(),
        },
        model: None,
        kernel_bytes: None,
    };
    if inventory {
        let mut scan = candidate::scan(&mut file, data, &doc.summary, &registry, limits);
        if geometry {
            doc.summary.document.stages.geometry = "unavailable";
            if let Err(e) = extract(&mut doc, &mut scan, limits, options) {
                let mut diagnostic = document::Diagnostic::new(
                    "geometry.extraction_failed",
                    "warning",
                    e.to_string(),
                    None,
                );
                diagnostic.profile = Some(doc.summary.document.geometry_profile.clone());
                if let Some(carrier) = &doc.summary.carrier {
                    let mut source = document::SourceSpan::stream(
                        source_id,
                        &carrier.stream,
                        carrier.kernel_offset,
                        carrier.kernel_end,
                    );
                    source.byte_domain = "inflated_stream";
                    diagnostic.source = Some(source);
                    diagnostic.record = Some(carrier.record_ordinal);
                    doc.summary.document.stages.geometry = "carrier_selected";
                }
                doc.summary.document.diagnostics.push(diagnostic);
                doc.summary.diagnostics.push(e.to_string());
            }
        }
        doc.summary.geometry = scan.info;
    }
    Ok(doc)
}

fn extract(
    doc: &mut Document,
    scan: &mut candidate::Scan,
    limits: &Limits,
    options: &candidate::ReadOptions,
) -> Result<()> {
    let index = candidate::select(&mut scan.info, &doc.summary, options)?;
    let chosen = &scan.info.candidates[index];
    doc.summary.carrier = chosen.carrier.clone();
    if let Some(c) = &chosen.carrier {
        doc.summary.document.geometry_profile.segment_major = Some(c.segment_major);
        doc.summary.document.geometry_profile.sab_version = c.save_version;
    }
    let prepared = scan.prepared.remove(&chosen.id).ok_or_else(|| {
        Error(
            chosen
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    doc.kernel_bytes = Some(prepared.kernel);
    if chosen.table_status != "solved_table" || chosen.profile_status != "supported_stored_table" {
        return Err(Error(
            chosen
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    let mut parsed = prepared
        .parsed
        .ok_or_else(|| Error("selected SAB table unavailable".into()))?;
    let info = doc.summary.carrier.as_ref().unwrap();
    let kernel_offset = info.kernel_offset;
    let mut metadata = parsed.model.metadata().clone();
    // Inventor's database coordinates are cm. Header scale is retained separately.
    metadata.units_mm = Some(10.0);
    metadata.dialect = Some(format!(
        "inventor/rse31/meta8/segment{}/sab{}",
        info.segment_major, parsed.header.save_version
    ));
    let mut entities = parsed.model.entities().to_vec();
    for entity in &mut entities {
        offset_entity_source(entity, kernel_offset);
    }
    let mut diagnostics = parsed.model.diagnostics().to_vec();
    for diagnostic in &mut diagnostics {
        if let Some(span) = &mut diagnostic.source {
            span.start_offset += kernel_offset;
            span.end_offset += kernel_offset;
        }
    }
    diagnostics.push(AcisDiagnostic {code:"inventor.geometry_scope".into(),message:"Selected stored PmBRep solved table; current Model State, suppression, history replay and assembly placement are unverified".into(),entity_index:None,source:None});
    parsed.model =
        AcisModel::new(metadata, entities, diagnostics).map_err(|e| Error(e.to_string()))?;
    doc.summary.status = "decoded_subset".into();
    doc.summary.document.stages.geometry = "decoded_subset";
    doc.summary.model_analysis = Some(analysis::analyze_model(
        &parsed.model,
        limits.max_records.saturating_mul(4),
    ));
    doc.model = Some(parsed.model);
    Ok(())
}

fn offset_entity_source(entity: &mut acis_core::Entity, offset: usize) {
    use acis_core::Entity::*;
    let raw = match entity {
        Raw(r) => r,
        Body(e) => &mut e.raw,
        Lump(e) => &mut e.raw,
        Shell(e) => &mut e.raw,
        Face(e) => &mut e.raw,
        Loop(e) => &mut e.raw,
        Coedge(e) => &mut e.raw,
        Edge(e) => &mut e.raw,
        Vertex(e) => &mut e.raw,
        Point(e) => &mut e.raw,
        StraightCurve(e) => &mut e.raw,
        EllipseCurve(e) => &mut e.raw,
        PlaneSurface(e) => &mut e.raw,
        ConeSurface(e) => &mut e.raw,
        SphereSurface(e) => &mut e.raw,
        TorusSurface(e) => &mut e.raw,
        BSplineSurface(e) => &mut e.raw,
        BSplineCurve(e) => &mut e.raw,
        Transform(e) => &mut e.raw,
    };
    if let Some(span) = &mut raw.source {
        span.start_offset += offset;
        span.end_offset += offset;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod property_tests;

#[cfg(test)]
mod candidate_tests;
