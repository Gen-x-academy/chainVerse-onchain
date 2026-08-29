//! Fix for #934: record acquisition and donation provenance hashes.
//!
//! ## Problem
//!
//! The future library flow needs auditable provenance — how each work was
//! acquired (purchase, donation, or institutional acquisition) — without
//! publishing donor names, invoice numbers, or agreement text on-chain.
//!
//! ## Solution
//!
//! The `library-rights` registry gains `attest_provenance`, which commits
//! only the hash of the off-chain provenance document:
//!
//! - `attest_provenance(caller, work_id, provenance_type, provenance_hash)`
//!   — role-gated to `PolicyManager` (only authorized roles can attest).
//!   The all-zero hash is rejected (`InvalidHash`). Private document
//!   contents stay off-chain (ADR-0001 I4); only the hash, the
//!   acquisition type, the attestor, and a timestamp are stored.
//! - Corrections **append** a new record linked through `previous_hash`
//!   instead of overwriting history, and the `PROV_NEW` event carries the
//!   old and new hashes so every correction is auditable.
//! - `provenance_len(work_id)` and `get_provenance(work_id, index)`
//!   expose the append-only history with bounds-checked reads.
//!
//! Three acquisition types are supported: `Purchase`, `Donation`, and
//! `InstitutionalAcquisition`.
//!
//! ## ABI impact
//!
//! Adds `attest_provenance(Address, BytesN<32>, ProvenanceType,
//! BytesN<32>)`, `provenance_len(BytesN<32>)`, and
//! `get_provenance(BytesN<32>, u64)` to `contracts/library-rights`. New
//! `ContractError` variants: `InvalidHash = 6`, `ProvenanceNotFound = 8`,
//! `Overflow = 9`. Contract version bumped to `0.5.0`.
//!
//! ## Storage impact
//!
//! Persistent keys: `ProvenanceCount(work_id)` (history length) and
//! `Provenance(work_id, index)` (append-only records), each TTL-tiered
//! with the catalog constants and renewed on read/write.
//!
//! ## Event impact
//!
//! `PROV_NEW` — `(work_id, provenance_type, old_hash, new_hash,
//! attested_by, attested_at)`. Old hash is all-zeros for the first
//! record. Donor/invoice details are never emitted.
//!
//! ## Privacy impact
//!
//! `ProvenanceRecord` carries no donor, invoice, or identity fields — a
//! structural test (`test_provenance_record_holds_only_public_facts`)
//! destructures the full field set so adding a privacy-violating field
//! stops compiling.
//!
//! ## Deployment & migration impact
//!
//! `library-rights` has never been deployed, so this is a pre-release ABI
//! evolution. History is append-only and keyed by a monotonic 1-based
//! index; future schema changes must bump `SCHEMA_VERSION`/keys (I3/I5).
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/provenance.rs` covers positive
//! round trips, per-work isolation, append-not-overwrite corrections,
//! zero-hash rejection, role authorization, missing-bootstrap failure,
//! out-of-bounds reads, ledger-timestamp capture, and the privacy
//! structural guarantee.
use std::collections::HashMap;

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
#[derive(Clone, PartialEq, Debug)]
pub enum ProvenanceType {
    Purchase,
    Donation,
    InstitutionalAcquisition,
}

pub struct ProvenanceLedger {
    admin: String,
    /// work -> append-only provenance hashes
    history: HashMap<u64, Vec<String>>,
}
impl ProvenanceLedger {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), history: HashMap::new() }
    }
    pub fn attest(
        &mut self,
        caller: &str,
        work: u64,
        provenance_type: ProvenanceType,
        hash: &str,
    ) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("unauthorized");
        }
        if hash.is_empty() || hash.chars().all(|c| c == '0') {
            return Err("invalid hash");
        }
        let _ = provenance_type;
        self.history.entry(work).or_default().push(hash.to_string());
        Ok(())
    }
    pub fn len(&self, work: u64) -> usize {
        self.history.get(&work).map_or(0, |v| v.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_append_instead_of_overwrite() {
        let mut ledger = ProvenanceLedger::new("pm");
        ledger.attest("pm", 1, ProvenanceType::Donation, "aaa").unwrap();
        ledger.attest("pm", 1, ProvenanceType::Donation, "bbb").unwrap();
        assert_eq!(ledger.len(1), 2);
    }

    #[test]
    fn unauthorized_attestation_rejected() {
        let mut ledger = ProvenanceLedger::new("pm");
        assert_eq!(
            ledger.attest("other", 1, ProvenanceType::Purchase, "aaa"),
            Err("unauthorized")
        );
    }
}
