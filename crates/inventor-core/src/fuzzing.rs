//! Bounded harness entry points, excluded from normal builds. No filesystem IO.
use crate::{read::Reader, rse, Limits};

pub fn limits() -> Limits {
    Limits {
        max_file_bytes: 2 * 1024 * 1024,
        max_stream_bytes: 256 * 1024,
        max_inflated_bytes: 256 * 1024,
        max_total_inflated_bytes: 512 * 1024,
        max_records: 4096,
        max_property_bytes: 65536,
        max_property_items: 1024,
        max_property_depth: 8,
        max_streams: 1024,
        max_candidates: 16,
    }
}

pub fn container(data: &[u8]) {
    let limits = limits();
    if data.len() > limits.max_file_bytes {
        return;
    }
    let _ = crate::inspect(data, "fuzz", &limits);
    let _ = crate::read(data, "fuzz", &limits);
    let _ = crate::assembly::inspect(data, "fuzz", &limits);
}

pub fn stream(data: &[u8]) {
    let Some((&mode, bytes)) = data.split_first() else {
        return;
    };
    if bytes.len() > 256 * 1024 {
        return;
    }
    let limits = limits();
    match mode % 5 {
        0 => {
            let _ = rse::inflate(bytes, 65536);
        }
        1 => {
            let _ = rse::meta(bytes, &limits);
        }
        2 => {
            let _ = rse::registry(bytes);
        }
        3 => {
            crate::assembly::fuzz_ufrx(&mut Reader::new(bytes), &limits);
        }
        _ => {
            let sab_limits = acis_core::sab::SabLimits {
                max_bytes: 256 * 1024,
                max_records: 4096,
                max_tokens: 16384,
                max_string_bytes: 65536,
                max_depth: 16,
            };
            let _ = acis_core::sab::parse_sab(bytes, "fuzz", &sab_limits);
        }
    }
}

/// Drawing container, paired Meta/B envelopes, and uncompressed framing. Inputs
/// carry a mode byte so mutation reaches trailers without breaking a checksum.
pub fn drawing(data: &[u8]) {
    let Some((&mode, bytes)) = data.split_first() else {
        return;
    };
    let limits = limits();
    if bytes.len() > limits.max_file_bytes {
        return;
    }
    match mode % 4 {
        0 => {
            // Keep the corpus harness's small typed-field allowance independent
            // of the larger production drawing default, like container work.
            let drawing_limits = crate::drawing::DrawingLimits {
                max_field_values: limits.max_records,
                ..Default::default()
            };
            if let Ok(doc) =
                crate::drawing::inspect_with_limits(bytes, "fuzz", &limits, &drawing_limits)
            {
                let _ = crate::drawing::stored_sheets(&doc, &limits);
                let scene = crate::drawing::experimental_scene(&doc, &limits);
                let _ = crate::drawing::read_embedded_images(bytes, &scene, &limits);
            }
        }
        1 if bytes.len() <= limits.max_stream_bytes => {
            let mut reader = Reader::new(bytes);
            if let Ok(n) = reader.count(limits.max_stream_bytes) {
                if let Ok(meta) = reader.take(n) {
                    crate::drawing::fuzz_pair(meta, &bytes[reader.pos..], &limits);
                }
            }
        }
        2 if bytes.len() <= limits.max_stream_bytes => {
            let _ = (|| -> crate::Result<()> {
                let mut r = Reader::new(bytes);
                let n = r.count(limits.max_records)?;
                let t = r.count(256)?;
                let blocks = r
                    .take(n * 4)?
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                let types = r
                    .take(t * 16)?
                    .chunks_exact(16)
                    .map(|b| b.try_into().unwrap())
                    .collect();
                let meta = rse::Meta {
                    id: [0; 16],
                    name: String::new(),
                    blocks,
                    types,
                    inflated_bytes: 0,
                    compressed_offset: 0,
                    codec: "synthetic",
                    state_words: [0; 3],
                    block_table_offset: 0,
                    type_table_offset: 0,
                    reference_sections: vec![],
                };
                rse::record_table(
                    &bytes[r.pos..],
                    &meta,
                    31,
                    &mut { limits.max_records },
                    true,
                )?;
                Ok(())
            })();
        }
        3 if bytes.len() <= limits.max_stream_bytes => crate::drawing::fuzz_fields(bytes, &limits),
        _ => {}
    }
}
