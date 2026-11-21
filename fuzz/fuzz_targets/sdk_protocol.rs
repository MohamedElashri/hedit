#![no_main]
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    hedit_plugin::fuzz_protocol(bytes);
});
