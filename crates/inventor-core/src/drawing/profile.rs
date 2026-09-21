//! Observed framing profile, not a qualified drawing semantic profile.
use crate::{Error, Result};

pub(super) const NAME: &str = "idw-rse31-meta8-major31-zstd-framing-v3";
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
    major == 31
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
    if !bytes.starts_with(&BULK_HEADER) {
        return Err(Error(
            "unsupported drawing B-stream header (expected observed 18-byte major31 envelope)"
                .into(),
        ));
    }
    let compressed = &bytes[BULK_HEADER.len()..];
    if !compressed.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Err(Error(
            "unsupported drawing B-stream codec (profile requires zstd)".into(),
        ));
    }
    Ok(compressed)
}
