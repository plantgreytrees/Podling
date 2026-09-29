//! Content-derived identifiers.
//!
//! Every artifact id is a BLAKE3 hash of the content that defines it, so the
//! same input always produces the same id and ids double as cache keys.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// A BLAKE3 digest rendered as 64 lowercase hex characters.
///
/// Deserialisation goes through [`FromStr`], so a malformed hash can never be
/// constructed — not even from a hand-edited JSON file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash(String);

impl JsonSchema for ContentHash {
    fn schema_name() -> Cow<'static, str> {
        "ContentHash".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^[0-9a-f]{64}$" })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid content hash {0:?}: expected 64 lowercase hex characters")]
pub struct InvalidHash(String);

impl ContentHash {
    /// Hashes several byte slices as one unambiguous message.
    ///
    /// Each part is length-prefixed, so `["ab", "c"]` and `["a", "bc"]` hash
    /// differently.
    pub fn of_parts(parts: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for part in parts {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        Self(hasher.finalize().to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ContentHash {
    type Err = InvalidHash;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidHash(s.to_owned()))
        }
    }
}

impl TryFrom<String> for ContentHash {
    type Error = InvalidHash;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ContentHash> for String {
    fn from(hash: ContentHash) -> Self {
        hash.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A stored id that doesn't match the content it claims to identify — the
/// artifact was edited by hand or corrupted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind} id {stored} does not match its content (expected {expected})")]
pub struct IdMismatch {
    pub kind: &'static str,
    pub stored: String,
    pub expected: String,
}

/// Declares a typed id wrapping a [`ContentHash`].
///
/// Distinct newtypes stop a `ChunkId` from being passed where a `DocumentId`
/// is expected — the compiler rejects the mix-up.
macro_rules! hash_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(ContentHash);

        impl $name {
            pub fn new(hash: ContentHash) -> Self {
                Self(hash)
            }

            pub fn hash(&self) -> &ContentHash {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

hash_id!(
    /// Identifies a source (connector + locator), independent of its content.
    SourceId
);
hash_id!(
    /// Identifies a document by its source and text.
    DocumentId
);
hash_id!(
    /// Identifies a chunk by its document and span.
    ChunkId
);
hash_id!(
    /// Identifies a claim by its normalised text.
    ClaimId
);

/// A human-readable speaker handle such as `host` or `guest`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct SpeakerId(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_well_formed() {
        let a = ContentHash::of_parts(&[b"hello"]);
        let b = ContentHash::of_parts(&[b"hello"]);
        assert_eq!(a, b);
        assert!(a.as_str().parse::<ContentHash>().is_ok());
    }

    #[test]
    fn parts_are_length_prefixed() {
        assert_ne!(
            ContentHash::of_parts(&[b"ab", b"c"]),
            ContentHash::of_parts(&[b"a", b"bc"])
        );
    }

    #[test]
    fn rejects_malformed_hashes() {
        assert!("abc".parse::<ContentHash>().is_err());
        assert!("G".repeat(64).parse::<ContentHash>().is_err());
        assert!(
            "A".repeat(64).parse::<ContentHash>().is_err(),
            "uppercase is not canonical"
        );
        assert!(serde_json::from_str::<ContentHash>("\"xyz\"").is_err());
    }
}
