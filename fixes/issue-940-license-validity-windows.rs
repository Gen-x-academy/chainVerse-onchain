//! Fix for #940: enforce license validity windows.
//!
//! ## Problem
//!
//! Digital rights need exact start and expiry behavior at ledger boundaries.
//! Licenses and any access grants derived from them must validate their
//! `not_before` / `expires-at` timestamps on every use, and every timestamp
//! computation must use checked arithmetic so an overflow fails
//! deterministically instead of wrapping to a timestamp in the past (the
//! release profile enables `overflow-checks`, which would panic on overflow).
//!
//! ## Solution
//!
//! New `contracts/library_licensing` Soroban contract (issue #940) with:
//!
//! - `grant_license` — admin-gated issuance of a license with an explicit
//!   window. `not_before` is **inclusive**, `expires_at` is **exclusive**;
//!   zero-length (`not_before == expires_at`) and inverted windows are
//!   rejected (`InvalidWindow`).
//! - `derive_access_grant` — the licensee delegates access for `duration`
//!   seconds. The grant window starts at the current ledger timestamp and is
//!   **clamped** to the parent license's window, so a derived grant can never
//!   start before or outlive the rights it derives from. The duration is
//!   added with `checked_add` (`WindowOverflow` on overflow).
//! - `revoke_license` — admin-gated revocation; existing grants are
//!   invalidated immediately because `is_grant_active` re-checks the parent
//!   license on every read.
//! - `is_license_active` / `is_grant_active` — read-only boundary checks
//!   (active iff `not_before <= now < expires_at` and status is Active).
//! - `license` / `access_grant` — read-only getters.
//!
//! Authorization follows the staking `require_admin` pattern: the caller is
//! compared explicitly to the stored admin (`Unauthorized` on mismatch) and
//! then must pass `require_auth`, so the unauthorized branch is reachable
//! and testable. License ids and grant ids are collision-resistant hashes of
//! a monotonic instance nonce + ledger timestamp + caller salt (ADR-0001 I3).
//!
//! ## Boundary semantics (before / at / after)
//!
//! ```text
//!        not_before                expires_at
//!            |<---- ACTIVE --------|            now < not_before  -> inactive / NotYetActive
//!            |                     |            now == not_before -> active  (inclusive)
//!            |                     |            now == expires_at -> inactive / Expired (exclusive)
//!            |                     |            now >  expires_at -> inactive / Expired
//! ```
//!
//! ## ABI impact
//!
//! New `#![no_std]` Soroban contract with a generated `Client` per
//! `#[contractimpl]` (no hand-rolled ABI). State-changing entry points:
//! `set_admin(Address)`, `upgrade(Address, BytesN<32>)`,
//! `grant_license(Address, BytesN<32>, Address, String, u64, u64)`,
//! `revoke_license(Address, BytesN<32>)`,
//! `derive_access_grant(Address, BytesN<32>, Address, u64)`. Read-only:
//! `is_license_active`, `is_grant_active`, `license`, `access_grant`.
//! Every entry point returns `Result<_, LicenseError>` with unique
//! discriminants (1..=11); no panics on the error paths.
//!
//! ## Storage impact
//!
//! - Instance: `Admin`, monotonic `LicenseCount` / `GrantCount` nonces.
//! - Persistent: `License(BytesN<32>)` and `AccessGrant(BytesN<32>)`,
//!   each written with an explicit TTL (`LICENSE_MIN_TTL`..`LICENSE_MAX_TTL`),
//!   matching the vault storage pattern.
//!
//! ## Event impact
//!
//! Every mutation publishes a `symbol_short!` topic: `LIC_NEW`
//! (id, licensee, not_before, expires_at), `LIC_REVK` (id), `GRANT_NEW`
//! (grant_id, license_id, grantee, not_before, expires_at), `upgraded`
//! (wasm hash). No sensitive legal text is ever emitted.
//!
//! ## Privacy impact
//!
//! Only consensus-relevant facts are on-chain (license window, grant
//! windows, status). No content bytes, reading history, or identity
//! attestations are stored (ADR-0001 I4/I5).
//!
//! ## Deployment & migration impact
//!
//! New contract, deployed separately; no existing storage layout is
//! modified. Upgrades use the admin-gated
//! `deployer().update_current_contract_wasm` path with a reviewed plan.
//! Record keys are content-derived and never reused (I3/I5), so a future
//! schema change must bump keys rather than mutate in place.
//!
//! ## Tests
//!
//! `contracts/library_licensing/src/tests.rs` covers positive, negative,
//! authorization, and before/at/after boundary cases, including checked-
//! arithmetic overflow (`duration = u64::MAX` -> `WindowOverflow`) and
//! grant clamping to the license expiry.
use std::collections::HashMap;

/// Illustrative core model (see `contracts/library_licensing` for the
/// deployable Soroban contract).
pub struct LicensingLedger {
    pub admin: String,
    next_id: u64,
    licenses: HashMap<u64, (u64, u64, bool)>, // id -> (not_before, expires_at, active)
}
impl LicensingLedger {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), next_id: 0, licenses: HashMap::new() }
    }
    pub fn grant(&mut self, caller: &str, not_before: u64, expires_at: u64) -> Result<u64, &'static str> {
        if caller != self.admin {
            return Err("unauthorized");
        }
        if not_before >= expires_at {
            return Err("invalid window"); // zero-length or inverted window
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or("window overflow")?;
        self.licenses.insert(id, (not_before, expires_at, true));
        Ok(id)
    }
    pub fn is_active(&self, id: u64, now: u64) -> Result<bool, &'static str> {
        match self.licenses.get(&id) {
            Some((nb, ex, active)) => Ok(*active && now >= *nb && now < *ex), // inclusive start, exclusive end
            None => Err("not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_at_start_inclusive_end_exclusive() {
        let mut l = LicensingLedger::new("admin");
        let id = l.grant("admin", 1000, 2000).unwrap();
        assert!(!l.is_active(id, 999).unwrap());
        assert!(l.is_active(id, 1000).unwrap()); // at not_before: active
        assert!(l.is_active(id, 1999).unwrap());
        assert!(!l.is_active(id, 2000).unwrap()); // at expires_at: expired
    }
}
