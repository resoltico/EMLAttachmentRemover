//! Byte identities for read-only contributor tooling.
use sha2::{Digest, Sha256};
/// Byte identity of the immutable inspected input, not review/behavior quality.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteIdentity {
    /// Exact snapshot byte length.
    pub byte_length: usize,
    /// SHA-256 of these same immutable bytes.
    pub sha256: String,
}
impl ByteIdentity {
    /// Identify bytes without implying source provenance or review quality.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len(),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        }
    }
}
