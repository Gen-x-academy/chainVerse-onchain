//! Fix for #943: track concurrent digital seats.
//!
//! ## Problem
//!
//! The repository has no atomic accounting for limited simultaneous
//! digital loans: nothing prevents a license from being used by more
//! concurrent readers than the publisher sold, and nothing guarantees a
//! returned seat is restored exactly once.
//!
//! ## Solution
//!
//! `contracts/library_licensing` licenses now carry a concurrent-seat
//! budget with checked arithmetic:
//!
//! - `grant_license(..., total_seats)` — the budget is fixed at issuance
//!   and must be `> 0` (`InvalidSeats`). `allocated_seats` starts at 0,
//!   so `allocated <= total` holds from the first lifecycle path.
//! - `allocate_seat(caller, license_id)` — licensee-gated. Uses
//!   `checked_add`; once `allocated == total`, competing calls fail with
//!   `NoSeatsAvailable` (allocation can never exceed supply). Allocation
//!   is rejected outside the license window (`NotYetActive`/`Expired`)
//!   or after revocation.
//! - `release_seat(caller, license_id)` — licensee-gated. Uses
//!   `checked_sub`; releasing with nothing allocated fails with
//!   `NoSeatsAllocated` (underflow guard). Release stays available on
//!   expired/revoked licenses so seats can always be cleaned up, and it
//!   restores exactly one unit of supply.
//! - `available_seats(license_id)` — read-only `total - allocated`,
//!   always non-negative by the invariant.
//!
//! ## ABI impact
//!
//! `grant_license` gains a trailing `total_seats: u32` parameter; new
//! entry points `allocate_seat(Address, BytesN<32>)`,
//! `release_seat(Address, BytesN<32>)`, `available_seats(BytesN<32>)`.
//! New `LicenseError` variants: `InvalidSeats = 12`,
//! `NoSeatsAvailable = 13`, `NoSeatsAllocated = 14`. `License` gains
//! `total_seats`/`allocated_seats` fields.
//!
//! ## Storage impact
//!
//! Seat counters live on the existing `License(BytesN<32>)` persistent
//! record (extended TTL on every seat mutation). No new keys.
//!
//! ## Event impact
//!
//! `SEAT_NEW` (license_id, allocated, total) on allocation and
//! `SEAT_RELS` (license_id, allocated, total) on release, so indexers can
//! track seat utilization over time.
//!
//! ## Privacy impact
//!
//! Only aggregate seat counts are on-chain — never who is borrowing or
//! what they are reading (ADR-0001 I4/I5).
//!
//! ## Deployment & migration impact
//!
//! `library_licensing` has never been deployed; the `License` struct
//! evolution is pre-release. No live storage is migrated.
//!
//! ## Tests
//!
//! `contracts/library_licensing/src/tests/seats.rs` covers allocation/
//! release round trips, supply exhaustion (competing calls), zero-budget
//! rejection, underflow guard, authorization, not-yet-active/expired/
//! revoked allocation rejection, release-after-expiry cleanup, and the
//! `available >= 0` invariant across every lifecycle path.
use std::collections::HashMap;

/// Illustrative core model (see `contracts/library_licensing` for the
/// deployable Soroban contract).
pub struct SeatLedger {
    admin: String,
    licensee: HashMap<u64, String>,
    total: HashMap<u64, u32>,
    allocated: HashMap<u64, u32>,
}
impl SeatLedger {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), licensee: HashMap::new(), total: HashMap::new(), allocated: HashMap::new() }
    }
    pub fn grant(&mut self, caller: &str, id: u64, licensee: &str, total: u32) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("unauthorized");
        }
        if total == 0 {
            return Err("invalid seats");
        }
        self.licensee.insert(id, licensee.to_string());
        self.total.insert(id, total);
        self.allocated.insert(id, 0);
        Ok(())
    }
    pub fn allocate(&mut self, caller: &str, id: u64) -> Result<(), &'static str> {
        if self.licensee.get(&id) != Some(&caller.to_string()) {
            return Err("unauthorized");
        }
        let a = self.allocated.get(&id).copied().unwrap_or(0);
        let t = self.total.get(&id).copied().unwrap_or(0);
        if a >= t {
            return Err("no seats available");
        }
        self.allocated.insert(id, a.checked_add(1).unwrap());
        Ok(())
    }
    pub fn release(&mut self, caller: &str, id: u64) -> Result<(), &'static str> {
        if self.licensee.get(&id) != Some(&caller.to_string()) {
            return Err("unauthorized");
        }
        let a = self.allocated.get(&id).copied().unwrap_or(0);
        if a == 0 {
            return Err("no seats allocated");
        }
        self.allocated.insert(id, a.checked_sub(1).unwrap());
        Ok(())
    }
    pub fn available(&self, id: u64) -> u32 {
        self.total.get(&id).copied().unwrap_or(0) - self.allocated.get(&id).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_cannot_exceed_supply_and_release_restores_exactly_one() {
        let mut ledger = SeatLedger::new("admin");
        ledger.grant("admin", 1, "lib", 2).unwrap();
        ledger.allocate("lib", 1).unwrap();
        ledger.allocate("lib", 1).unwrap();
        assert_eq!(ledger.allocate("lib", 1), Err("no seats available"));
        assert_eq!(ledger.available(1), 0);
        ledger.release("lib", 1).unwrap();
        assert_eq!(ledger.available(1), 1);
    }

    #[test]
    fn release_with_none_allocated_rejected() {
        let mut ledger = SeatLedger::new("admin");
        ledger.grant("admin", 1, "lib", 2).unwrap();
        assert_eq!(ledger.release("lib", 1), Err("no seats allocated"));
    }
}
