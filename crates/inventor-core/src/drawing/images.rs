//! Only image placements emitted by the scene can request an embedded stream.
//! No external file loading or image signature search in unrelated payloads.
use super::*;
use crate::{property::Binary, rse, Error, Limits, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

#[derive(Debug, Serialize)]
pub struct EmbeddedImage {
    pub reference: u32,
    pub status: &'static str,
    pub mime_type: Option<&'static str>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: Option<String>,
    pub data: Option<Binary>,
    pub source: SourceSpan,
    pub diagnostic: Option<String>,
}

pub fn read_embedded_images(
    data: &[u8],
    scene: &ExperimentalScene,
    limits: &Limits,
) -> Result<Vec<EmbeddedImage>> {
    limits.validate()?;
    if data.len() > limits.max_file_bytes
        || format!("{:x}", Sha256::digest(data)) != scene.source_sha256
    {
        return Err(Error(
            "image scene/source identity mismatch or file limit".into(),
        ));
    }
    let mut work = limits.max_records;
    let mut refs = BTreeMap::new();
    for s in &scene.spaces {
        for item in &s.items {
            rse::charge(&mut work, 1)?;
            if let DisplayGeometry::Image {
                reference, format, ..
            } = item.geometry
            {
                if reference == 0 {
                    return Err(Error("invalid embedded image reference".into()));
                }
                if let Some((old, _)) =
                    refs.insert(reference, (format, item.source.source_id.clone()))
                {
                    if old != format {
                        return Err(Error("conflicting embedded image formats".into()));
                    }
                }
            }
        }
    }
    if refs.is_empty() {
        return Ok(vec![]);
    }
    if refs.len() > limits.max_streams {
        return Err(Error("embedded image stream count limit".into()));
    }
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).map_err(|e| Error(e.to_string()))?;
    let mut bytes_left = limits
        .max_property_bytes
        .min(limits.max_total_inflated_bytes);
    let mut pixels_left = 16_777_216u64;
    let mut out = vec![];
    for (reference, (format, source_id)) in refs {
        let path = format!("/RSeStorage/RefdFiles/RefdFile_{reference}");
        let mut image = EmbeddedImage {
            reference,
            status: "unavailable",
            mime_type: None,
            width: None,
            height: None,
            sha256: None,
            data: None,
            source: SourceSpan::stream(&source_id, &path, 0, 0),
            diagnostic: None,
        };
        let result = (|| -> Result<()> {
            let mut stream = file.open_stream(&path).map_err(|e| Error(e.to_string()))?;
            let n =
                usize::try_from(stream.len()).map_err(|_| Error("image length overflow".into()))?;
            if n > limits.max_stream_bytes || n > bytes_left {
                return Err(Error("embedded image byte limit".into()));
            }
            bytes_left -= n;
            rse::charge(&mut work, n.div_ceil(1024) + 1)?;
            let mut b = Vec::with_capacity(n);
            (&mut stream)
                .take(n as u64 + 1)
                .read_to_end(&mut b)
                .map_err(|e| Error(e.to_string()))?;
            if b.len() != n {
                return Err(Error("embedded image length mismatch".into()));
            }
            image.source.end_offset = n;
            let (mime, (w, h)) = match format {
                0 => ("image/jpeg", jpeg(&b)?),
                2 => ("image/png", crate::thumbnail::validate_png(&b)?),
                _ => return Err(Error("unsupported embedded image format".into())),
            };
            let pixels = (w as u64) * (h as u64);
            if pixels > pixels_left {
                return Err(Error("embedded image pixel limit".into()));
            }
            pixels_left -= pixels;
            image.mime_type = Some(mime);
            image.width = Some(w);
            image.height = Some(h);
            image.sha256 = Some(format!("{:x}", Sha256::digest(&b)));
            image.data = Some(Binary(b));
            image.status = "container_validated_pixels_not_decoded";
            Ok(())
        })();
        if let Err(e) = result {
            image.diagnostic = Some(e.to_string());
        }
        out.push(image);
    }
    Ok(out)
}

