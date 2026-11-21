//! Identifiers carry no authority; the owning session must resolve every use.
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{fmt, io};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle([u8; 16]);
impl Handle {
    pub fn fresh() -> io::Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(io::Error::other)?;
        Ok(Self(bytes))
    }
}
impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}
impl Serialize for Handle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for Handle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Identifier;
        impl Visitor<'_> for Identifier {
            type Value = Handle;
            fn expecting(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
                fmt.write_str("a canonical opaque handle")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Handle, E> {
                if value.len() != 32
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(E::custom("invalid handle"));
                }
                let mut bytes = [0; 16];
                hex::decode_to_slice(value, &mut bytes).map_err(|_| E::custom("invalid handle"))?;
                Ok(Handle(bytes))
            }
        }
        deserializer.deserialize_str(Identifier)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_are_canonical_data_without_an_owner_assertion() {
        let handle = Handle::fresh().unwrap();
        let json = serde_json::to_string(&handle).unwrap();
        assert_eq!(json.len(), 34);
        assert_eq!(serde_json::from_str::<Handle>(&json).unwrap(), handle);
        for value in [
            "\"ABCDEF0123456789ABCDEF0123456789\"",
            "\"../outside\"",
            "{}",
            "0",
            "null",
        ] {
            assert!(serde_json::from_str::<Handle>(value).is_err());
        }
    }
}
