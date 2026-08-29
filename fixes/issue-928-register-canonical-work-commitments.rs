//! Fix for #928: register canonical work commitments.
//!
//! ## Problem
//!
//! No on-chain primitive represents a library work or its accountable
//! publisher: `put_work` stores a bare hash + custodian with no
//! identifier validation, no overwrite protection, and no event, and
//! nothing binds a canonical work id to a bounded metadata commitment.
//!
//! ## Solution
//!
//! `contracts/library-rights` gains `register_work(caller, work_id,
//! metadata, custodian)` (where `metadata` is a
//! [`MetadataCommitment`] carrying a bounded URI + manifest hash):
//!
//! - **validates identifiers** -- all-zero work ids and manifest hashes
//!   are rejected (`InvalidIdentifier` / `InvalidHash`);
//! - **authenticates the issuer** -- only the `PolicyManager` role can
//!   register, and `require_auth` proves the caller consented
//!   (`governance::require_role`);
//! - **prevents overwrite** -- a work id can only be registered once
//!   (`AlreadyRegistered`);
//! - **renews TTL** -- the entry and its version snapshot are written
//!   with the CATALOG TTL tier and renewed on every read;
//! - **emits a versioned event** -- `WRK_NEW (work_id, version,
//!   metadata_hash)` so indexers can follow registration history.
//!
//! ## ABI impact
//!
//! New `register_work` entry point plus read-only `entry(entry_id)`,
//! `entry_version(entry_id, version)`, `entry_version_count(entry_id)`.
//! Existing entry points are unchanged.
//!
//! ## Storage impact
//!
//! New persistent keys: `Entry(work_id)` (current entry) and
//! `EntryVersion(work_id, 1)` (immutable v1 snapshot). No existing keys
//! are reshaped.
//!
//! ## Event impact
//!
//! `WRK_NEW` published once per registration, carrying the work id, the
//! version (always 1 at registration), and the metadata manifest hash --
//! never the manifest contents.
//!
//! ## Privacy impact
//!
//! Only the canonical id, a bounded metadata URI, the manifest hash, and
//! a pseudonymous custodian address land on-chain. Metadata contents,
//! names, and emails stay off-chain (ADR-0001 I4/I5).
//!
//! ## Deployment & migration impact
//!
//! Additive evolution of the never-deployed library-rights contract; no
//! migration required. Future schema changes bump `keys::SCHEMA_VERSION`.
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/registry.rs`: round trip, version
//! snapshot, all-zero id/hash rejection, duplicate rejection,
//! PolicyManager authorization, pre-bootstrap failure, versioned event
//! assertion, and TTL renewal on read.

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
pub struct WorkRegistry {
    pub registrant: String,
    works: std::collections::HashMap<[u8; 32], u32>, // id -> version
}
impl WorkRegistry {
    pub fn new(registrant: &str) -> Self {
        Self { registrant: registrant.to_string(), works: Default::default() }
    }
    pub fn register(&mut self, caller: &str, id: [u8; 32]) -> Result<u32, &'static str> {
        if caller != self.registrant {
            return Err("unauthorized");
        }
        if id == [0u8; 32] {
            return Err("invalid identifier");
        }
        if self.works.contains_key(&id) {
            return Err("already registered");
        }
        self.works.insert(id, 1);
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_once_and_rejects_duplicates() {
        let mut r = WorkRegistry::new("policy-manager");
        assert_eq!(r.register("policy-manager", [1u8; 32]), Ok(1));
        assert_eq!(r.register("policy-manager", [1u8; 32]), Err("already registered"));
        assert_eq!(r.register("stranger", [2u8; 32]), Err("unauthorized"));
        assert_eq!(r.register("policy-manager", [0u8; 32]), Err("invalid identifier"));
    }
}