// Bounded baseline JPEG envelope/SOF/SOS validation, not a DCT pixel decoder.
fn jpeg(b: &[u8]) -> Result<(u32, u32)> {
    let bad = || Error("invalid or unsupported embedded baseline JPEG".into());
    if !b.starts_with(&[0xff, 0xd8]) {
        return Err(bad());
    }
    let mut p = 2;
    let mut dims = None;
    while p < b.len() {
        if b[p] != 0xff {
            return Err(bad());
        }
        p += 1;
        while b.get(p) == Some(&0xff) {
            p += 1;
        }
        let marker = *b.get(p).ok_or_else(bad)?;
        p += 1;
        let len = b.get(p..p + 2).ok_or_else(bad)?;
        let n = u16::from_be_bytes(len.try_into().unwrap()) as usize;
        if n < 2 {
            return Err(bad());
        }
        let payload = b
            .get(p + 2..p.checked_add(n).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        p += n;
        match marker {
            0xc0 => {
                if dims.is_some() || payload.len() < 6 || payload[0] != 8 {
                    return Err(bad());
                }
                let h = u16::from_be_bytes(payload[1..3].try_into().unwrap()) as u32;
                let w = u16::from_be_bytes(payload[3..5].try_into().unwrap()) as u32;
                let channels = payload[5] as usize;
                if !matches!(channels, 1 | 3)
                    || payload.len() != 6 + 3 * channels
                    || w == 0
                    || h == 0
                    || (w as u64) * (h as u64) > 16_777_216
                {
                    return Err(bad());
                }
                dims = Some((w, h));
            }
            0xda => {
                let dims = dims.ok_or_else(bad)?;
                if payload.is_empty()
                    || payload.len() != 4 + 2 * payload[0] as usize
                    || payload[payload.len() - 3..] != [0, 63, 0]
                {
                    return Err(bad());
                }
                while p < b.len() {
                    if b[p] != 0xff {
                        p += 1;
                        continue;
                    }
                    p += 1;
                    while b.get(p) == Some(&0xff) {
                        p += 1;
                    }
                    let code = *b.get(p).ok_or_else(bad)?;
                    p += 1;
                    match code {
                        0 | 0xd0..=0xd7 => (),
                        0xd9 if p == b.len() => return Ok(dims),
                        _ => return Err(bad()),
                    }
                }
                return Err(bad());
            }
            0xc4 | 0xdb | 0xdd | 0xe0..=0xef | 0xfe => (),
            _ => return Err(bad()),
        }
    }
    Err(bad())
}

#[cfg(feature = "fuzzing")]
pub(super) fn fuzz(bytes: &[u8]) {
    let _ = jpeg(bytes);
    let _ = crate::thumbnail::validate_png(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jpeg_envelope_rejects_truncation_huge_dimensions_and_extra_members() {
        // Synthetic header/entropy markers only; this is deliberately not a pixel oracle.
        let b = vec![
            255, 216, 255, 192, 0, 11, 8, 0, 2, 0, 3, 1, 1, 17, 0, 255, 218, 0, 8, 1, 1, 0, 0, 63,
            0, 7, 255, 0, 255, 217,
        ];
        assert_eq!(jpeg(&b).unwrap(), (3, 2));
        for n in 0..b.len() {
            assert!(jpeg(&b[..n]).is_err());
        }
        let mut extra = b.clone();
        extra.extend(&b);
        assert!(jpeg(&extra).is_err());
        let mut large = b;
        large[7..11].copy_from_slice(&[255; 4]);
        assert!(jpeg(&large).is_err());
    }

    #[test]
    fn embedded_assets_are_source_bound_deduplicated_and_budgeted() {
        use std::io::Write;
        let jpeg = vec![
            255, 216, 255, 192, 0, 11, 8, 0, 2, 0, 3, 1, 1, 17, 0, 255, 218, 0, 8, 1, 1, 0, 0, 63,
            0, 7, 255, 217,
        ];
        let mut c = cfb::CompoundFile::create(Cursor::new(vec![])).unwrap();
        c.create_storage_all("/RSeStorage/RefdFiles").unwrap();
        c.create_stream("/RSeStorage/RefdFiles/RefdFile_12")
            .unwrap()
            .write_all(&jpeg)
            .unwrap();
        let data = c.into_inner().into_inner();
        let make_item = || DisplayItem {
            segment_id: "synthetic".into(),
            record_ordinal: 7,
            source: SourceSpan::stream("synthetic", "/SM", 0, 64),
            placement_record: 7,
            group_path: vec![],
            transform: crate::assembly::IDENTITY,
            style: DisplayStyle::default(),
            geometry: DisplayGeometry::Image {
                reference: 12,
                format: 0,
                origin: [0.; 3],
                u: [1., 0., 0.],
                v: [0., -1., 0.],
            },
        };
        let mut scene = ExperimentalScene {
            status: "experimental_partial",
            qualified: false,
            source_sha256: format!("{:x}", Sha256::digest(&data)),
            units: "source_units_unverified",
            diagnostics: vec![],
            spaces: vec![DisplaySpace {
                segment_id: "synthetic".into(),
                extent_candidate: [42., 29.7],
                bindings: vec![],
                items: vec![make_item(), make_item()],
                omitted: vec![],
            }],
        };
        let images = read_embedded_images(&data, &scene, &Limits::default()).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].data.as_ref().unwrap().0, jpeg);
        assert_eq!((images[0].width, images[0].height), (Some(3), Some(2)));
        let limit = Limits {
            max_property_bytes: 1,
            ..Limits::default()
        };
        assert!(read_embedded_images(&data, &scene, &limit).unwrap()[0]
            .data
            .is_none());
        if let DisplayGeometry::Image { reference, .. } = &mut scene.spaces[0].items[0].geometry {
            *reference = 99;
        }
        let missing = read_embedded_images(&data, &scene, &Limits::default()).unwrap();
        assert!(missing
            .iter()
            .any(|i| i.reference == 99 && i.data.is_none() && i.diagnostic.is_some()));
        scene.source_sha256 = "not the input".into();
        assert!(read_embedded_images(&data, &scene, &Limits::default()).is_err());
    }
}
