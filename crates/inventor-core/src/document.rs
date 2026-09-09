//! Document facts independent of kernel decoding or model-state evaluation.
use crate::{
    property::{self, Binary, Budget, PropertySet},
    read::Reader,
    rse, stream, Error, Limits, Result, StreamInfo,
};
use serde::Serialize;
use std::io::Cursor;

#[derive(Debug, Clone, Serialize)]
pub struct SourceSpan {
    pub source_id: String,
    pub stream: String,
    pub byte_domain: &'static str,
    pub start_offset: usize,
    pub end_offset: usize,
}
impl SourceSpan {
    pub fn stream(source_id: &str, stream: &str, start: usize, end: usize) -> Self {
        Self {
            source_id: source_id.into(),
            stream: stream.into(),
            byte_domain: "cfb_stream",
            start_offset: start,
            end_offset: end,
        }
    }
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct FormatProfile {
    pub rse_db_schema: Option<u32>,
    pub segment_major: Option<u8>,
    pub sab_version: Option<u32>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: &'static str,
    pub message: String,
    pub source: Option<SourceSpan>,
    pub record: Option<usize>,
    pub fmtid: Option<String>,
    pub pid: Option<u32>,
    pub profile: Option<FormatProfile>,
}
impl Diagnostic {
    pub fn new(
        code: &str,
        severity: &'static str,
        message: impl Into<String>,
        source: Option<SourceSpan>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            source,
            record: None,
            fmtid: None,
            pid: None,
            profile: None,
        }
    }
    pub fn property(mut self, fmtid: &str, pid: Option<u32>) -> Self {
        self.fmtid = Some(fmtid.into());
        self.pid = pid;
        self
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Identification {
    pub kind: String,
    pub status: &'static str,
    pub basis: &'static str,
    pub root_clsid: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseInfo {
    pub stream: String,
    pub schema: Option<u32>,
    pub status: &'static str,
    pub name: Option<String>,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, Serialize)]
pub struct SegmentInfo {
    pub name: String,
    pub kind: String,
    pub id: String,
    pub major: u8,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, Serialize)]
pub struct UnparsedPropertyStream {
    pub source: SourceSpan,
    pub status: &'static str,
    pub raw_data: Option<Binary>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Stages {
    pub identification: &'static str,
    pub properties: &'static str,
    pub registry: &'static str,
    pub geometry: &'static str,
    pub conversion: &'static str,
    pub state: &'static str,
    pub references: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct DocumentInfo {
    pub api_version: u32,
    pub identification: Identification,
    pub databases: Vec<DatabaseInfo>,
    pub segments: Vec<SegmentInfo>,
    pub property_sets: Vec<PropertySet>,
    pub unparsed_property_streams: Vec<UnparsedPropertyStream>,
    pub thumbnails: Vec<crate::thumbnail::Thumbnail>,
    pub diagnostics: Vec<Diagnostic>,
    pub stages: Stages,
    pub geometry_profile: FormatProfile,
}
pub(crate) fn root_kind(clsid: &str) -> Option<&'static str> {
    // Exact CLSIDs observed in the pinned IPT/IAM/IDW/IPN corpus. Extensions are
    // not used to classify a document; conflicting registry evidence is reported.
    match clsid {
        "4d29b490-49b2-11d0-93c3-7e0706000000" => Some("part"),
        "e60f81e1-49b3-11d0-93c3-7e0706000000" => Some("assembly"),
        "bbf9fdf1-52dc-11d0-8c04-0800090be8ec" => Some("drawing"),
        "76283a80-50dd-11d3-a7e3-00c04f79d7bc" => Some("presentation"),
        _ => None,
    }
}
fn database_paths(streams: &[StreamInfo]) -> Vec<&StreamInfo> {
    streams
        .iter()
        .filter(|s| {
            let parts = s.path.split('/').collect::<Vec<_>>();
            parts.len() == 4
                && parts[1] == "RSeStorage"
                && parts[3] == "RSeDb"
                && parts[2]
                    .strip_prefix('V')
                    .is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
        })
        .collect()
}
fn property_path(path: &str) -> bool {
    let parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    parts.last().is_some_and(|s| s.starts_with('\u{5}'))
        || (parts.last() == Some(&"CONTENTS")
            && parts.len() > 1
            && parts[parts.len() - 2].starts_with('\u{5}'))
}
pub(crate) fn inspect(
    file: &mut cfb::CompoundFile<Cursor<&[u8]>>,
    source_id: &str,
    root_clsid: &str,
    streams: &[StreamInfo],
    limits: &Limits,
) -> (DocumentInfo, Vec<rse::Segment>) {
    let kind = root_kind(root_clsid);
    let mut info = DocumentInfo {
        api_version: 1,
        identification: Identification {
            kind: kind.unwrap_or("unknown").into(),
            status: if kind.is_some() {
                "identified"
            } else {
                "unknown"
            },
            basis: if kind.is_some() {
                "observed_root_clsid"
            } else {
                "unresolved"
            },
            root_clsid: root_clsid.into(),
        },
        databases: Vec::new(),
        segments: Vec::new(),
        property_sets: Vec::new(),
        unparsed_property_streams: Vec::new(),
        thumbnails: Vec::new(),
        diagnostics: Vec::new(),
        stages: Stages {
            identification: "unknown",
            properties: "missing",
            registry: "unavailable",
            geometry: "not_attempted",
            conversion: "not_attempted",
            state: "unresolved",
            references: "not_decoded",
        },
        geometry_profile: FormatProfile::default(),
    };
    let mut property_bytes = 0usize;
    let mut budget = Budget::new(limits);
    for s in streams.iter().filter(|s| property_path(&s.path)) {
        let span = SourceSpan::stream(source_id, &s.path, 0, s.bytes as usize);
        let data = if s.bytes as u128 + property_bytes as u128 > limits.max_property_bytes as u128 {
            Err(Error(
                "aggregate property-stream byte limit exceeded".into(),
            ))
        } else {
            property_bytes += s.bytes as usize;
            stream(file, &s.path, limits.max_stream_bytes)
        };
        match data {
            Err(e) => {
                info.unparsed_property_streams.push(UnparsedPropertyStream {
                    source: span.clone(),
                    status: "unavailable",
                    raw_data: None,
                });
                info.diagnostics.push(Diagnostic::new(
                    "property.stream_unavailable",
                    "warning",
                    e.to_string(),
                    Some(span),
                ));
            }
            Ok(data)
                if data.len() >= 4
                    && data[..2] == [0xfe, 0xff]
                    && u16::from_le_bytes([data[2], data[3]]) > 1 =>
            {
                info.diagnostics.push(Diagnostic::new(
                    "property.profile_unsupported",
                    "warning",
                    format!(
                        "Unsupported property-set version {}",
                        u16::from_le_bytes([data[2], data[3]])
                    ),
                    Some(span.clone()),
                ));
                info.unparsed_property_streams.push(UnparsedPropertyStream {
                    source: span,
                    status: "unsupported",
                    raw_data: Some(Binary(data)),
                });
            }
            Ok(data) => match property::parse(
                &data,
                source_id,
                &s.path,
                &mut budget,
                &mut info.diagnostics,
            ) {
                Ok(mut sets) => info.property_sets.append(&mut sets),
                Err(e) => {
                    info.unparsed_property_streams.push(UnparsedPropertyStream {
                        source: span.clone(),
                        status: "malformed",
                        raw_data: Some(Binary(data)),
                    });
                    info.diagnostics.push(Diagnostic::new(
                        "property.stream_invalid",
                        "warning",
                        e.to_string(),
                        Some(span),
                    ));
                }
            },
        }
    }
    info.stages.properties = if !info.unparsed_property_streams.is_empty()
        || info.property_sets.iter().any(|s| s.status != "decoded")
    {
        "partial"
    } else if info.property_sets.is_empty() {
        "missing"
    } else {
        "decoded"
    };
    if info.stages.properties == "missing" {
        info.diagnostics.push(Diagnostic::new(
            "property.sets_missing",
            "info",
            "No property-set streams are present",
            None,
        ));
    }
    for set in &info.property_sets {
        for prop in &set.properties {
            match crate::thumbnail::extract(set, prop) {
                Ok(Some(thumbnail)) => info.thumbnails.push(thumbnail),
                Err(e) => info.diagnostics.push(
                    Diagnostic::new(
                        "thumbnail.unsupported_or_invalid",
                        "warning",
                        e.to_string(),
                        Some(prop.source.clone()),
                    )
                    .property(&set.fmtid, Some(prop.pid)),
                ),
                _ => {}
            }
        }
    }
    for db in database_paths(streams) {
        let mut entry = DatabaseInfo {
            stream: db.path.clone(),
            schema: None,
            status: "unavailable",
            name: None,
            source: SourceSpan::stream(source_id, &db.path, 0, db.bytes as usize),
        };
        let parsed: Result<()> = (|| {
            let data = stream(file, &db.path, limits.max_stream_bytes)?;
            let mut r = Reader::new(&data);
            r.skip(16)?;
            let schema = r.u32()?;
            entry.schema = Some(schema);
            if schema != 31 {
                entry.status = "unsupported";
                return Err(Error(format!("unsupported RSeDb schema {schema}")));
            }
            r.skip(32)?;
            entry.name = Some(r.utf16()?);
            r.finish()?;
            entry.status = "decoded";
            Ok(())
        })();
        if let Err(e) = parsed {
            info.diagnostics.push(Diagnostic::new(
                "rse.database_unavailable",
                "warning",
                e.to_string(),
                Some(entry.source.clone()),
            ));
        }
        info.databases.push(entry);
    }
    let mut registry = Vec::new();
    // A shared grammar permits inventory, not selection of a database/state.
    // Every database must be present and decoded; do not ignore failed entries.
    if !info.databases.is_empty()
        && info
            .databases
            .iter()
            .all(|db| db.status == "decoded" && db.schema == Some(31))
    {
        info.geometry_profile.rse_db_schema = Some(31);
        let path = "/RSeStorage/RSeSegInfo";
        let source = streams
            .iter()
            .find(|s| s.path == path)
            .map(|s| SourceSpan::stream(source_id, path, 0, s.bytes as usize));
        match stream(file, path, limits.max_stream_bytes).and_then(|b| rse::registry(&b)) {
            Ok(entries) => {
                info.stages.registry = "decoded";
                for entry in &entries {
                    info.segments.push(SegmentInfo {
                        name: entry.name.clone(),
                        kind: entry.kind.clone(),
                        id: property::guid(&entry.id),
                        major: entry.major,
                        source: SourceSpan::stream(
                            source_id,
                            path,
                            entry.start_offset,
                            entry.end_offset,
                        ),
                    });
                }
                let registry_kind = if entries.iter().any(|s| s.kind == "AmDcSegmentType") {
                    Some("assembly")
                } else if entries.iter().any(|s| s.kind == "PmBrepSegmentType") {
                    Some("part")
                } else {
                    None
                };
                if let Some(registry_kind) = registry_kind {
                    if kind.is_some_and(|k| k != registry_kind) {
                        info.identification.kind = "unknown".into();
                        info.identification.status = "conflicting";
                        info.diagnostics.push(Diagnostic::new("document.kind_conflict", "warning", "Root CLSID and registry document kind disagree; geometry is not admitted", source));
                    } else if kind.is_none() {
                        info.identification.kind = registry_kind.into();
                        info.identification.status = "identified";
                        info.identification.basis = "rse_registry";
                    }
                }
                registry = entries;
                if info.databases.len() > 1 {
                    info.diagnostics.push(Diagnostic::new(
                        "rse.database_binding_unresolved", "warning",
                        "Multiple schema-31 databases share the registry grammar; no database or active state is selected", None));
                }
            }
            Err(e) => info.diagnostics.push(Diagnostic::new(
                "rse.registry_unavailable",
                "warning",
                e.to_string(),
                source,
            )),
        }
    } else {
        info.diagnostics.push(Diagnostic::new(
            "rse.database_selection_unavailable",
            "warning",
            format!(
                "Registry requires every RSeDb to use the supported schema; found {} database candidates",
                info.databases.len()
            ),
            None,
        ));
    }
    info.stages.identification = info.identification.status;
    info.diagnostics.push(Diagnostic::new("document.state_unresolved", "info", "Properties are stored observations. Effective Model State values and all-state inheritance have not been established", None));
    (info, registry)
}
