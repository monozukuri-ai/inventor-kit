//! Only image placements emitted by the scene can request an embedded stream.
//! No external file loading or image signature search in unrelated payloads.
use super::*;
use crate::{property::Binary, rse, Error, Limits, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
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
    read_embedded_images_with_limits(data, scene, limits, &DrawingLimits::default())
}

pub fn read_embedded_images_with_limits(
    data: &[u8],
    scene: &ExperimentalScene,
    limits: &Limits,
    drawing: &DrawingLimits,
) -> Result<Vec<EmbeddedImage>> {
    drawing.validate()?;
    limits.validate()?;
    if data.len() > limits.max_file_bytes
        || format!("{:x}", Sha256::digest(data)) != scene.source_sha256
    {
        return Err(Error(
            "image scene/source identity mismatch or file limit".into(),
        ));
    }
    let mut work = limits.max_records.min(drawing.max_reference_visits);
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
                if let Some((old, old_source)) =
                    refs.insert(reference, (format, item.source.clone()))
                {
                    if old != format
                        || (format == 3
                            && (old_source.stream != item.source.stream
                                || old_source.start_offset != item.source.start_offset
                                || old_source.end_offset != item.source.end_offset
                                || old_source.byte_domain != item.source.byte_domain))
                    {
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
        .min(limits.max_total_inflated_bytes)
        .min(drawing.max_image_bytes);
    let mut pixels_left = drawing.max_image_pixels as u64;
    let mut out = vec![];
    let mut expanded = limits.max_total_inflated_bytes;
    for (reference, (format, source)) in refs {
        let path = format!("/RSeStorage/RefdFiles/RefdFile_{reference}");
        let mut image = EmbeddedImage {
            reference,
            status: "unavailable",
            mime_type: None,
            width: None,
            height: None,
            sha256: None,
            data: None,
            source: if format == 3 {
                source.clone()
            } else {
                SourceSpan::stream(&source.source_id, &path, 0, 0)
            },
            diagnostic: None,
        };
        let result = (|| -> Result<()> {
            if format == 3 {
                // Only the exact record span emitted by a bound SM view is read.
                if reference < 0x80000000
                    || source.byte_domain != "inflated_stream"
                    || !source.stream.starts_with("/RSeStorage/B")
                    || source.stream[12..].contains('/')
                {
                    return Err(Error("invalid view bitmap source".into()));
                }
                let n = source
                    .end_offset
                    .checked_sub(source.start_offset)
                    .ok_or_else(|| Error("invalid view bitmap span".into()))?;
                if n > bytes_left {
                    return Err(Error("embedded image byte limit".into()));
                }
                bytes_left -= n;
                let encoded = crate::stream(&mut file, &source.stream, limits.max_stream_bytes)?;
                let (body, _) = rse::inflate_budgeted(
                    super::profile::bulk(&encoded)?,
                    limits.max_inflated_bytes,
                    &mut expanded,
                )?;
                let record = body
                    .get(source.start_offset..source.end_offset)
                    .ok_or_else(|| Error("view bitmap span out of bounds".into()))?;
                let (w, h, png) =
                    monochrome_bitmap_png(record, bytes_left, &mut pixels_left, &mut work)?;
                bytes_left -= png.len();
                image.mime_type = Some("image/png");
                image.width = Some(w);
                image.height = Some(h);
                image.sha256 = Some(format!("{:x}", Sha256::digest(&png)));
                image.data = Some(Binary(png));
                image.status = "decoded_monochrome_view_cache_unqualified";
                return Ok(());
            }
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

// Observed major31 monochrome cache only: height/width, five zero layout bytes,
// then bottom-up RGBA pixels. Other colors/alpha layouts remain unavailable.
fn monochrome_bitmap_png(
    b: &[u8],
    output_limit: usize,
    pixels_left: &mut u64,
    work: &mut usize,
) -> Result<(u32, u32, Vec<u8>)> {
    let bad = || Error("unsupported monochrome view bitmap".into());
    let mut r = crate::read::Reader::new(b);
    r.skip(6)?;
    let h = r.u32()?;
    let w = r.u32()?;
    if r.take(5)? != [0; 5] {
        return Err(bad());
    }
    let pixels = w as u64 * h as u64;
    if w == 0 || h == 0 || pixels > 16_777_216 || pixels > *pixels_left {
        return Err(Error("embedded image pixel limit".into()));
    }
    *pixels_left -= pixels;
    let rgba = r.take(pixels as usize * 4)?;
    r.finish()?;
    rse::charge(work, (pixels as usize).div_ceil(1024) + 1)?;
    if rgba
        .chunks_exact(4)
        .any(|p| p[..3] != [0; 3] || !matches!(p[3], 0 | 255))
    {
        return Err(bad());
    }
    // Bound zlib output before allocation; raw and PNG bytes share the asset budget.
    let cap = DrawingLimits {
        max_output_bytes: output_limit.min(64 * 1024 * 1024),
        ..DrawingLimits::default()
    };
    let buffer = cap.output_buffer()?;
    let mut z = flate2::write::ZlibEncoder::new(buffer, flate2::Compression::default());
    for row in rgba.chunks_exact(w as usize * 4).rev() {
        z.write_all(&[0])
            .and_then(|_| z.write_all(row))
            .map_err(|e| Error(e.to_string()))?;
    }
    let compressed = z.finish().map_err(|e| Error(e.to_string()))?.into_bytes();
    let mut png = cap.output_buffer()?;
    png.write_all(b"\x89PNG\r\n\x1a\n")
        .map_err(|e| Error(e.to_string()))?;
    fn chunk(out: &mut impl Write, kind: &[u8; 4], b: &[u8]) -> std::io::Result<()> {
        out.write_all(&(b.len() as u32).to_be_bytes())?;
        out.write_all(kind)?;
        out.write_all(b)?;
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(b);
        out.write_all(&crc.finalize().to_be_bytes())
    }
    let mut header = [0; 13];
    header[..4].copy_from_slice(&w.to_be_bytes());
    header[4..8].copy_from_slice(&h.to_be_bytes());
    header[8..10].copy_from_slice(&[8, 6]);
    chunk(&mut png, b"IHDR", &header)
        .and_then(|_| chunk(&mut png, b"IDAT", &compressed))
        .and_then(|_| chunk(&mut png, b"IEND", &[]))
        .map_err(|e| Error(e.to_string()))?;
    Ok((w, h, png.into_bytes()))
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
    fn monochrome_cache_flips_rows_and_rejects_unobserved_layouts_and_budget_overruns() {
        let mut b = vec![0; 6];
        b.extend(2u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend([0; 5]);
        b.extend([0, 0, 0, 255, 0, 0, 0, 0]); // Bottom opaque, top transparent.
        let (w, h, png) = monochrome_bitmap_png(&b, 1024, &mut 2, &mut 10).unwrap();
        assert_eq!((w, h), (1, 2));
        assert_eq!(crate::thumbnail::validate_png(&png).unwrap(), (1, 2));
        let n = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let mut raw = vec![];
        flate2::read::ZlibDecoder::new(&png[41..41 + n])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw, [0, 0, 0, 0, 0, 0, 0, 0, 0, 255]);
        for end in 0..b.len() {
            assert!(monochrome_bitmap_png(&b[..end], 1024, &mut 2, &mut 10).is_err());
        }
        assert!(monochrome_bitmap_png(&b, 1024, &mut 1, &mut 10).is_err());
        assert!(monochrome_bitmap_png(&b, 1, &mut 2, &mut 10).is_err());
        assert!(monochrome_bitmap_png(&b, 1024, &mut 2, &mut 0).is_err());
        for index in [14, 18, 19, 20, 21, 22] {
            let mut bad = b.clone();
            bad[index] = 1;
            assert!(monochrome_bitmap_png(&bad, 1024, &mut 2, &mut 10).is_err());
        }
        let mut bad = b.clone();
        bad.extend([0; 4]);
        assert!(monochrome_bitmap_png(&bad, 1024, &mut 2, &mut 10).is_err());
        b[6..14].fill(255);
        let mut pixels = u64::MAX;
        assert!(monochrome_bitmap_png(&b, 1024, &mut pixels, &mut 10).is_err());
    }

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
                views: vec![],
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
