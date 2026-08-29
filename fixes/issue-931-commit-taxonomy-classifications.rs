//! Fix for #931: commit taxonomy and audience classifications.
//!
//! ## Problem
//!
//! Subjects, genres, languages, and audience ratings are managed off-chain
//! as versioned classification manifests, but the chain has no integrity
//! anchor for them: nothing proves a given manifest hash was the one the
//! registry committed at a given time, and corrections would silently
//! overwrite prior versions.
//!
//! ## Solution
//!
//! The `library-rights` registry gains a `commit_classification` entry
//! point that commits only the off-chain manifest hash plus its schema
//! version and the issuing role-holder:
//!
//! - `commit_classification(caller, kind, manifest_hash, schema_version)`
//!   — role-gated to `PolicyManager`. An all-zero hash is rejected
//!   (`InvalidHash`) so a commitment always carries a real content
//!   address. The previous commitment (if any) is preserved: the new
//!   record is appended to an immutable per-kind history and linked
//!   through `previous_hash`, and the `CLS_NEW` event carries both the
//!   old and the new hash for indexers.
//! - `get_classification(kind)` — returns the current commitment.
//! - `classification_history(kind, index)` / `classification_history_len(kind)`
//!   — bounded, append-only reads for indexers and audits.
//!
//! Two kinds are supported: `ClassificationKind::Taxonomy` (subjects,
//! genres, languages) and `ClassificationKind::Audience` (ratings).
//!
//! ## ABI impact
//!
//! Adds `commit_classification(Address, ClassificationKind, BytesN<32>,
//! u32)`, `get_classification(ClassificationKind)`,
//! `classification_history_len(ClassificationKind)`, and
//! `classification_history(ClassificationKind, u64)` to
//! `contracts/library-rights`. New `ContractError` variants:
//! `InvalidHash = 6`, `ClassificationNotFound = 7`, `Overflow = 9`.
//! Contract version bumped to `0.5.0`.
//!
//! ## Storage impact
//!
//! Persistent keys per [`DataKey`](crate::keys::DataKey):
//! `Classification(kind)` (current), `ClassificationCount(kind)`
//! (history length), `ClassificationHistory(kind, index)` (append-only),
//! each TTL-tiered with the catalog constants
//! (`CATALOG_MIN_TTL`..`CATALOG_MAX_TTL`) and renewed on read/write.
//!
//! ## Event impact
//!
//! `CLS_NEW` — `(kind, old_hash, new_hash, schema_version, issuer)`.
//! Old hash is all-zeros for the first commit of a kind. Only hashes and
//! role addresses are emitted — never manifest contents.
//!
//! ## Privacy impact
//!
//! Only hashes, schema versions, timestamps, and issuer addresses land
//! on-chain (ADR-0001 I4). Manifest contents (subjects, genres,
//! languages, ratings) remain off-chain in chainVerse-backend #983.
//!
//! ## Deployment & migration impact
//!
//! `library-rights` has never been deployed, so this is a pre-release
//! ABI evolution. History is append-only and keyed by a monotonic
//! 1-based index; a future schema change must bump
//! `SCHEMA_VERSION`/keys rather than mutate in place (I3/I5).
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/classifications.rs` covers
//! positive round trips, provenance-preserving updates, per-kind
//! independence, zero-hash rejection, role authorization, missing-bootstrap
//! failure, and out-of-bounds history reads.
use std::collections::HashMap;

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
pub struct ClassificationRegistry {
    admin: String,
    /// kind -> (current_hash, schema_version, previous_hash)
    current: HashMap<String, (String, u32, Option<String>)>,
    /// kind -> append-only history of manifest hashes
    history: HashMap<String, Vec<String>>,
}
impl ClassificationRegistry {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), current: HashMap::new(), history: HashMap::new() }
    }
    pub fn commit(
        &mut self,
        caller: &str,
        kind: &str,
        hash: &str,
        schema: u32,
    ) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("unauthorized");
        }
        if hash.is_empty() || hash.chars().all(|c| c == '0') {
            return Err("invalid hash");
        }
        let prev = self.current.get(kind).map(|(h, _, _)| h.clone());
        self.history.entry(kind.to_string()).or_default().push(hash.to_string());
        self.current.insert(kind.to_string(), (hash.to_string(), schema, prev));
        Ok(())
    }
    pub fn current_hash(&self, kind: &str) -> Option<(&str, u32, Option<&str>)> {
        self.current
            .get(kind)
            .map(|(h, s, p)| (h.as_str(), *s, p.as_deref()))
    }
    pub fn history_len(&self, kind: &str) -> usize {
        self.history.get(kind).map_or(0, |v| v.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_append_and_preserve_provenance() {
        let mut reg = ClassificationRegistry::new("pm");
        reg.commit("pm", "taxonomy", "aaa", 1).unwrap();
        reg.commit("pm", "taxonomy", "bbb", 2).unwrap();
        assert_eq!(reg.history_len("taxonomy"), 2);
        let (cur, schema, prev) = reg.current_hash("taxonomy").unwrap();
        assert_eq!(cur, "bbb");
        assert_eq!(schema, 2);
        assert_eq!(prev, Some("aaa"));
    }

    #[test]
    fn malformed_hash_rejected() {
        let mut reg = ClassificationRegistry::new("pm");
        assert_eq!(reg.commit("pm", "taxonomy", "000", 1), Err("invalid hash"));
    }
}
