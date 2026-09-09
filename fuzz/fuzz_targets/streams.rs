#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| inventor_core::fuzzing::stream(data));
