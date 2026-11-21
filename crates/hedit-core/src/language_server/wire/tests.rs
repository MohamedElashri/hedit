use super::*;
use serde_json::json;

fn frame(body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend(body);
    bytes
}
fn decode(bytes: &[u8], chunk: usize) -> io::Result<Vec<Value>> {
    let mut decoder = Decoder::new(Limits::SCOPED);
    let mut messages = Vec::new();
    for bytes in bytes.chunks(chunk) {
        decoder.push(bytes)?;
        while let Some(value) = decoder.next_message()? {
            messages.push(value);
        }
    }
    decoder.finish()?;
    Ok(messages)
}

#[test]
fn fragmented_and_coalesced_unicode_frames_round_trip() {
    let values = [
        json!({"jsonrpc":"2.0","method":"document","params":{"text":"café / 東京 / 😀"}}),
        json!({"jsonrpc":"2.0","id":1,"result":null}),
        json!({"jsonrpc":"2.0","id":"request","error":{"code":-32601,"message":"Unavailable"}}),
    ];
    let bytes: Vec<_> = values
        .iter()
        .flat_map(|value| encode(value, Limits::SCOPED).unwrap())
        .collect();
    for chunk in [1, 2, 3, 4, 7, 31, READ_BYTES] {
        assert_eq!(decode(&bytes, chunk).unwrap(), values);
    }
}

#[test]
fn every_truncated_prefix_fails_without_hiding_partial_messages() {
    let bytes = encode(
        &json!({"jsonrpc":"2.0","id":4,"result":{"text":"😀"}}),
        Limits::SCOPED,
    )
    .unwrap();
    assert!(decode(&[], 1).unwrap().is_empty());
    for end in 1..bytes.len() {
        assert!(decode(&bytes[..end], 1).is_err(), "prefix {end}");
    }
}

#[test]
fn malformed_headers_and_duplicate_json_fields_poison_connection() {
    for bytes in [
        b"Content-Length: 1\r\nContent-Length: 1\r\n\r\n0".to_vec(),
        b"Content-Length: +1\r\n\r\n0".to_vec(),
        b"Content-Length: 99999999999999999999999\r\n\r\n".to_vec(),
        b"Content-Length: 0\r\n\r\n".to_vec(),
        b"Content-Length: 262145\r\n\r\n".to_vec(),
        b"Content-Length: 1\n\n0".to_vec(),
        b"Content-Length: 1\r\nOther: value\r\n\r\n0".to_vec(),
        b"Content-Length: 1\r\nContent-Type: application/json; charset=latin1\r\n\r\n0".to_vec(),
        frame(br#"{"jsonrpc":"2.0","id":1,"result":null,"result":{}}"#),
        frame(br#"{"jsonrpc":"2.0","id":1,"result":{"x":1,"x":2}}"#),
        frame(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"\xff\"}"),
        vec![b'x'; HEADER_BYTES + 1],
    ] {
        assert!(decode(&bytes, 1).is_err());
        let mut decoder = Decoder::new(Limits::SCOPED);
        // Oversized pushes fail before extending the buffer.
        assert!(decoder.push(&vec![0; READ_BYTES + 1]).is_err());
        assert!(decoder.push(&[]).is_err());
        assert!(decoder.next_message().is_err());
        assert!(decoder.finish().is_err());
    }
}

#[test]
fn envelope_validation_rejects_ambiguous_authority() {
    for value in [
        json!([]),
        json!({"jsonrpc":"1.0","id":1,"result":null}),
        json!({"jsonrpc":"2.0","id":1,"result":null,"error":{}}),
        json!({"jsonrpc":"2.0","id":1}),
        json!({"jsonrpc":"2.0","method":"x","result":1}),
        json!({"jsonrpc":"2.0","method":"x","id":null}),
        json!({"jsonrpc":"2.0","method":"x","id":2147483648u64}),
        json!({"jsonrpc":"2.0","method":"x","params":"scalar"}),
        json!({"jsonrpc":"2.0","method":"x\n"}),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":1,"message":3}}),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"x","extra":1}}),
    ] {
        assert!(encode(&value, Limits::SCOPED).is_err(), "{value}");
    }
}

#[test]
fn nested_and_flat_allocation_budgets_apply_before_building_values() {
    let deep = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{}{}}}"#,
        "[".repeat(DEPTH + 1),
        "]".repeat(DEPTH + 1)
    );
    assert!(parse_json(deep.as_bytes(), Limits::SCOPED).is_err());
    let flat = json!({"jsonrpc":"2.0","id":1,"result":vec![0; 8192]});
    assert!(encode(&flat, Limits::SCOPED).is_err());
    assert!(encode(&flat, Limits::BUILTIN).is_ok());
    let big = json!({"jsonrpc":"2.0","id":1,"result":"x".repeat(256*1024)});
    assert!(encode(&big, Limits::SCOPED).is_err());
}

#[test]
fn bounded_mutation_corpus_never_panics_or_accepts_partial_frames() {
    let original = encode(
        &json!({"jsonrpc":"2.0","method":"x","params":{"x":"😀"}}),
        Limits::SCOPED,
    )
    .unwrap();
    for position in 0..original.len() {
        for byte in [0, b'\r', b'\n', b'"', 0xff] {
            let mut bytes = original.clone();
            bytes[position] = byte;
            let _ = decode(&bytes, 3);
        }
    }
}
