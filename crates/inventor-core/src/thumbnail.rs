//! Exact observed Inventor PNG thumbnail carrier; never a signature scan.
use crate::{
    document::SourceSpan,
    property::{Binary, Property, PropertySet, PropertyValue},
    Error, Result,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Thumbnail {
    pub mime_type: &'static str,
    pub data: Binary,
    pub width: u32,
    pub height: u32,
    pub source: SourceSpan,
    pub fmtid: String,
    pub pid: u32,
    pub profile: &'static str,
    pub validation: &'static str,
    pub state_binding: &'static str,
}
pub(crate) fn extract(set: &PropertySet, prop: &Property) -> Result<Option<Thumbnail>> {
    if set.fmtid != "3d38de39-0588-4c14-bb37-18f4d5dd31c7" || prop.pid != 17 {
        return Ok(None);
    }
    let Some(PropertyValue::Clipboard {
        format: 0xffff_ffff,
        data,
    }) = &prop.value
    else {
        return Err(Error(
            "thumbnail property is not the admitted Windows clipboard carrier".into(),
        ));
    };
    let b = &data.0;
    // Observed carrier: clipboard ID 3, 8-byte legacy prefix, then PNG. This is
    // not interpreted as a general CF_METAFILEPICT image or a Windows handle.
    if b.len() < 12 || b[..6] != [3, 0, 0, 0, 8, 0] || b[10..12] != [0, 0] {
        return Err(Error("unknown Inventor thumbnail wrapper".into()));
    }
    let width = u16::from_le_bytes([b[6], b[7]]) as u32;
    let height = u16::from_le_bytes([b[8], b[9]]) as u32;
    let png = &b[12..];
    let dimensions = validate_png(png)?;
    if dimensions != (width, height) {
        return Err(Error("thumbnail wrapper/PNG dimensions disagree".into()));
    }
    let start = prop.source.start_offset + 24;
    Ok(Some(Thumbnail {
        mime_type: "image/png",
        data: Binary(png.to_vec()),
        width,
        height,
        source: SourceSpan::stream(
            &prop.source.source_id,
            &prop.source.stream,
            start,
            start + png.len(),
        ),
        fmtid: set.fmtid.clone(),
        pid: prop.pid,
        profile: "inventor-clipboard3-prefix8-png",
        validation: "chunk_bounds_crc_and_dimensions",
        state_binding: "unresolved",
    }))
}
fn validate_png(b: &[u8]) -> Result<(u32, u32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error("thumbnail payload is not PNG".into()));
    }
    let mut pos = 8;
    let mut dimensions = None;
    let mut idat = false;
    while pos < b.len() {
        let header = b
            .get(pos..pos + 8)
            .ok_or_else(|| Error("truncated PNG chunk".into()))?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = pos
            .checked_add(12)
            .and_then(|p| p.checked_add(length))
            .filter(|&p| p <= b.len())
            .ok_or_else(|| Error("PNG chunk out of bounds".into()))?;
        if crc32fast::hash(&b[pos + 4..end - 4])
            != u32::from_be_bytes(b[end - 4..end].try_into().unwrap())
        {
            return Err(Error("PNG CRC mismatch".into()));
        }
        let kind = &header[4..];
        let payload = &b[pos + 8..end - 4];
        if dimensions.is_none() && kind != b"IHDR" {
            return Err(Error("PNG does not start with IHDR".into()));
        }
        match kind {
            b"IHDR" => {
                if dimensions.is_some() || payload.len() != 13 {
                    return Err(Error("invalid/duplicate PNG IHDR".into()));
                }
                let w = u32::from_be_bytes(payload[..4].try_into().unwrap());
                let h = u32::from_be_bytes(payload[4..8].try_into().unwrap());
                if w == 0
                    || h == 0
                    || (w as u64) * (h as u64) > 16_777_216
                    || payload[10..12] != [0, 0]
                    || payload[12] > 1
                {
                    return Err(Error(
                        "invalid/oversized PNG dimensions or IHDR methods".into(),
                    ));
                }
                if !matches!(
                    (payload[9], payload[8]),
                    (0, 1 | 2 | 4 | 8 | 16) | (2 | 4 | 6, 8 | 16) | (3, 1 | 2 | 4 | 8)
                ) {
                    return Err(Error("invalid PNG color/bit-depth pair".into()));
                }
                dimensions = Some((w, h));
            }
            b"IDAT" => idat = true,
            b"IEND" => {
                if length != 0 || end != b.len() || !idat {
                    return Err(Error("invalid PNG end or missing image data".into()));
                }
                return Ok(dimensions.unwrap());
            }
            b"PLTE" => {}
            _ if kind[0] & 0x20 == 0 => return Err(Error("unknown critical PNG chunk".into())),
            _ => {}
        }
        pos = end;
    }
    Err(Error("PNG has no complete IEND".into()))
}
