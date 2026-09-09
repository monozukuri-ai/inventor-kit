//! Bounded MS-OLEPS property sets. FMTID/PID and original bytes remain authoritative.
//! Layout references and exact support are recorded in docs/format-reference.md.
use crate::{
    document::{Diagnostic, SourceSpan},
    read::Reader,
    Error, Limits,
};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binary(pub Vec<u8>);
impl Serialize for Binary {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex(&self.0))
    }
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
pub(crate) fn guid(b: &[u8]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{}-{}",
        u32::from_le_bytes(b[..4].try_into().unwrap()),
        u16::from_le_bytes(b[4..6].try_into().unwrap()),
        u16::from_le_bytes(b[6..8].try_into().unwrap()),
        hex(&b[8..10]),
        hex(&b[10..16])
    )
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ArrayDimension {
    pub size: u32,
    pub lower_bound: i32,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DictionaryEntry {
    pub pid: u32,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropertyValue {
    Empty,
    Null,
    Signed {
        value: i64,
    },
    Unsigned {
        value: u64,
    },
    Float {
        value: f64,
    },
    Bool {
        value: bool,
    },
    Text {
        value: String,
    },
    Guid {
        value: String,
    },
    Filetime {
        ticks_100ns: u64,
    },
    OleDate {
        days: f64,
    },
    Currency {
        scaled_value: i64,
    },
    Decimal {
        low: u64,
        high: u32,
        scale: u8,
        negative: bool,
    },
    Blob {
        data: Binary,
    },
    Clipboard {
        format: u32,
        data: Binary,
    },
    Sequence {
        element_type: u16,
        dimensions: Vec<ArrayDimension>,
        values: Vec<PropertyValue>,
    },
    Dictionary {
        entries: Vec<DictionaryEntry>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct Property {
    pub pid: u32,
    pub name: Option<String>,
    pub semantic_name: Option<&'static str>,
    pub type_code: Option<u16>,
    pub status: &'static str,
    pub value: Option<PropertyValue>,
    pub raw_data: Binary,
    pub source: SourceSpan,
}
#[derive(Debug, Clone, Serialize)]
pub struct PropertySet {
    pub fmtid: String,
    pub stream: String,
    pub version: u16,
    pub system_identifier: u32,
    pub clsid: String,
    pub code_page: Option<u16>,
    pub storage_scope: &'static str,
    pub state_binding: &'static str,
    pub status: &'static str,
    pub source: SourceSpan,
    pub properties: Vec<Property>,
}

#[derive(Debug)]
enum Failure {
    Invalid(String),
    Unsupported(String),
    Limit(String),
}
impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Self::Invalid(e.0)
    }
}
impl Failure {
    fn details(&self) -> (&'static str, &'static str, &str) {
        match self {
            Self::Invalid(s) => ("property.value_invalid", "malformed", s),
            Self::Unsupported(s) => ("property.value_unsupported", "unsupported", s),
            Self::Limit(s) => ("property.limit_exceeded", "limited", s),
        }
    }
}
type PResult<T> = Result<T, Failure>;
pub(crate) struct Budget {
    remaining: usize,
    depth: usize,
}
impl Budget {
    pub fn new(limits: &Limits) -> Self {
        Self {
            remaining: limits.max_property_items,
            depth: limits.max_property_depth,
        }
    }
    fn reserve(&mut self, n: usize) -> PResult<()> {
        self.remaining = self
            .remaining
            .checked_sub(n)
            .ok_or_else(|| Failure::Limit("document property item budget exhausted".into()))?;
        Ok(())
    }
}
fn invalid(message: &str) -> Failure {
    Failure::Invalid(message.into())
}
fn align(r: &mut Reader<'_>) -> PResult<()> {
    let n = (4 - r.pos % 4) % 4;
    if r.take(n)?.iter().any(|&b| b != 0) {
        return Err(invalid("nonzero typed-value padding"));
    }
    Ok(())
}
fn text(bytes: &[u8], code_page: Option<u16>) -> PResult<String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    let cp = code_page.ok_or_else(|| {
        Failure::Unsupported("missing/invalid code page; no locale default is assumed".into())
    })?;
    if cp == 1200 {
        if !bytes.len().is_multiple_of(2) || !bytes.ends_with(&[0, 0]) {
            return Err(invalid("invalid UTF-16 length or missing terminator"));
        }
        return String::from_utf16(
            &bytes[..bytes.len() - 2]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| invalid("invalid UTF-16 code units"));
    }
    if bytes.last() != Some(&0) {
        return Err(invalid("missing string terminator"));
    }
    let bytes = &bytes[..bytes.len() - 1];
    if cp == 65001 {
        return String::from_utf8(bytes.to_vec()).map_err(|_| invalid("invalid UTF-8"));
    }
    let encoding = match cp {
        1252 => encoding_rs::WINDOWS_1252,
        932 => encoding_rs::SHIFT_JIS,
        _ => return Err(Failure::Unsupported(format!("unsupported code page {cp}"))),
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|s| s.into_owned())
        .ok_or_else(|| invalid("invalid encoded string"))
}
fn string(r: &mut Reader<'_>, cp: Option<u16>, wide: bool) -> PResult<String> {
    let n = r.u32()? as usize;
    let n = n
        .checked_mul(if wide { 2 } else { 1 })
        .ok_or_else(|| invalid("string length overflow"))?;
    let value = text(r.take(n)?, if wide { Some(1200) } else { cp })?;
    align(r)?;
    Ok(value)
}
fn finite(value: f64) -> PResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(
            "nonfinite floating-point value retained only as raw bytes",
        ))
    }
}
fn scalar(
    r: &mut Reader<'_>,
    vt: u16,
    cp: Option<u16>,
    budget: &mut Budget,
    depth: usize,
) -> PResult<PropertyValue> {
    use PropertyValue::*;
    Ok(match vt {
        0 => Empty,
        1 => Null,
        2 => Signed {
            value: r.u16()? as i16 as i64,
        },
        3 | 22 => Signed {
            value: r.u32()? as i32 as i64,
        },
        4 => Float {
            value: finite(f32::from_bits(r.u32()?) as f64)?,
        },
        5 => Float {
            value: finite(f64::from_bits(r.u64()?))?,
        },
        6 => Currency {
            scaled_value: r.u64()? as i64,
        },
        7 => OleDate {
            days: finite(f64::from_bits(r.u64()?))?,
        },
        8 | 30 => Text {
            value: string(r, cp, false)?,
        },
        10 | 19 | 23 => Unsigned {
            value: r.u32()? as u64,
        },
        11 => Bool {
            value: match r.u16()? {
                0 => false,
                0xffff => true,
                _ => return Err(invalid("VARIANT_BOOL must be 0 or -1")),
            },
        },
        12 => return typed(r, cp, budget, depth + 1),
        14 => {
            if r.u16()? != 0 {
                return Err(invalid("nonzero DECIMAL reserved field"));
            }
            let scale = r.u8()?;
            let sign = r.u8()?;
            if scale > 28 || !matches!(sign, 0 | 128) {
                return Err(invalid("invalid DECIMAL scale/sign"));
            }
            Decimal {
                high: r.u32()?,
                low: r.u64()?,
                scale,
                negative: sign == 128,
            }
        }
        16 => Signed {
            value: r.u8()? as i8 as i64,
        },
        17 => Unsigned {
            value: r.u8()? as u64,
        },
        18 => Unsigned {
            value: r.u16()? as u64,
        },
        20 => Signed {
            value: r.u64()? as i64,
        },
        21 => Unsigned { value: r.u64()? },
        31 => Text {
            value: string(r, cp, true)?,
        },
        64 => Filetime {
            ticks_100ns: r.u64()?,
        },
        65 | 70 => {
            let n = r.u32()? as usize;
            let data = Binary(r.take(n)?.to_vec());
            align(r)?;
            Blob { data }
        }
        71 => {
            let n = r.u32()? as usize;
            let bytes = r.take(n)?;
            if n < 4 {
                return Err(invalid("clipboard size omits format"));
            }
            let value = Clipboard {
                format: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                data: Binary(bytes[4..].to_vec()),
            };
            align(r)?;
            value
        }
        72 => Guid {
            value: guid(r.take(16)?),
        },
        _ => {
            return Err(Failure::Unsupported(format!(
                "unsupported property type 0x{vt:04x}"
            )))
        }
    })
}
fn typed(
    r: &mut Reader<'_>,
    cp: Option<u16>,
    budget: &mut Budget,
    depth: usize,
) -> PResult<PropertyValue> {
    if depth > budget.depth {
        return Err(Failure::Limit("property nesting depth exceeded".into()));
    }
    budget.reserve(1)?;
    let vt = r.u16()?;
    if r.u16()? != 0 {
        return Err(invalid("nonzero property type padding"));
    }
    let flags = vt & 0xf000;
    let value = if matches!(flags, 0x1000 | 0x2000) {
        let element_type = vt & 0x0fff;
        let allowed = match flags {
            0x1000 => matches!(element_type, 2..=8 | 10..=12 | 16..=21 | 30 | 31 | 64 | 71 | 72),
            _ => matches!(element_type, 2..=8 | 10..=12 | 14 | 16..=19 | 22 | 23),
        };
        if !allowed {
            return Err(Failure::Unsupported(format!(
                "unsupported sequence type 0x{vt:04x}"
            )));
        }
        let mut dimensions = Vec::new();
        let count = if flags == 0x1000 {
            r.u32()? as usize
        } else {
            if r.u32()? != element_type as u32 {
                return Err(invalid("array element type mismatch"));
            }
            let n = r.u32()?;
            if !(1..=31).contains(&n) {
                return Err(invalid("array dimension count outside 1..31"));
            }
            let mut count = 1usize;
            for _ in 0..n {
                let size = r.u32()?;
                let lower_bound = r.u32()? as i32;
                count = count
                    .checked_mul(size as usize)
                    .ok_or_else(|| invalid("array element count overflow"))?;
                dimensions.push(ArrayDimension { size, lower_bound });
            }
            count
        };
        budget.reserve(count)?;
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(scalar(r, element_type, cp, budget, depth)?);
        }
        PropertyValue::Sequence {
            element_type,
            dimensions,
            values,
        }
    } else if flags == 0 && vt != 12 {
        scalar(r, vt, cp, budget, depth)?
    } else {
        return Err(Failure::Unsupported(format!(
            "unsupported property type 0x{vt:04x}"
        )));
    };
    align(r)?;
    Ok(value)
}
fn dictionary(r: &mut Reader<'_>, cp: Option<u16>, budget: &mut Budget) -> PResult<PropertyValue> {
    let n = r.u32()? as usize;
    budget.reserve(n)?;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for _ in 0..n {
        let pid = r.u32()?;
        let count = r.u32()? as usize;
        if pid < 2 || !ids.insert(pid) {
            return Err(invalid("invalid/duplicate dictionary identifier"));
        }
        let count = count
            .checked_mul(if cp == Some(1200) { 2 } else { 1 })
            .ok_or_else(|| invalid("dictionary length overflow"))?;
        let name = text(r.take(count)?, cp)?;
        if cp == Some(1200) {
            align(r)?;
        }
        entries.push(DictionaryEntry { pid, name });
    }
    align(r)?;
    Ok(PropertyValue::Dictionary { entries })
}

