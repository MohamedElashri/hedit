#![no_main]
use hedit_core::language_server::wire::{self, Decoder, Limits, READ_BYTES};

libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    let limits = Limits::SCOPED;
    if let Ok(value) = wire::parse_json(bytes, limits) {
        let frame = wire::encode(&value, limits).unwrap();
        let mut decoder = Decoder::new(limits);
        let mut decoded = Vec::new();
        for chunk in frame.chunks(READ_BYTES) {
            decoder.push(chunk).unwrap();
            while let Some(value) = decoder.next_message().unwrap() {
                decoded.push(value);
            }
        }
        decoder.finish().unwrap();
        assert_eq!(decoded, [value]);
    }
    let mut decoder = Decoder::new(limits);
    let chunk = bytes.first().copied().unwrap_or(0) as usize + 1;
    for chunk in bytes.chunks(chunk) {
        if decoder.push(chunk).is_err() {
            return;
        }
        loop {
            match decoder.next_message() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
    let _ = decoder.finish();
});
