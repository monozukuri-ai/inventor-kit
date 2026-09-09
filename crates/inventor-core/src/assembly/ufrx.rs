//! Exact UFRx15 section profile observed in the pinned SampleBg corpus.
//! Layout reference: cadmpeg external_reference.rs, Apache-2.0 (see notices).
use crate::{document::SourceSpan, property::guid, read::Reader, Error, Limits, Result};
use serde::Serialize;

const SECTIONS: [u16; 27] = [
    31, 32, 18, 28, 3, 12, 7, 4, 1, 5, 1, 2, 7, 2, 3, 7, 0, 5, 5, 0, 1, 0, 1, 0, 3, 2, 2,
];

#[derive(Debug, Clone, Serialize, Default)]
pub struct UfrxDocument {
    pub status: String,
    pub schema: Option<u16>,
    pub sections: Vec<u16>,
    pub save_version: Option<[u8; 8]>,
    pub document_id: Option<String>,
    pub database_revision_id: Option<String>,
    pub original_filename: Option<String>,
    pub active_representation: Option<[String; 2]>,
    pub active_model_state: Option<String>,
    pub secondary_representation: Option<String>,
    pub model_states: Vec<String>,
    pub model_state_records: Vec<SourceSpan>,
    pub reference_count: Option<usize>,
    pub occurrence_count: Option<usize>,
    pub references: Vec<ExternalReference>,
    pub embedded_references: Vec<SourceSpan>,
    pub occurrences: Vec<StoredOccurrence>,
    pub opaque_tail: Option<SourceSpan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalReference {
    pub path: String,
    pub library_id: i32,
    pub library_name: String,
    pub display_name: String,
    pub prefix_state: u16,
    pub state_groups: Vec<[u16; 3]>,
    pub state: [u16; 2],
    pub document_id: String,
    pub database_id: String,
    pub reference_id: u32,
    pub occurrence_count: u32,
    pub version: u32,
    pub flags: u32,
    pub member_name: Option<String>,
    pub member_factory_reference: Option<u32>,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredOccurrence {
    pub end_string_flag: u32,
    pub reference_id: u32,
    pub occurrence_id: u32,
    pub header_value: u32,
    pub title: Option<String>,
    /// Typed values with unknown semantics; tags are not visibility/name/state APIs.
    pub properties: Vec<OccurrenceProperty>,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Serialize)]
pub struct OccurrenceProperty {
    pub section_index: u8,
    pub section_header: u32,
    pub flag: bool,
    pub tag: u8,
    pub header_value: u32,
    pub trailer_value: u32,
    pub value: PropertyValue,
    pub source: SourceSpan,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PropertyValue {
    String(String),
    Byte(u8),
    Integer(u32),
    Bytes16([u8; 16]),
}

pub(super) fn require(actual: u32, expected: u32) -> Result<()> {
    if actual != expected {
        return Err(Error(format!("expected {expected:#x}, got {actual:#x}")));
    }
    Ok(())
}
pub(super) fn peek(r: &Reader<'_>) -> Result<u32> {
    let mut copy = Reader {
        bytes: r.bytes,
        pos: r.pos,
    };
    copy.u32()
}

// All nested collections share one item budget, including skipped properties.
fn count(r: &mut Reader<'_>, budget: &mut usize) -> Result<usize> {
    let n = r.count(*budget)?;
    *budget -= n;
    Ok(n)
}
fn boolean(r: &mut Reader<'_>) -> Result<bool> {
    let n = r.u8()?;
    if n > 1 {
        return Err(Error("invalid UFRx boolean".into()));
    }
    Ok(n == 1)
}

pub(super) fn parse(
    r: &mut Reader<'_>,
    source: &str,
    kind: &str,
    limits: &Limits,
    out: &mut UfrxDocument,
) -> Result<()> {
    out.status = "unavailable".into();
    let schema = r.u16()?;
    out.schema = Some(schema);
    let n = r.u16()? as usize;
    if n > 256 {
        return Err(Error("UFRx section count limit exceeded".into()));
    }
    for _ in 0..n {
        out.sections.push(r.u16()?);
    }
    if schema != 15 || out.sections != SECTIONS {
        return Err(Error(
            "unsupported UFRx schema/section profile (only observed schema 15 / major 31 profile)"
                .into(),
        ));
    }
    let version: [u8; 8] = r.take(8)?.try_into().unwrap();
    out.save_version = Some(version);
    require(version[2] as u32, 31)?;
    r.skip(24)?;
    r.utf16()?;
    r.skip(32)?;
    out.database_revision_id = Some(guid(&r.id()?));
    r.u32()?;
    out.document_id = Some(guid(&r.id()?));
    out.original_filename = Some(r.utf16()?);
    r.u16()?;
    // M5 needs part identity, not an unverified interpretation of the part's
    // representation branch. The remainder is retained by opaque_tail.
    if kind == "part" {
        out.status = "identity_only".into();
        return Ok(());
    }
    let mut budget = limits.max_property_items.min(limits.max_records);
    for _ in 0..count(r, &mut budget)? {
        r.skip(4)?;
        r.utf16()?;
        r.skip(2)?;
    }
    for _ in 0..count(r, &mut budget)? {
        r.utf16()?;
        r.utf16()?;
    }
    r.skip(4)?;
    r.u16()?;
    if kind == "assembly" {
        out.active_representation = Some([r.utf16()?, r.utf16()?]);
    }
    r.skip(4)?;
    out.secondary_representation = Some(r.utf16()?);
    r.skip(4)?;
    // The observed major-31 IAM profile has an additional representation word;
    // the reference implementation's synthetic schema-15 header omits it.
    require(r.u32()?, 2)?;
    r.u32()?;
    r.u16()?;
    require(r.u16()? as u32, 1)?;
    r.skip(8 + 32)?;
    for _ in 0..count(r, &mut budget)? {
        let start = r.pos;
        r.u8()?;
        out.model_states.push(r.utf16()?);
        r.skip(8)?;
        for _ in 0..count(r, &mut budget)? {
            r.utf16()?;
            r.skip(5)?;
            r.utf16()?;
            r.u16()?;
        }
        // 81-byte suffix in this observed profile; contents remain uninterpreted.
        r.skip(81)?;
        out.model_state_records
            .push(SourceSpan::stream(source, "/UFRxDoc", start, r.pos));
    }
    let n = count(r, &mut budget)?;
    out.reference_count = Some(n);
    r.utf16()?;
    r.u32()?;
    for _ in 0..n {
        let start = r.pos;
        let path = r.utf16()?;
        let library_id = r.u32()? as i32;
        let library_name = r.utf16()?;
        let prefix_state = r.u16()?;
        if peek(r)? & 0xffff0000 != 0 {
            require(r.u16()? as u32, 0)?;
        }
        let display_name = r.utf16()?;
        let mut state_groups = vec![];
        for _ in 0..count(r, &mut budget)? {
            state_groups.push([r.u16()?, r.u16()?, r.u16()?]);
        }
        let state = [r.u16()?, r.u16()?];
        let document_id = guid(&r.id()?);
        let database_id = guid(&r.id()?);
        let reference_id = r.u32()?;
        let occurrence_count = r.u32()?;
        let version = r.u32()?;
        let flags = r.u32()?;
        if !matches!(flags, 1 | 16 | 33) {
            return Err(Error(format!("unsupported reference flags {flags:#x}")));
        }
        let member_name = if flags == 0x21 {
            Some(r.utf16()?)
        } else {
            None
        };
        let member_factory_reference = if flags == 0x21 { Some(r.u32()?) } else { None };
        out.references.push(ExternalReference {
            path,
            library_id,
            library_name,
            display_name,
            prefix_state,
            state_groups,
            state,
            document_id,
            database_id,
            reference_id,
            occurrence_count,
            version,
            flags,
            member_name,
            member_factory_reference,
            source: SourceSpan::stream(source, "/UFRxDoc", start, r.pos),
        });
    }
    require(r.u8()? as u32, 0)?;
    for _ in 0..count(r, &mut budget)? {
        let start = r.pos;
        r.skip(24)?;
        r.utf16()?;
        r.u32()?;
        r.utf16()?;
        r.u16()?;
        r.utf16()?;
        r.skip(8)?;
        out.embedded_references
            .push(SourceSpan::stream(source, "/UFRxDoc", start, r.pos));
    }
    require(r.u8()? as u32, 0)?;
    let n = count(r, &mut budget)?;
    out.occurrence_count = Some(n);
    for _ in 0..n {
        let start = r.pos;
        let end_string_flag = r.u32()?;
        let reference_id = r.u32()?;
        let occurrence_id = r.u32()?;
        let header_value = r.u32()?;
        let marker = r.count(65536)?;
        let title = if marker == 0 { None } else { Some(r.utf16()?) };
        // Preserve a proved occurrence header even if its property tail fails.
        out.occurrences.push(StoredOccurrence {
            end_string_flag,
            reference_id,
            occurrence_id,
            header_value,
            title,
            properties: vec![],
            source: SourceSpan::stream(source, "/UFRxDoc", start, r.pos),
        });
        let mut padding = 0;
        loop {
            let marker = r.u16()?;
            if marker != 0 {
                require(marker as u32, 0x2080)?;
                break;
            }
            padding += 1;
            if padding > 8 {
                return Err(Error("excessive occurrence padding".into()));
            }
        }
        require(r.u32()?, 0)?;
        require(r.u32()?, 1)?;
        require(r.u32()?, 0)?;
        for section_index in 0..2 {
            let section_header = r.u32()?;
            for _ in 0..count(r, &mut budget)? {
                let start = r.pos;
                let flag = boolean(r)?;
                let tag = r.u8()?;
                let header_value = r.u32()?;
                require(r.u8()? as u32, tag as u32)?;
                let value = value(r, tag, false)?;
                let trailer_value = r.u32()?;
                let occurrence = out.occurrences.last_mut().unwrap();
                occurrence.properties.push(OccurrenceProperty {
                    section_index,
                    section_header,
                    flag,
                    tag,
                    header_value,
                    trailer_value,
                    value,
                    source: SourceSpan::stream(source, "/UFRxDoc", start, r.pos),
                });
                occurrence.source.end_offset = r.pos;
            }
        }
        for _ in 0..count(r, &mut budget)? {
            r.utf16()?;
            r.skip(16)?;
            r.text()?;
        }
        r.skip(11)?;
        let n = peek(r)?;
        let next = {
            let mut copy = Reader {
                bytes: r.bytes,
                pos: r.pos + 4,
            };
            copy.u32()?
        };
        if matches!(n, 0x00ffffff | u32::MAX) {
            r.u32()?;
            require(r.u32()?, 0)?;
        } else if n > 1 || (n == 1 && next > 1) {
            if next > 0xffff {
                r.utf16()?;
                r.skip(16)?;
                r.text()?;
            } else {
                for _ in 0..count(r, &mut budget)? {
                    r.utf16()?;
                    let n = count(r, &mut budget)?;
                    require(r.u32()?, n as u32)?;
                    for _ in 0..n {
                        boolean(r)?;
                        let tag = r.u8()?;
                        for _ in 0..count(r, &mut budget)? {
                            require(r.u8()? as u32, tag as u32)?;
                            value(r, tag, true)?;
                        }
                        r.u32()?;
                    }
                    r.skip(13)?;
                }
            }
        } else {
            r.u32()?;
        }
        out.occurrences.last_mut().unwrap().source.end_offset = r.pos;
    }
    if n == 0 {
        require(r.u32()?, 0)?;
    }
    out.status = "decoded_subset".into();
    Ok(())
}

fn value(r: &mut Reader<'_>, tag: u8, item: bool) -> Result<PropertyValue> {
    if item
        && !matches!(
            tag,
            0x07 | 0x19 | 0x12 | 0x16 | 0x17 | 0x18 | 0x23 | 0x24 | 0x25 | 0x2a
        )
    {
        return Err(Error(format!("unsupported occurrence export tag {tag:#x}")));
    }
    Ok(match tag {
        0x05 | 0x1e => PropertyValue::String(r.utf16()?),
        0x07 | 0x0d | 0x0f | 0x10 | 0x1d => PropertyValue::Byte(r.u8()?),
        0x19 => PropertyValue::Integer(r.u32()?),
        0x02
        | 0x03
        | 0x11..=0x13
        | 0x15..=0x18
        | 0x1c
        | 0x1f
        | 0x20
        | 0x22..=0x25
        | 0x2a..=0x2e => PropertyValue::Bytes16(r.id()?),
        _ => {
            return Err(Error(format!(
                "unsupported occurrence property tag {tag:#x}"
            )))
        }
    })
}
