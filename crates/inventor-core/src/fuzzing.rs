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
