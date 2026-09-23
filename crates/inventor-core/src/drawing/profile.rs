//! Observed framing profile, not a qualified drawing semantic profile.
use crate::{Error, Result};

/// Independently observed wire choices. Compression does not identify the
/// document, font, reference, or bitmap layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BitmapLayout {
    Rgba,
    Monochrome,
}
#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub major: u8,
    pub zstd: bool,
    pub document_prefix: usize,
    pub link_suffix: usize,
    pub reference_flags: u32,
    pub compact_references: bool,
    pub legacy_fonts: bool,
    pub layer_prefix: usize,
    pub empty_attributes: bool,
    pub views: bool,
    pub view_extra_references: bool,
    pub view_suffix_guid: bool,
    pub bitmap: Option<BitmapLayout>,
    pub legacy_records: bool,
    pub historical_context: bool,
    pub external_children: bool,
}
impl Profile {
    pub fn fields_name(self) -> &'static str {
        match self.major {
            23 => "idw-major23-typed-fields-v1",
            24 => "idw-major24-typed-fields-v1",
            26 => "idw-major26-typed-fields-v1",
            28 => "idw-major28-typed-fields-v1",
            29 => "idw-major29-typed-fields-v1",
            31 => "idw-major31-typed-fields-v1",
            _ => unreachable!("profiles are constructed only by get"),
        }
    }
}
pub(super) fn get(major: u8) -> Option<Profile> {
    let mut p = Profile {
        major,
        zstd: false,
        document_prefix: 30,
        link_suffix: 4,
        reference_flags: 0,
        compact_references: true,
        legacy_fonts: true,
        layer_prefix: 19,
        empty_attributes: true,
        views: true,
        view_extra_references: false,
        view_suffix_guid: false,
        bitmap: Some(BitmapLayout::Rgba),
        legacy_records: true,
        historical_context: true,
        external_children: true,
    };
    match major {
        23 => (),
        24 => {
            p.layer_prefix = 21;
            // No view-cache fixture for this profile yet. Preserve omissions.
            p.bitmap = None;
            p.views = false;
            p.legacy_records = false;
            p.historical_context = false;
            p.external_children = false;
        }
        26 => {
            p.link_suffix = 8;
            p.legacy_fonts = false;
            p.layer_prefix = 21;
            p.view_suffix_guid = true;
            p.bitmap = None;
            p.legacy_records = false;
            p.historical_context = false;
            p.external_children = false;
        }
        28 | 29 | 31 => {
            p.zstd = major == 31;
            p.document_prefix = 34;
            p.link_suffix = 8;
            p.reference_flags = 0x10;
            p.compact_references = major == 28;
            p.legacy_fonts = false;
            p.layer_prefix = 21;
            p.empty_attributes = major != 31;
            p.view_extra_references = major == 31;
            p.view_suffix_guid = true;
            p.bitmap = if major == 28 {
                Some(BitmapLayout::Rgba)
            } else if major == 31 {
                Some(BitmapLayout::Monochrome)
            } else {
                None
            };
            p.legacy_records = false;
            p.historical_context = false;
            p.external_children = false;
        }
        _ => return None,
    }
    Some(p)
}

pub(super) fn name(major: u8) -> &'static str {
    match major {
        23 => "idw-rse31-meta8-major23-zlib-framing-v1",
        24 => "idw-rse31-meta8-major24-zlib-framing-v1",
        26 => "idw-rse31-meta8-major26-zlib-framing-v1",
        28 => "idw-rse31-meta8-major28-zlib-framing-v1",
        29 => "idw-rse31-meta8-major29-zlib-framing-v1",
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
    get(major).is_some()
        && (matches!(major, 24 | 31) || kind != "FBAttributeSegment")
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

#[cfg(feature = "fuzzing")]
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
    let profile = get(major).ok_or_else(|| Error("unsupported drawing B-stream major".into()))?;
    header[17] = if profile.zstd { 2 } else { 1 };
    if !bytes.starts_with(&header) {
        return Err(Error(format!(
            "unsupported drawing B-stream header for major {major}"
        )));
    }
    let compressed = &bytes[BULK_HEADER.len()..];
    let valid_codec = if profile.zstd {
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
