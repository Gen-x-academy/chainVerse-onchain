//! Bounded metadata URI commitments (#933).
//!
//! Every catalog entry version carries a [`MetadataCommitment`]: a
//! content-addressed URI plus the hash of the manifest it points at.
//! The URI is validated against a scheme allowlist and a length bound so
//! an untrusted string can never exceed Soroban per-call budgets, and
//! updates always create a new version instead of mutating the current
//! one.

use soroban_sdk::{BytesN, Env};

use crate::errors::ContractError;
use crate::types::MetadataCommitment;

/// #933 — maximum length of a committed metadata URI. Bounds untrusted
/// strings so they can never blow past Soroban per-call budgets.
pub const METADATA_URI_MAX_LEN: u32 = 200;

/// #933 — allowlisted URI schemes. Only content-addressed or verified
/// transport schemes are accepted; anything else is rejected.
const ALLOWED_URI_SCHEMES: [&str; 4] = ["ipfs://", "ipns://", "https://", "ar://"];

/// #933 — validates a metadata commitment: the manifest hash must be
/// non-zero and the URI must be non-empty, within
/// [`METADATA_URI_MAX_LEN`], and use an allowlisted scheme.
///
/// The URI is compared as raw bytes so a prefix like `ipfs://` embedded
/// anywhere else in the string (e.g. `https://evil/ipfs://...`) cannot
/// pass: only strings *starting* with an allowlisted scheme are valid.
/// The length bound is enforced before any byte copy, so an untrusted
/// string can never drive unbounded work or exceed Soroban budgets.
pub fn validate_metadata(env: &Env, metadata: &MetadataCommitment) -> Result<(), ContractError> {
    if metadata.manifest_hash == BytesN::from_array(env, &[0u8; 32]) {
        return Err(ContractError::InvalidHash);
    }
    if metadata.uri.is_empty() || metadata.uri.len() > METADATA_URI_MAX_LEN {
        return Err(ContractError::InvalidMetadataUri);
    }
    let uri_len = metadata.uri.len() as usize;
    let mut buf = [0u8; METADATA_URI_MAX_LEN as usize];
    metadata.uri.copy_into_slice(&mut buf[..uri_len]);
    let valid = ALLOWED_URI_SCHEMES
        .iter()
        .any(|s| buf[..uri_len].starts_with(s.as_bytes()));
    if !valid {
        return Err(ContractError::InvalidMetadataUri);
    }
    Ok(())
}
