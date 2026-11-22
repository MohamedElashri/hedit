use super::*;
use crate::options::Value;
use std::io::{self, Cursor};

fn input() -> Input {
    Input {
        source: "-- café / 東京\nreturn function(text) return text end".into(),
        selections: vec!["café\r\n東京".into()],
        language: "markdown".into(),
        settings: [("prefix".into(), Value::Text("→ ".into()))].into(),
    }
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut packet = REQUEST_MAGIC.to_le_bytes().to_vec();
    packet.extend((payload.len() as u32).to_le_bytes());
    packet.extend(payload);
    packet
}

fn assert_request_rejected(input: &Input) {
    assert_eq!(encode_request(input), Err(Error::Request));
    // Bypass the trusted encoder to exercise the worker's independent checks.
    assert!(matches!(
        decode_request(frame(&serde_json::to_vec(input).unwrap()).as_slice()),
        Err(Error::Request)
    ));
}

#[test]
fn request_limits_are_enforced_on_both_sides_of_the_pipe() {
    let mut request = input();
    request
        .source
        .push_str(&" ".repeat(MAX_SOURCE_BYTES - request.source.len()));
    request.language = "λ".repeat(MAX_LANGUAGE_BYTES / 2);
    request.selections =
        vec!["λ".repeat(MAX_SELECTION_BYTES / MAX_SELECTIONS / 2); MAX_SELECTIONS];
    request.settings = (0..plugin_options::MAX_SETTINGS)
        .map(|i| {
            (
                format!("setting-{i}"),
                Value::Text("λ".repeat(plugin_options::MAX_VALUE_BYTES / 2)),
            )
        })
        .collect();
    let valid = serde_json::to_vec(&request).unwrap();
    let decoded = decode_request(encode_request(&request).unwrap().as_slice()).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), valid);
    let over = |mutate: fn(&mut Input)| {
        let mut request: Input = serde_json::from_slice(&valid).unwrap();
        mutate(&mut request);
        assert_request_rejected(&request);
    };
    over(|r| r.source.push(' '));
    over(|r| r.language.push(' '));
    over(|r| r.selections[0].push(' '));
    over(|r| r.selections.push(String::new()));
    over(|r| r.selections.clear());
    over(|r| {
        r.settings.insert("extra".into(), Value::Boolean(true));
    });
    over(|r| {
        r.settings
            .insert("prefix".into(), Value::Text("x".repeat(257)));
    });
    over(|r| {
        r.settings.clear();
        r.settings.insert("prefix".into(), Value::Text("\0".into()));
    });
    for key in ["", "Upper", "a.b", "../file", "with space", &"a".repeat(49)] {
        let mut request = input();
        request.settings = [(key.into(), Value::Boolean(true))].into();
        assert_request_rejected(&request);
    }
}

#[test]
fn every_truncation_wrong_magic_and_trailing_message_is_rejected() {
    let packet = encode_request(&input()).unwrap();
    for length in 0..packet.len() {
        assert!(
            matches!(decode_request(&packet[..length]), Err(Error::Request)),
            "length {length}"
        );
    }
    for byte in 0..4 {
        let mut changed = packet.clone();
        changed[byte] ^= 1;
        assert!(matches!(
            decode_request(changed.as_slice()),
            Err(Error::Request)
        ));
    }
    for tail in [b" ".as_slice(), b"\0", b"{}", packet.as_slice()] {
        let mut changed = packet.clone();
        changed.extend(tail);
        assert!(matches!(
            decode_request(changed.as_slice()),
            Err(Error::Request)
        ));
    }
}

#[test]
fn oversized_length_is_rejected_before_reading_or_allocating_its_payload() {
    struct HeaderOnly(Cursor<Vec<u8>>);
    impl Read for HeaderOnly {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            assert!(self.0.position() < 8, "oversized payload must not be read");
            self.0.read(bytes)
        }
    }
    for size in [MAX_BATCH_REQUEST_BYTES as u32 + 1, u32::MAX] {
        let mut header = REQUEST_MAGIC.to_le_bytes().to_vec();
        header.extend(size.to_le_bytes());
        assert!(matches!(
            decode_request(HeaderOnly(Cursor::new(header))),
            Err(Error::Request)
        ));
    }
}

