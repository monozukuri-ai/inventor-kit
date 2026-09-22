//! Observed framing profile, not a qualified drawing semantic profile.
use crate::{Error, Result};

pub(super) fn name(major: u8) -> &'static str {
    match major {
        23 => "idw-rse31-meta8-major23-zlib-framing-v1",
        31 => "idw-rse31-meta8-major31-zstd-framing-v3",
        _ => "unsupported",
    }
}
pub(super) fn meta_layout(kind: &str) -> crate::rse::MetaLayout {
    match kind {
        "DlDocDcSegmentType" => crate::rse::MetaLayout::DRAWING_DOC_DC,
        "DlSheetDcSegmentType" => crate::rse::MetaLayout::DRAWING_SHEET_DC,
        _ => crate::rse::MetaLayout::STANDARD,
    }
}
pub(super) const BULK_HEADER: [u8; 18] = [
    0x9e, 0xc2, 0x2b, 0xa4, 0xd4, 0x11, 0xe8, 0x01, 0x60, 0x00, 0x2d, 0xb3, 0xee, 0x29, 0xfb, 0xb0,
    0x04, 0x02,
];

pub(super) fn admits(kind: &str, major: u8) -> bool {
    matches!(major, 23 | 31)
        && (major != 23 || kind != "FBAttributeSegment")
        && matches!(
            kind,
            "DlDocDcSegmentType"
                | "DlBRxSegmentType"
                | "DlDirectorySegmentType"
                | "AppSegmentType"
                | "FBAttributeSegment"
                | "DlSheetDcSegmentType"
                | "DlSheetSmSegmentType"
                | "DlSheetDlSegmentType"
        )
}

pub(super) fn bulk(bytes: &[u8]) -> Result<&[u8]> {
    let major = match bytes.get(..18) {
        Some(header) if header == BULK_HEADER => 31,
        Some(header) if header[..17] == BULK_HEADER[..17] && header[17] == 1 => 23,
        _ => return Err(Error("unsupported drawing B-stream envelope".into())),
    };
    bulk_for_major(bytes, major)
}

pub(super) fn bulk_for_major(bytes: &[u8], major: u8) -> Result<&[u8]> {
    let mut header = BULK_HEADER;
    match major {
        23 => header[17] = 1,
        31 => (),
        _ => return Err(Error("unsupported drawing B-stream major".into())),
    }
    if !bytes.starts_with(&header) {
        return Err(Error(format!(
            "unsupported drawing B-stream header for major {major}"
        )));
    }
    let compressed = &bytes[BULK_HEADER.len()..];
    let valid_codec = if major == 31 {
        compressed.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
    } else {
        compressed.len() >= 2
            && compressed[0] == 0x78
            && compressed[1] & 0x20 == 0
            && u16::from_be_bytes([compressed[0], compressed[1]]).is_multiple_of(31)
    };
    if !valid_codec {
        return Err(Error(format!(
            "unsupported drawing B-stream codec for major {major}"
        )));
    }
    Ok(compressed)
}
