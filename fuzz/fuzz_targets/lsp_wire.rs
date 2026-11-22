#![no_main]
use hedit_protocol::lsp::{self as wire, Decoder, Limits, READ_BYTES};

libfuzzer_sys::fuzz_target!(|bytes: &[u8]| {
    let _ = wire::parse_data(bytes, Limits::DATA);
    let limits = Limits::SCOPED;
    // A short exponent may normalize into a longer decimal representation.
    // Encoding has its own byte budget, so not every accepted source JSON
    // representation can be re-encoded within the same limit.
    if let Ok((value, frame)) = wire::parse_json(bytes, limits).and_then(|value| {
        wire::encode(&value, limits).map(|frame| (value, frame))
    }) {
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
