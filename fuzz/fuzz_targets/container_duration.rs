#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| big_multimedia_fuzz::container_duration(data));
