//! IDW framing and provisional typed fields. No qualified sheet selection or rendering.
mod appearance;
mod fields;
mod geometry;
mod images;
mod inventory;
mod limits;
mod profile;
mod revisions;
mod scene;
mod sheet;
mod sheets;
mod style;
mod text;

use crate::document::{Diagnostic, DocumentInfo, SegmentInfo, SourceSpan};
pub use appearance::DisplayStyle;
pub use images::{read_embedded_images, read_embedded_images_with_limits, EmbeddedImage};
pub use inventory::inspect;
pub use limits::{DrawingLimits, DrawingOutputBuffer};
pub use revisions::{Revision, RevisionTable};
pub use scene::{
    experimental_scene, experimental_scene_with_limits, DisplayBinding, DisplayFont,
    DisplayGeometry, DisplayItem, DisplaySpace, ExperimentalScene, Omission, StoredView,
};
use serde::Serialize;
pub use sheets::{stored_sheets, stored_sheets_with_limits, StoredSheet, StoredSheets};

#[derive(Debug, Serialize)]
pub struct DrawingInventory {
    pub api_version: u32,
    pub source_id: String,
    pub source_sha256: String,
    pub status: &'static str,
    pub drawing_semantics: &'static str,
    pub sheet_count: Option<usize>,
    pub metadata: DocumentInfo,
    pub segments: Vec<SegmentInventory>,
    pub identities: Vec<MetaIdentity>,
    pub unclaimed_streams: Vec<OpaqueRegion>,
    pub usage: Usage,
    pub diagnostics: Vec<Diagnostic>,
    pub revisions: Option<RevisionTable>,
}

#[derive(Debug, Serialize, Default)]
pub struct Usage {
    /// Declared RSe bytes, including Templates and unsupported/opaque streams.
    pub encoded_bytes: usize,
    /// Spent expansion allowance, including failed attempts. An opaque decoder
    /// error conservatively exhausts the allowance when output is unmeasurable.
    pub expanded_bytes: usize,
    /// Registry, identity, Meta-table, record and trailer collection work.
    pub work_items: usize,
}

#[derive(Debug, Serialize)]
pub struct MetaIdentity {
    pub source: SourceSpan,
    pub bulk_source: Option<SourceSpan>,
    pub scope: &'static str,
    pub id: Option<String>,
    pub name: Option<String>,
    pub registry_indices: Vec<usize>,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SegmentInventory {
    pub registry_index: usize,
    pub registry: SegmentInfo,
    pub status: &'static str,
    pub profile: Option<&'static str>,
    pub meta: Option<MetaInventory>,
    pub bulk: Option<BulkInventory>,
    pub records: Vec<RecordInventory>,
    /// Exact typed fields with unqualified roles; never a display list.
    pub observations: Vec<PayloadObservation>,
    pub opaque_regions: Vec<OpaqueRegion>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub struct PayloadObservation {
    pub record_ordinal: usize,
    pub type_id: String,
    pub source: SourceSpan,
    pub layout: &'static str,
    pub proposed_role: &'static str,
    pub status: &'static str,
    pub fields: Vec<FieldObservation>,
}

#[derive(Debug, Serialize)]
pub struct FieldObservation {
    pub name: &'static str,
    pub value: FieldValue,
    pub source: SourceSpan,
}

#[derive(Debug, Serialize)]
#[serde(tag = "encoding", content = "raw")]
pub enum FieldValue {
    #[serde(rename = "u8")]
    U8(u8),
    #[serde(rename = "utf16le_counted")]
    Utf16(String),
    #[serde(rename = "f32le")]
    F32(Vec<f32>),
    #[serde(rename = "f64le")]
    F64(Vec<f64>),
    #[serde(rename = "u32le")]
    U32(Vec<u32>),
    #[serde(rename = "u16le")]
    U16(u16),
    #[serde(rename = "compact_transform")]
    CompactTransform {
        prefixed: bool,
        set: u16,
        zero: u16,
        values: [f64; 16],
    },
}

#[derive(Debug, Serialize)]
pub struct MetaInventory {
    pub source: SourceSpan,
    pub compressed_source: SourceSpan,
    pub codec: &'static str,
    pub expanded_bytes: usize,
    pub state_words: [u32; 3],
    /// All block slots, including inactive slots. Ordinals are never compacted.
    pub block_words: Vec<u32>,
    pub block_table_source: SourceSpan,
    pub types: Vec<TypeEntry>,
    /// Unqualified cross-segment reference tables; no external files are opened.
    pub reference_tables: Vec<MetaReferenceTable>,
}

#[derive(Debug, Serialize)]
pub struct MetaReferenceTable {
    pub section: u8,
    pub count: usize,
    pub bytes: Vec<u8>,
    pub source: SourceSpan,
}

#[derive(Debug, Serialize)]
pub struct TypeEntry {
    pub index: usize,
    pub type_id: String,
    pub source: SourceSpan,
}

#[derive(Debug, Serialize)]
pub struct BulkInventory {
    pub source: SourceSpan,
    pub compressed_source: SourceSpan,
    pub codec: &'static str,
    pub expanded_bytes: usize,
    pub terminal_source: SourceSpan,
}

#[derive(Debug, Serialize)]
pub struct RecordInventory {
    pub ordinal: usize,
    pub type_id: String,
    /// Retain all bits; only the low byte selects the Meta type table.
    pub selector_word: u32,
    pub type_index: usize,
    pub meta_block_source: SourceSpan,
    pub source: SourceSpan,
    pub selector_source: SourceSpan,
    pub trailer_source: SourceSpan,
    pub references: Vec<ReferenceCandidate>,
}

#[derive(Debug, Serialize)]
pub struct ReferenceCandidate {
    pub basis: &'static str,
    pub raw_name: String,
    pub raw_value: u32,
    pub status: &'static str,
    pub source: SourceSpan,
}

#[derive(Debug, Serialize)]
pub struct OpaqueRegion {
    pub source: SourceSpan,
    pub reason: &'static str,
}

#[cfg(test)]
mod tests;

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_fields(bytes: &[u8], limits: &crate::Limits) {
    fields::fuzz(bytes, limits);
}

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_pair(meta_bytes: &[u8], bulk_bytes: &[u8], limits: &crate::Limits) {
    let mut expanded = limits.max_total_inflated_bytes;
    let mut work = limits.max_records;
    let _ = (|| -> crate::Result<()> {
        let (_, name) = crate::rse::meta_identity(meta_bytes)?;
        let kind = if name == "DlDocDCSegment" {
            "DlDocDcSegmentType"
        } else if name.starts_with("DLSheet") && name.ends_with("DCSegment") {
            "DlSheetDcSegmentType"
        } else {
            ""
        };
        let meta = crate::rse::meta_layout_budgeted(
            meta_bytes,
            limits,
            &mut expanded,
            &mut work,
            profile::meta_layout(kind),
        )?;
        let compressed = profile::bulk(bulk_bytes)?;
        let (body, _) =
            crate::rse::inflate_budgeted(compressed, limits.max_inflated_bytes, &mut expanded)?;
        crate::rse::record_table(&body, &meta, 31, &mut work, true)?;
        Ok(())
    })();
}
