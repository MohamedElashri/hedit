use hedit_remote::protocol::{self, Action, Envelope, Request, MAX_FRAME, VERSION};
use std::io::{self, Cursor, Read};

fn sample() -> Envelope {
    Envelope {
        version: VERSION,
        id: 3,
        request: Request::Apply {
            session: "0".repeat(64),
            epoch: 1,
            document: 1,
            sequence: 1,
            base_revision: 0,
            action: Action::Edit {
                start: 0,
                end: 0,
                text: "العربية\r\nשלום 🦀".into(),
            },
        },
    }
}

#[test]
fn fragmentation_and_consecutive_frames_preserve_unicode() {
    struct OneByte(Cursor<Vec<u8>>);
    impl Read for OneByte {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let len = out.len().min(1);
            self.0.read(&mut out[..len])
        }
    }
    let mut bytes = Vec::new();
    protocol::write_frame(&mut bytes, &sample()).unwrap();
    protocol::write_frame(&mut bytes, &sample()).unwrap();
    let mut reader = OneByte(Cursor::new(bytes));
    for _ in 0..2 {
        let envelope: Envelope = protocol::read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(
            protocol::encode(&envelope).unwrap(),
            protocol::encode(&sample()).unwrap()
        );
    }
    assert!(protocol::read_frame::<Envelope>(&mut reader)
        .unwrap()
        .is_none());
}

#[test]
fn every_truncation_inside_a_frame_is_rejected() {
    let mut bytes = Vec::new();
    protocol::write_frame(&mut bytes, &sample()).unwrap();
    for end in 1..bytes.len() {
        assert!(
            protocol::read_frame::<Envelope>(&mut &bytes[..end]).is_err(),
            "prefix {end}"
        );
    }
}

#[test]
fn malicious_lengths_are_rejected_without_reading_body() {
    for length in [0, MAX_FRAME as u32 + 1, u32::MAX] {
        let error =
            protocol::read_frame::<Envelope>(&mut length.to_be_bytes().as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}

#[test]
fn unknown_fields_duplicate_fields_and_deep_nesting_are_rejected() {
    for input in [
        r#"{"version":1,"id":1,"request":{"op":"ping","execute":"rm"}}"#.into(),
        r#"{"version":1,"id":1,"id":2,"request":{"op":"ping"}}"#.into(),
        format!("{}0{}", "[".repeat(200), "]".repeat(200)),
        "not json".into(),
    ] {
        let mut bytes = (input.len() as u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(input.as_bytes());
        assert!(protocol::read_frame::<Envelope>(&mut bytes.as_slice()).is_err());
    }
}

#[test]
fn outbound_json_expansion_is_bounded() {
    assert!(protocol::encode(&"\0".repeat(MAX_FRAME / 2)).is_err());
}
