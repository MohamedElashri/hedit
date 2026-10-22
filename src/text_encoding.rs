//! Explicit, lossless decoding and encoding. Legacy encodings are never guessed.
pub const LABELS: &[&str] = &[
    "UTF-8",
    "UTF-16LE",
    "UTF-16BE",
    "windows-1252",
    "ISO-8859-1",
    "Shift_JIS",
    "GBK",
];
pub fn canonical(label: &str) -> Result<&'static str, String> {
    if label.eq_ignore_ascii_case("ISO-8859-1") || label.eq_ignore_ascii_case("latin1") {
        return Ok("ISO-8859-1");
    }
    encoding_rs::Encoding::for_label(label.as_bytes())
        .map(|e| e.name())
        .ok_or_else(|| "Unknown encoding.".into())
}
pub const DECODE_LIMIT: u64 = 32 * 1024 * 1024;

pub fn decode(bytes: &[u8], label: &str) -> Result<(String, bool), String> {
    if canonical(label)? == "ISO-8859-1" {
        if bytes.contains(&0) {
            return Err("This looks like a binary file.".into());
        }
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        if text.len() as u64 > DECODE_LIMIT {
            return Err("Decoded text exceeds 32 MiB.".into());
        }
        return Ok((text, false));
    }
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).ok_or("Unknown encoding.")?;
    let bom = (encoding == encoding_rs::UTF_8 && bytes.starts_with(&[0xef, 0xbb, 0xbf]))
        || (encoding == encoding_rs::UTF_16LE && bytes.starts_with(&[0xff, 0xfe]))
        || (encoding == encoding_rs::UTF_16BE && bytes.starts_with(&[0xfe, 0xff]));
    let prefix = if bom {
        if encoding == encoding_rs::UTF_8 {
            3
        } else {
            2
        }
    } else {
        0
    };
    let text = encoding
        .decode_without_bom_handling_and_without_replacement(&bytes[prefix..])
        .ok_or_else(|| format!("Invalid {label} text; choose the correct encoding."))?;
    if text.contains('\0') {
        return Err("This looks like a binary file.".into());
    }
    if text.len() as u64 > DECODE_LIMIT {
        return Err("Decoded text exceeds 32 MiB.".into());
    }
    Ok((text.into_owned(), bom))
}

pub fn detect(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xfe]) {
        "UTF-16LE"
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        "UTF-16BE"
    } else {
        "UTF-8"
    }
}

pub fn encode(text: &str, label: &str, bom: bool) -> Result<Vec<u8>, String> {
    if canonical(label)? == "ISO-8859-1" {
        return text
            .chars()
            .map(|c| {
                u8::try_from(c as u32).map_err(|_| {
                    "Some characters cannot be represented in ISO-8859-1; choose UTF-8.".into()
                })
            })
            .collect();
    }
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).ok_or("Unknown encoding.")?;
    if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
        let mut out = Vec::with_capacity(text.len() * 2 + 2);
        if bom {
            out.extend_from_slice(if encoding == encoding_rs::UTF_16LE {
                &[0xff, 0xfe]
            } else {
                &[0xfe, 0xff]
            });
        }
        for ch in text.encode_utf16() {
            out.extend_from_slice(&if encoding == encoding_rs::UTF_16LE {
                ch.to_le_bytes()
            } else {
                ch.to_be_bytes()
            });
        }
        return Ok(out);
    }
    let (bytes, _, errors) = encoding.encode(text);
    if errors {
        return Err(format!(
            "Some characters cannot be represented in {label}; choose UTF-8."
        ));
    }
    let mut out = Vec::new();
    if bom && encoding == encoding_rs::UTF_8 {
        out.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    out.extend_from_slice(&bytes);
    Ok(out)
}