#[test]
fn schema_types_unknown_fields_duplicates_and_invalid_encodings_are_rejected() {
    for payload in [
        b"null".as_slice(),
        b"[]",
        b"{}",
        b"\xff",
        b"{",
        br#"{"source":"","source":"","selections":[""],"language":"","settings":{}}"#,
        br#"{"source":"","selections":[""],"language":"","settings":{},"extra":true}"#,
        br#"{"source":42,"selections":[""],"language":"","settings":{}}"#,
        br#"{"source":"","selections":[null],"language":"","settings":{}}"#,
        br#"{"source":"","selections":[""],"language":"","settings":{"a":2147483648}}"#,
        br#"{"source":"","selections":[""],"language":"","settings":{"a":1.5}}"#,
        br#"{"source":"","selections":[""],"language":"","settings":{"a":{}}}"#,
        br#"{"source":"","selections":["\ud800"],"language":"","settings":{}}"#,
    ] {
        assert!(matches!(
            decode_request(frame(payload).as_slice()),
            Err(Error::Request)
        ));
    }
}

#[test]
fn fragmented_reads_work_and_io_failures_never_produce_a_request() {
    struct Fragmented<'a> {
        bytes: &'a [u8],
        fail_at: usize,
        read: usize,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.read == self.fail_at {
                return Err(io::Error::other("injected read failure"));
            }
            let size = buffer.len().min(1);
            let n = self.bytes.read(&mut buffer[..size])?;
            self.read += n;
            Ok(n)
        }
    }
    let packet = encode_request(&input()).unwrap();
    let read = |fail_at| Fragmented {
        bytes: &packet,
        fail_at,
        read: 0,
    };
    assert!(decode_request(read(usize::MAX)).is_ok());
    // Include the final EOF check, which is part of the frame.
    for fail_at in 0..=packet.len() {
        assert!(matches!(decode_request(read(fail_at)), Err(Error::Request)));
    }
}

#[cfg(any(unix, windows))]
#[test]
fn worker_statuses_never_carry_error_text_or_accept_incomplete_success() {
    for status in 1..=u8::MAX {
        let expected = Error::from_worker_code(status);
        for delivered in [false, true] {
            assert_eq!(worker_reply(vec![status], delivered), Err(expected));
            assert_eq!(
                worker_reply(vec![status, b'x'], delivered),
                Err(Error::Worker)
            );
        }
    }
    assert_eq!(worker_reply(vec![], true), Err(Error::Worker));
    assert_eq!(worker_reply(b"\0[]".to_vec(), false), Err(Error::Worker));
    assert_eq!(
        worker_reply(b"\0[\"ok\"]".to_vec(), true).unwrap(),
        br#"["ok"]"#
    );
    assert!(worker_reply(vec![0; MAX_BATCH_REPLY_BYTES + 1], true).is_ok());
    assert_eq!(
        worker_reply(vec![0; MAX_BATCH_REPLY_BYTES + 2], true),
        Err(Error::Worker)
    );
}