// Names in a dictionary are retained as source text. Semantic aliases use IDs,
// never localized display names. Modern private Inventor FMTIDs are not guessed.
fn semantic(fmtid: &str, pid: u32) -> Option<&'static str> {
    match (fmtid, pid) {
        ("32853f0f-3444-11d1-9e93-0060b03c1ca6", 5) => Some("part_number"),
        ("f29f85e0-4ff9-1068-ab91-08002b27b3d9", 2) => Some("title"),
        ("f29f85e0-4ff9-1068-ab91-08002b27b3d9", 3) => Some("subject"),
        ("f29f85e0-4ff9-1068-ab91-08002b27b3d9", 4) => Some("author"),
        ("f29f85e0-4ff9-1068-ab91-08002b27b3d9", 6) => Some("comments"),
        _ => None,
    }
}

pub(crate) fn parse(
    bytes: &[u8],
    source_id: &str,
    path: &str,
    budget: &mut Budget,
    diagnostics: &mut Vec<Diagnostic>,
) -> crate::Result<Vec<PropertySet>> {
    let mut r = Reader::new(bytes);
    if r.u16()? != 0xfffe {
        return Err(Error("unsupported property-set byte order".into()));
    }
    let version = r.u16()?;
    if version > 1 {
        return Err(Error(format!("unsupported property-set version {version}")));
    }
    let system_identifier = r.u32()?;
    let clsid = guid(r.take(16)?);
    let n = r.u32()? as usize;
    if !(1..=2).contains(&n) {
        return Err(Error(
            "property-set stream must contain one or two sections".into(),
        ));
    }
    let mut headers = Vec::new();
    let mut ids = BTreeSet::new();
    for _ in 0..n {
        let fmtid = guid(r.take(16)?);
        let offset = r.u32()? as usize;
        if offset < 28 + n * 20 || !offset.is_multiple_of(4) || !ids.insert(fmtid.clone()) {
            return Err(Error("invalid section offset or duplicate FMTID".into()));
        }
        let mut section = Reader::new(
            bytes
                .get(offset..)
                .ok_or_else(|| Error("section outside stream".into()))?,
        );
        let size = section.u32()? as usize;
        let end = offset
            .checked_add(size)
            .filter(|&end| end <= bytes.len() && size >= 8 && size.is_multiple_of(4))
            .ok_or_else(|| Error("invalid property section size".into()))?;
        headers.push((fmtid, offset, end));
    }
    let mut ranges = headers.iter().map(|(_, a, b)| (*a, *b)).collect::<Vec<_>>();
    ranges.sort_unstable();
    if ranges.windows(2).any(|p| p[0].1 > p[1].0) {
        return Err(Error("overlapping property sections".into()));
    }
    let mut sets = Vec::new();
    for (fmtid, start, end) in headers {
        let mut set = PropertySet {
            fmtid,
            stream: path.into(),
            version,
            system_identifier,
            clsid: clsid.clone(),
            code_page: None,
            storage_scope: if path.trim_start_matches('/').contains('/') {
                "nested_storage"
            } else {
                "document_storage"
            },
            state_binding: "unresolved",
            status: "decoded",
            source: SourceSpan::stream(source_id, path, start, end),
            properties: Vec::new(),
        };
        if let Err(error) = section(&bytes[start..end], &mut set, budget, diagnostics) {
            let (code, status, message) = error.details();
            set.status = status;
            set.properties.clear();
            diagnostics.push(
                Diagnostic::new(
                    if status == "malformed" {
                        "property.section_invalid"
                    } else {
                        code
                    },
                    "warning",
                    message,
                    Some(set.source.clone()),
                )
                .property(&set.fmtid, None),
            );
        }
        sets.push(set);
    }
    Ok(sets)
}
fn section(
    bytes: &[u8],
    set: &mut PropertySet,
    budget: &mut Budget,
    diagnostics: &mut Vec<Diagnostic>,
) -> PResult<()> {
    let mut r = Reader::new(bytes);
    r.skip(4)?;
    let n = r.u32()? as usize;
    budget.reserve(n)?;
    if n > bytes.len().saturating_sub(8) / 8 {
        return Err(invalid("property table exceeds section"));
    }
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    let mut offsets = BTreeSet::new();
    for _ in 0..n {
        let pid = r.u32()?;
        let offset = r.u32()? as usize;
        if !ids.insert(pid)
            || !offsets.insert(offset)
            || offset < 8 + n * 8
            || !offset.is_multiple_of(4)
            || offset >= bytes.len()
        {
            return Err(invalid(
                "duplicate property identifier/offset or invalid property boundary",
            ));
        }
        entries.push((offset, pid));
    }
    entries.sort_unstable();
    // Resolve code page before dictionary or string values, regardless of table order.
    if let Some(&(off, _)) = entries.iter().find(|(_, pid)| *pid == 1) {
        let end = offsets
            .range(off + 1..)
            .next()
            .copied()
            .unwrap_or(bytes.len());
        let value = &bytes[off..end];
        if value.len() >= 8 && value[..4] == [2, 0, 0, 0] && value[6..8] == [0, 0] {
            set.code_page = Some(u16::from_le_bytes([value[4], value[5]]));
        }
    }
    if set.code_page.is_none() {
        set.status = "partial";
        diagnostics.push(
            Diagnostic::new(
                "property.code_page_missing",
                "warning",
                "Missing/invalid CodePage; no fallback encoding is assumed",
                Some(set.source.clone()),
            )
            .property(&set.fmtid, Some(1)),
        );
    }
    let mut names = BTreeMap::new();
    for (i, &(offset, pid)) in entries.iter().enumerate() {
        let end = entries.get(i + 1).map(|e| e.0).unwrap_or(bytes.len());
        let raw = &bytes[offset..end];
        let source = SourceSpan::stream(
            &set.source.source_id,
            &set.stream,
            set.source.start_offset + offset,
            set.source.start_offset + end,
        );
        let type_code = if pid == 0 {
            None
        } else {
            raw.get(..2).map(|b| u16::from_le_bytes([b[0], b[1]]))
        };
        let mut reader = Reader::new(raw);
        let decoded = if pid == 1 && set.code_page.is_none() {
            Err(invalid("CodePage property must be a padded VT_I2"))
        } else if set.version == 0 && type_code.is_some_and(|t| t & 0x2000 != 0) {
            Err(Failure::Unsupported(
                "array property requires property-set version 1".into(),
            ))
        } else if pid == 0 {
            dictionary(&mut reader, set.code_page, budget)
        } else {
            typed(&mut reader, set.code_page, budget, 0)
        };
        let (status, value) = match decoded {
            Ok(v) => {
                if let PropertyValue::Dictionary { entries } = &v {
                    for entry in entries {
                        names.insert(entry.pid, entry.name.clone());
                    }
                }
                ("decoded", Some(v))
            }
            Err(e) => {
                let (code, status, message) = e.details();
                set.status = "partial";
                diagnostics.push(
                    Diagnostic::new(code, "warning", message, Some(source.clone()))
                        .property(&set.fmtid, Some(pid)),
                );
                (status, None)
            }
        };
        set.properties.push(Property {
            pid,
            name: None,
            semantic_name: semantic(&set.fmtid, pid),
            type_code,
            status,
            value,
            raw_data: Binary(raw.to_vec()),
            source,
        });
    }
    for prop in &mut set.properties {
        prop.name = names.remove(&prop.pid);
    }
    Ok(())
}
