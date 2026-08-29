//! Fix for #942: represent per-format entitlements.
//!
//! ## Problem
//!
//! A publisher may license EPUB, PDF, audio, or accessible renditions of
//! a work independently. A license with only a generic `rights` string
//! cannot express "Borrow of EPUB only" vs "Accessible alternative of
//! EPUB", so one format could accidentally unlock another.
//!
//! ## Solution
//!
//! `contracts/library_licensing` now binds entitlements to explicit
//! rendition ids and allowed access modes:
//!
//! - `grant_entitlement(caller, license_id, rendition_id, access_mode)`
//!   — admin-gated. Entitlements are keyed by the exact
//!   `(rendition_id, access_mode)` pair, so granting `Borrow` on one
//!   rendition never unlocks another rendition **or** mode, while an
//!   `AccessibleAlternative` can be granted intentionally without
//!   disturbing the primary entitlement. Re-granting the same pair is
//!   idempotent.
//! - `revoke_entitlement(caller, license_id, rendition_id, access_mode)`
//!   — admin-gated removal of one pair.
//! - `is_entitled(license_id, rendition_id, access_mode)` — read-only;
//!   returns `Ok(false)` when the license is inactive or the exact pair
//!   is not granted.
//! - `entitlement(...)` (single read), `entitlements(license_id, from,
//!   limit)` (bounded pagination), `entitlements_len(license_id)` — the
//!   mapping is always queryable within bounds (windows clamp, never
//!   panic).
//!
//! ## ABI impact
//!
//! Adds `AccessMode` (`Borrow`, `AccessibleAlternative`), `Entitlement`,
//! and the functions above to `contracts/library_licensing`. New
//! `LicenseError` variants: `EntitlementNotFound = 15`, `Overflow = 16`.
//! Existing entry points are unchanged.
//!
//! ## Storage impact
//!
//! Persistent keys: `Entitlement(license_id, rendition_id, access_mode)`
//! -> `Entitlement`, and `EntitlementKeys(license_id)` -> ordered
//! `Vec<(Symbol, AccessMode)>` for bounded queries, each TTL-extended
//! with the license TTL constants.
//!
//! ## Event impact
//!
//! `ENT_GRANT` (license_id, rendition_id, access_mode, index),
//! `ENT_UPD` (idempotent re-grant), `ENT_REVK` (license_id, rendition_id,
//! access_mode). No content or reader data is emitted.
//!
//! ## Privacy impact
//!
//! Only rendition ids, access modes, and timestamps land on-chain — no
//! reader identity or reading history (ADR-0001 I4/I5).
//!
//! ## Deployment & migration impact
//!
//! `library_licensing` has never been deployed; existing tests were
//! updated for the additive ABI. Key shapes are stable per license;
//! future changes must not reuse keys (I3/I5).
//!
//! ## Tests
//!
//! `contracts/library_licensing/src/tests/entitlements.rs` covers exact
//! pair matching, cross-format and cross-mode isolation, intentional
//! accessible alternatives, idempotent re-grant, bounded pagination,
//! revocation, authorization, missing-license/entitlement errors, and
//! window/revocation boundaries.
use std::collections::HashMap;

/// Illustrative core model (see `contracts/library_licensing` for the
/// deployable Soroban contract).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessMode {
    Borrow,
    AccessibleAlternative,
}

pub struct EntitlementLedger {
    admin: String,
    /// license -> (rendition, mode) -> granted
    map: HashMap<u64, HashMap<(String, AccessMode), bool>>,
}
impl EntitlementLedger {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), map: HashMap::new() }
    }
    pub fn grant(
        &mut self,
        caller: &str,
        license: u64,
        rendition: &str,
        mode: AccessMode,
    ) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("unauthorized");
        }
        self.map
            .entry(license)
            .or_default()
            .insert((rendition.to_string(), mode), true);
        Ok(())
    }
    pub fn is_entitled(&self, license: u64, rendition: &str, mode: AccessMode) -> bool {
        self.map
            .get(&license)
            .and_then(|m| m.get(&(rendition.to_string(), mode)))
            .copied()
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowing_one_format_does_not_unlock_another() {
        let mut ledger = EntitlementLedger::new("admin");
        ledger.grant("admin", 1, "EPUB", AccessMode::Borrow).unwrap();
        assert!(ledger.is_entitled(1, "EPUB", AccessMode::Borrow));
        assert!(!ledger.is_entitled(1, "PDF", AccessMode::Borrow));
        assert!(!ledger.is_entitled(1, "EPUB", AccessMode::AccessibleAlternative));
    }

    #[test]
    fn accessible_alternative_granted_intentionally() {
        let mut ledger = EntitlementLedger::new("admin");
        ledger.grant("admin", 1, "EPUB", AccessMode::Borrow).unwrap();
        ledger
            .grant("admin", 1, "EPUB", AccessMode::AccessibleAlternative)
            .unwrap();
        assert!(ledger.is_entitled(1, "EPUB", AccessMode::Borrow));
        assert!(ledger.is_entitled(1, "EPUB", AccessMode::AccessibleAlternative));
    }
}
