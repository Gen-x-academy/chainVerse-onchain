//! Fix for #932: anchor digital content hashes.
//!
//! ## Problem
//!
//! There is no way to verify that a delivered EPUB, PDF, or audio
//! rendition matches its registered artifact: files and access URLs
//! live off-chain with no on-chain integrity pointer.
//!
//! ## Solution
//!
//! `contracts/library-rights` anchors algorithm-tagged content hashes
//! per rendition:
//!
//! - **Allowlisted algorithms** -- the `HashAlgorithm` enum (`Sha256`,
//!   `Sha512`, per FIPS 180-4) is the supported-algorithm list; anything
//!   else is rejected at the ABI boundary.
//! - **Per rendition** -- `register_rendition` takes a `content`
//!   [`ContentCommitment`] `{ algorithm, digest }`; works and editions
//!   carry no content commitment (`update_content_hash` on a
//!   non-rendition is `InvalidKind`).
//! - **Immutable per version** -- every version gets an append-only
//!   `VersionSnapshot`; `update_content_hash` bumps the version and the
//!   old snapshot keeps the original digest, so any specific version can
//!   be verified exactly as registered.
//! - **Verification** -- `verify_content(rendition_id, algorithm,
//!   digest)` is a read-only check; tests use deterministic FIPS/NIST
//!   vectors (`sha256("abc")`, `sha512("abc")`).
//!
//! ## ABI impact
//!
//! New `register_rendition`, `update_content_hash`, and `verify_content`
//! entry points.
//!
//! ## Storage impact
//!
//! New persistent keys: `Entry(rendition_id)` with an embedded
//! `ContentCommitment { algorithm, digest }`, plus immutable
//! `EntryVersion(rendition_id, v)` snapshots. All CATALOG-tiered.
//!
//! ## Event impact
//!
//! `RND_NEW (rendition_id, parent, version, algorithm, digest)` at
//! registration and `HASH_UPD (rendition_id, old_version, new_version,
//! algorithm, digest)` on every hash change.
//!
//! ## Privacy impact
//!
//! Only digests are stored -- never file bytes, access URLs, or reader
//! behavior. A digest reveals nothing about the file's contents beyond
//! what a caller already has locally.
//!
//! ## Deployment & migration impact
//!
//! Additive; no existing keys reshaped. Because digests are immutable
//! per version, a future algorithm deprecation only affects newly
//! registered versions.
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/content.rs`: deterministic
//! sha256/sha512 vectors, wrong-digest/wrong-algorithm rejection, zero
//! digest rejection, version bump with preserved history, non-rendition
//! rejection, authorization, and NotFound reads.

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
pub struct ContentLedger {
    hashes: std::collections::HashMap<u64, (u8, [u8; 32])>, // id -> (algorithm, digest)
}
impl ContentLedger {
    pub fn new() -> Self {
        Self { hashes: Default::default() }
    }
    pub fn commit(&mut self, id: u64, algorithm: u8, digest: [u8; 32]) -> Result<(), &'static str> {
        if algorithm > 1 {
            return Err("unsupported algorithm"); // 0 = Sha256, 1 = Sha512
        }
        if digest == [0u8; 32] {
            return Err("invalid hash");
        }
        self.hashes.insert(id, (algorithm, digest));
        Ok(())
    }
    pub fn verify(&self, id: u64, algorithm: u8, digest: [u8; 32]) -> bool {
        self.hashes.get(&id) == Some(&(algorithm, digest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sha256_vector_verifies() {
        // sha256("abc") -- FIPS 180-4 test vector.
        let digest: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        let mut l = ContentLedger::new();
        l.commit(1, 0, digest).unwrap();
        assert!(l.verify(1, 0, digest));
        assert!(!l.verify(1, 1, digest)); // wrong algorithm
        assert_eq!(l.commit(2, 9, digest), Err("unsupported algorithm"));
    }
}
