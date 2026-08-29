//! Fix for #933: add bounded metadata URI commitments.
//!
//! ## Problem
//!
//! Catalog metadata is entirely off-chain with no canonical integrity
//! pointer, and an untrusted URI string passed to a contract could blow
//! past Soroban per-call budgets if unbounded.
//!
//! ## Solution
//!
//! `contracts/library-rights` associates every catalog entry version
//! with a bounded content-addressed metadata commitment:
//!
//! - **Scheme allowlist** -- only `ipfs://`, `ipns://`, `https://`, and
//!   `ar://` are accepted; plain `http`, `file://`, `javascript:`, data
//!   URIs, and scheme-less strings are rejected (`InvalidMetadataUri`).
//!   The comparison is a raw prefix check, so an allowlisted scheme
//!   embedded mid-string (e.g. `http://evil/ipfs://...`) never passes.
//! - **Length bound** -- URIs are capped at
//!   [`metadata::METADATA_URI_MAX_LEN`] (200 chars), so untrusted
//!   strings can never exceed Soroban budgets.
//! - **Manifest hash** -- every commitment carries the hash of the
//!   metadata manifest the URI points at; all-zero hashes are rejected.
//! - **Updates create versions** -- `update_metadata` bumps the entry's
//!   version and appends an immutable `VersionSnapshot`, so the previous
//!   version's URI/hash stay exactly as they were.
//!
//! ## ABI impact
//!
//! New `update_metadata` entry point (plus `register_work` /
//! `register_edition` / `register_rendition`, which all carry the
//! commitment at registration) and read-only `entry_version`.
//!
//! ## Storage impact
//!
//! `MetadataCommitment { uri, manifest_hash }` is embedded in
//! `Entry(id)` and immutable `EntryVersion(id, v)` snapshots.
//!
//! ## Event impact
//!
//! `MET_UPD (entry_id, old_version, new_version, metadata_hash)` on
//! every metadata update; the URI itself is intentionally not emitted to
//! keep event payloads small.
//!
//! ## Privacy impact
//!
//! Only the URI and hash of the manifest are on-chain; manifest
//! contents (descriptions, contributor names, reviews) stay off-chain.
//!
//! ## Deployment & migration impact
//!
//! Additive; no migration required. The 200-char bound is enforced at
//! write time, so no historical entry can exceed it.
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/metadata.rs`: allowlisted schemes
//! round-trip, non-allowlisted schemes rejected, empty URI rejected,
//! exact max-length accepted / one-over rejected, zero manifest hash
//! rejected, versioned updates with immutable history, authorization,
//! and NotFound reads.

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
pub struct MetadataLedger {
    entries: std::collections::HashMap<u64, (String, u32)>, // id -> (uri, version)
}
const URI_MAX_LEN: usize = 200;
const SCHEMES: [&str; 4] = ["ipfs://", "ipns://", "https://", "ar://"];
impl MetadataLedger {
    pub fn new() -> Self {
        Self { entries: Default::default() }
    }
    pub fn commit(&mut self, id: u64, uri: &str) -> Result<u32, &'static str> {
        if uri.is_empty() || uri.len() > URI_MAX_LEN || !SCHEMES.iter().any(|s| uri.starts_with(s))
        {
            return Err("invalid metadata uri");
        }
        let version = self.entries.get(&id).map_or(0, |(_, v)| *v) + 1;
        self.entries.insert(id, (uri.to_string(), version));
        Ok(version) // updates create versions; history is append-only on-chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_and_length_bounds() {
        let mut m = MetadataLedger::new();
        assert_eq!(m.commit(1, "ipfs://QmX"), Ok(1));
        assert_eq!(m.commit(1, "ipfs://QmY"), Ok(2)); // versioned update
        assert_eq!(m.commit(2, "http://insecure"), Err("invalid metadata uri"));
        assert_eq!(m.commit(3, ""), Err("invalid metadata uri"));
        let too_long = format!("ipfs://{}", "a".repeat(193 + 1));
        assert_eq!(m.commit(4, &too_long), Err("invalid metadata uri"));
    }
}
