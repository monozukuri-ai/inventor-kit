//! Bounded saved IAM structure. UFRx/AmDc/AmGraphics layout observations:
//! cadmpeg faa73bf (Apache-2.0); see THIRD_PARTY_NOTICES.md.
//! Saved graphics placement is not proof of the current Inventor Model State.
mod matrix;
mod records;
mod resolve;
mod ufrx;

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_ufrx(reader: &mut crate::read::Reader<'_>, limits: &crate::Limits) {
    let _ = ufrx::parse(
        reader,
        "fuzz",
        "assembly",
        limits,
        &mut UfrxDocument::default(),
    );
}

pub use matrix::{compose, Matrix, IDENTITY};
pub use resolve::{resolve_file, AssemblyGraph, Definition, Instance, ResolveOptions};
pub use ufrx::{
    ExternalReference, OccurrenceProperty, PropertyValue, StoredOccurrence, UfrxDocument,
};

use crate::{
    document::{Diagnostic, SourceSpan},
    Error, Limits, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Cursor;

#[derive(Debug, Clone, Serialize)]
pub struct Occurrence {
    pub occurrence_id: u32,
    pub reference_id: u32,
    pub name: Option<String>,
    pub suppressed: Option<bool>,
    pub visible: Option<bool>,
    pub substitute: Option<bool>,
    pub status: String,
    pub local_transform_mm: Option<Matrix>,
    pub source: SourceSpan,
    pub identity: Option<records::Identity>,
    pub placement: Option<records::Placement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssemblyDocument {
    pub api_version: u32,
    pub source_id: String,
    pub source_sha256: String,
    pub kind: String,
    pub status: String,
    pub current_state: String,
    pub ufrx: UfrxDocument,
    pub occurrences: Vec<Occurrence>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(crate) fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Inventory saved references and placements. This does not load external files
/// or inflate part B-reps. Unsupported records remain explicit diagnostics.
pub fn inspect(data: &[u8], source_id: &str, limits: &Limits) -> Result<AssemblyDocument> {
    let metadata = crate::inspect(data, source_id, limits)?;
    let summary = &metadata.summary;
    let mut doc = AssemblyDocument {
        api_version: 1,
        source_id: source_id.into(),
        source_sha256: digest(data),
        kind: summary.kind.clone(),
        status: "unavailable".into(),
        current_state: "unverified".into(),
        ufrx: UfrxDocument::default(),
        occurrences: vec![],
        diagnostics: vec![],
    };
    if !matches!(doc.kind.as_str(), "part" | "assembly")
        || summary.document.identification.status == "conflicting"
    {
        doc.issue(
            "assembly.document_kind",
            "only IPT/IAM saved references are admitted",
            None,
        );
        return Ok(doc);
    }
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| Error(e.to_string()))?;
    match crate::stream(
        &mut file,
        "/UFRxDoc",
        limits.max_stream_bytes.min(limits.max_property_bytes),
    ) {
        Ok(bytes) => {
            let mut reader = crate::read::Reader::new(&bytes);
            if let Err(e) = ufrx::parse(&mut reader, source_id, &doc.kind, limits, &mut doc.ufrx) {
                doc.issue(
                    "assembly.ufrx_unavailable",
                    e.to_string(),
                    Some(SourceSpan::stream(
                        source_id,
                        "/UFRxDoc",
                        reader.pos,
                        bytes.len(),
                    )),
                );
            }
            doc.ufrx.opaque_tail = Some(SourceSpan::stream(
                source_id,
                "/UFRxDoc",
                reader.pos,
                bytes.len(),
            ));
        }
        Err(e) => doc.issue("assembly.ufrx_unavailable", e.to_string(), None),
    }
    if doc.kind == "assembly" && doc.ufrx.status == "decoded_subset" {
        match records::scan(&mut file, summary, limits, &mut doc.diagnostics) {
            Ok((identities, placements)) => records::join(&mut doc, identities, placements),
            Err(e) => doc.issue("assembly.records_unavailable", e.to_string(), None),
        }
    }
    // Even failed AmDc/graphics decoding must leave every stored occurrence visible.
    if doc.occurrences.is_empty() && !doc.ufrx.occurrences.is_empty() {
        for o in &doc.ufrx.occurrences {
            doc.occurrences.push(Occurrence {
                occurrence_id: o.occurrence_id,
                reference_id: o.reference_id,
                name: o.title.clone(),
                suppressed: None,
                visible: None,
                substitute: None,
                status: "unresolved_identity".into(),
                local_transform_mm: None,
                source: o.source.clone(),
                identity: None,
                placement: None,
            });
        }
    }
    doc.status = if !matches!(doc.ufrx.status.as_str(), "decoded_subset" | "identity_only") {
        "unavailable"
    } else if !doc.diagnostics.is_empty() || doc.occurrences.iter().any(|o| o.status != "resolved")
    {
        "partial"
    } else {
        "decoded_subset"
    }
    .into();
    Ok(doc)
}

impl AssemblyDocument {
    fn issue(&mut self, code: &str, message: impl Into<String>, source: Option<SourceSpan>) {
        self.diagnostics
            .push(Diagnostic::new(code, "warning", message, source));
    }
}

#[cfg(test)]
mod tests;