#[test]
fn replies_require_exact_edit_count_and_valid_utf8_strings_without_trailing_data() {
    for reply in [
        b"".as_slice(),
        b"null",
        b"{}",
        b"[]",
        b"[1]",
        b"[null]",
        b"[{}]",
        b"[\"ok\",\"extra\"]",
        b"[\"ok\"]{}",
        b"[\"\xff\"]",
        br#"["\ud800"]"#,
    ] {
        assert_eq!(decode_edits(reply, 1), Err(Error::Worker));
    }
    for count in [0, MAX_SELECTIONS + 1, usize::MAX] {
        assert_eq!(decode_edits(br#"["ok"]"#, count), Err(Error::Worker));
    }
    assert_eq!(decode_edits(br#"["ok"]"#, 2), Err(Error::Worker));
    assert_eq!(
        decode_edits(b" [\"caf\xc3\xa9\\r\\n\\u6771\\u4eac\"] \n", 1).unwrap(),
        ["café\r\n東京"]
    );
    assert_eq!(
        decode_edits(br#"["\u0000"]"#, 1),
        Err(Error::ReturnEncoding)
    );
    assert_eq!(
        decode_edits(&vec![b' '; MAX_BATCH_REPLY_BYTES + 1], 1),
        Err(Error::Worker)
    );
}

#[test]
fn reply_byte_limits_count_decoded_utf8_across_all_edits() {
    let text = "λ".repeat(MAX_REPLACEMENT_BYTES / 2);
    let payload = serde_json::to_vec(&vec![text.clone()]).unwrap();
    assert_eq!(
        decode_edits(&payload, 1).unwrap().as_slice(),
        std::slice::from_ref(&text)
    );
    let too_many = serde_json::to_vec(&vec![text, "x".into()]).unwrap();
    assert_eq!(decode_edits(&too_many, 2), Err(Error::ReturnSize));
    // JSON escaping can expand a valid 256 KiB result to more than 1 MiB.
    let escaped = vec!["\u{1}".repeat(MAX_REPLACEMENT_BYTES / MAX_SELECTIONS); MAX_SELECTIONS];
    let payload = serde_json::to_vec(&escaped).unwrap();
    assert!(payload.len() > 1024 * 1024);
    assert_eq!(decode_edits(&payload, MAX_SELECTIONS).unwrap(), escaped);
}

#[test]
fn extra_replacements_are_refused_before_their_contents_are_deserialized() {
    let failure = Cell::new(Error::Worker);
    // Deliberately invalid JSON after the expected edit. The count guard must
    // reject it before the parser attempts to decode any extra value.
    let mut decoder = serde_json::Deserializer::from_slice(b"[\"ok\",not-json");
    let error = Edits {
        count: 1,
        failure: &failure,
    }
    .deserialize(&mut decoder)
    .unwrap_err();
    assert!(error.to_string().contains("unexpected replacement"));
    let many = format!("[\"ok\",{}]", "\"\",".repeat(100_000));
    assert_eq!(decode_edits(many.as_bytes(), 1), Err(Error::Worker));
}

#[test]
fn deterministic_mutation_corpus_preserves_the_protocol_invariants() {
    let request = encode_request(&input()).unwrap();
    let reply = r#"["café\r\n東京"]"#.as_bytes();
    let mut random = 0xa0761d6478bd642fu64;
    let mut next = || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random as usize
    };
    for case in 0..10_000 {
        let mut bytes = if case % 2 == 0 {
            request.clone()
        } else {
            reply.to_vec()
        };
        for _ in 0..1 + next() % 8 {
            let at = next() % bytes.len();
            bytes[at] = next() as u8;
        }
        if case % 3 == 0 {
            bytes.truncate(next() % bytes.len());
        }
        if let Ok(decoded) = decode_request(bytes.as_slice()) {
            assert!(encode_request(&decoded).is_ok());
        }
        if let Ok(edits) = decode_edits(&bytes, 1) {
            assert_eq!(edits.len(), 1);
            assert!(validate_edits(&edits).is_ok());
        }
        // Feed arbitrary bytes as well as near-valid messages.
        let arbitrary: Vec<u8> = (0..next() % 512).map(|_| next() as u8).collect();
        assert!(decode_request(arbitrary.as_slice()).is_err());
        if let Ok(edits) = decode_edits(&arbitrary, 1) {
            assert_eq!(edits.len(), 1);
            assert!(validate_edits(&edits).is_ok());
        }
    }
}

#[test]
fn sdk_framing_refuses_truncation_trailing_bytes_and_version_confusion() {
    let input: crate::sdk::Input = serde_json::from_slice(
        include_bytes!("../../../../../fuzz/corpus/sdk_protocol/request")
            .get(8..)
            .unwrap(),
    )
    .unwrap();
    let bytes = encode_sdk(&input).unwrap();
    assert!(matches!(
        decode_invocation(bytes.as_slice()),
        Ok(Invocation::Sdk(_))
    ));
    for end in 0..bytes.len() {
        assert!(decode_invocation(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_invocation(trailing.as_slice()).is_err());
    let mut wrong = bytes.clone();
    wrong[..4].copy_from_slice(&REQUEST_MAGIC.to_le_bytes());
    assert!(decode_invocation(wrong.as_slice()).is_err());
    let mut wrong_version = serde_json::to_value(&input).unwrap();
    wrong_version["sdk_version"] = 2.into();
    let body = serde_json::to_vec(&wrong_version).unwrap();
    let mut packet = SDK_MAGIC.to_le_bytes().to_vec();
    packet.extend((body.len() as u32).to_le_bytes());
    packet.extend(body);
    assert!(decode_invocation(packet.as_slice()).is_err());
    let mut huge = SDK_MAGIC.to_le_bytes().to_vec();
    huge.extend(u32::MAX.to_le_bytes());
    assert!(decode_invocation(huge.as_slice()).is_err());
    assert!(decode_request(bytes.as_slice()).is_err());
}
