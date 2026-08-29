//! Algorithm-tagged digital content hashes (#932).
//!
//! Files and access URLs stay off-chain; only the digest of the digital
//! artifact (EPUB, PDF, audio, ...) is anchored per rendition, tagged
//! with its algorithm. The supported-algorithm list is enforced by the
//! [`HashAlgorithm`] enum itself, digests must be non-zero, and hashes
//! are immutable per version -- changing a rendition's hash creates a
//! new version while the old version's snapshot keeps the original
//! digest.

use soroban_sdk::{BytesN, Env};

use crate::errors::ContractError;
use crate::registry;
use crate::types::{ContentCommitment, ContentState, HashAlgorithm};

/// #932 — a content commitment must carry a real digest; an all-zero
/// digest is rejected so a commitment can never be vacuous.
pub fn validate_content(env: &Env, content: &ContentCommitment) -> Result<(), ContractError> {
    if content.digest == BytesN::from_array(env, &[0u8; 32]) {
        return Err(ContractError::InvalidHash);
    }
    Ok(())
}

/// #932 — read-only verification: does the current content commitment of
/// `rendition_id` exactly match `(algorithm, digest)`? Entries without a
/// content commitment (works, editions) verify as `false` rather than
/// erroring, so callers can treat `false` as "does not match".
pub fn verify_content(
    env: &Env,
    rendition_id: &BytesN<32>,
    algorithm: HashAlgorithm,
    digest: &BytesN<32>,
) -> Result<bool, ContractError> {
    let entry = registry::get_entry(env, rendition_id)?;
    match &entry.content {
        ContentState::Committed(content) => {
            Ok(content.algorithm == algorithm && content.digest == *digest)
        }
        ContentState::None => Ok(false),
    }
}
