//! Property-based lifecycle invariants for licenses and seats (#1013).
//!
//! Example tests pick the sequences someone already thought of. This module
//! generates them: randomized actors, seat counts, timestamps, and transitions
//! are replayed against a reference model, and four invariants are re-checked
//! after **every** action rather than only at the end.
//!
//! ## Invariants
//!
//! 1. **Seat conservation** — `allocated <= total` always holds, and
//!    `allocated` equals the number of accepted allocations minus the number of
//!    accepted releases. Supply is never created or destroyed by a failed call.
//! 2. **One terminal outcome** — a license reaches `Revoked` at most once;
//!    revocation is irreversible and a second revocation is rejected.
//! 3. **Bounded allocation** — allocation past `total_seats` is always refused
//!    with `NoSeatsAvailable`, and release at zero with `NoSeatsAllocated`.
//!    Neither boundary can be crossed by any interleaving.
//! 4. **Reproducibility** — every sequence is generated from an explicit seed,
//!    so a CI failure names the seed and replays exactly.
//!
//! ## Why release stays open on a dead license
//!
//! `release_seat` deliberately works on expired and revoked licenses so seats
//! can always be cleaned up. The model encodes that asymmetry rather than
//! assuming symmetry with `allocate_seat`, and
//! [`seats_can_be_reclaimed_after_revocation`] pins it.
//!
//! ## Impact
//!
//! Test-only. No ABI, storage, event, privacy, deployment, or migration impact.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, String,
};

use crate::{LibraryLicensingClient, LicenseError, LicenseStatus};

/// Actions per generated sequence.
const SEQUENCE_LEN: usize = 150;
/// Licenses always open at this timestamp.
const NOT_BEFORE: u64 = 1_000;
/// ...and close at this one.
const EXPIRES_AT: u64 = 2_000;

// ── Reference model ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    /// The licensee allocates one seat.
    Allocate,
    /// The licensee releases one seat.
    Release,
    /// A non-licensee attempts to allocate.
    AllocateAsStranger,
    /// The admin revokes the license.
    Revoke,
    /// Move the ledger clock to `to`.
    SetTime { to: u64 },
}

struct Model {
    now: u64,
    total_seats: u32,
    allocated: u32,
    revoked: bool,
    /// Counts used to prove seat conservation independently of `allocated`.
    accepted_allocations: u32,
    accepted_releases: u32,
    revocations: u32,
}

impl Model {
    fn new(total_seats: u32, now: u64) -> Self {
        Self {
            now,
            total_seats,
            allocated: 0,
            revoked: false,
            accepted_allocations: 0,
            accepted_releases: 0,
            revocations: 0,
        }
    }

    fn apply(&mut self, action: Action) -> Outcome {
        match action {
            Action::Allocate => {
                // Mirrors `allocate_seat`: window, then status, then supply.
                if self.now < NOT_BEFORE {
                    return Outcome::Rejected; // NotYetActive
                }
                if self.now >= EXPIRES_AT {
                    return Outcome::Rejected; // Expired
                }
                if self.revoked {
                    return Outcome::Rejected; // LicenseRevoked
                }
                if self.allocated >= self.total_seats {
                    return Outcome::Rejected; // NoSeatsAvailable
                }
                self.allocated += 1;
                self.accepted_allocations += 1;
                Outcome::Accepted
            }

            Action::Release => {
                // `release_seat` checks only supply, deliberately staying
                // available on expired and revoked licenses.
                if self.allocated == 0 {
                    return Outcome::Rejected; // NoSeatsAllocated
                }
                self.allocated -= 1;
                self.accepted_releases += 1;
                Outcome::Accepted
            }

            Action::AllocateAsStranger => Outcome::Rejected, // Unauthorized

            Action::Revoke => {
                if self.revoked {
                    return Outcome::Rejected; // LicenseRevoked
                }
                self.revoked = true;
                self.revocations += 1;
                Outcome::Accepted
            }

            Action::SetTime { to } => {
                self.now = to;
                Outcome::Accepted
            }
        }
    }

    /// Re-checked after every action.
    fn assert_invariants(&self, seed: u64, step: usize) {
        // 1. Seat conservation.
        assert!(
            self.allocated <= self.total_seats,
            "seed {seed} step {step}: {} of {} seats allocated",
            self.allocated,
            self.total_seats
        );
        assert_eq!(
            self.allocated,
            self.accepted_allocations - self.accepted_releases,
            "seed {seed} step {step}: allocation ledger does not reconcile"
        );
        // 2. One terminal outcome.
        assert!(
            self.revocations <= 1,
            "seed {seed} step {step}: license revoked {} times",
            self.revocations
        );
    }
}

// ── Deterministic generation ───────────────────────────────────────────────

/// xorshift64*, so every sequence replays exactly from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn generate(rng: &mut Rng) -> Action {
    match rng.below(20) {
        0..=8 => Action::Allocate,
        9..=14 => Action::Release,
        15 => Action::AllocateAsStranger,
        16 => Action::Revoke,
        // Straddle both window boundaries so NotYetActive, active, and Expired
        // are all exercised.
        _ => Action::SetTime {
            to: rng.below(3_000),
        },
    }
}

// ── Harness ────────────────────────────────────────────────────────────────

struct Harness<'a> {
    env: Env,
    client: LibraryLicensingClient<'a>,
    admin: Address,
    licensee: Address,
    stranger: Address,
    license_id: BytesN<32>,
}

fn harness(total_seats: u32) -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(crate::LibraryLicensing, ());
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.set_admin(&admin);

    // Grant inside the validity window so the license starts usable.
    env.ledger().set_timestamp(1_500);
    let licensee = Address::generate(&env);
    let license_id = client.grant_license(
        &admin,
        &BytesN::from_array(&env, &[9u8; 32]),
        &licensee,
        &String::from_str(&env, "read"),
        &NOT_BEFORE,
        &EXPIRES_AT,
        &total_seats,
    );

    let stranger = Address::generate(&env);

    Harness {
        env,
        client,
        admin,
        licensee,
        stranger,
        license_id,
    }
}

impl Harness<'_> {
    fn execute(&self, action: Action) -> Outcome {
        let ok = match action {
            Action::Allocate => self
                .client
                .try_allocate_seat(&self.licensee, &self.license_id)
                .is_ok(),
            Action::Release => self
                .client
                .try_release_seat(&self.licensee, &self.license_id)
                .is_ok(),
            Action::AllocateAsStranger => self
                .client
                .try_allocate_seat(&self.stranger, &self.license_id)
                .is_ok(),
            Action::Revoke => self
                .client
                .try_revoke_license(&self.admin, &self.license_id)
                .is_ok(),
            Action::SetTime { to } => {
                self.env.ledger().set_timestamp(to);
                true
            }
        };
        if ok {
            Outcome::Accepted
        } else {
            Outcome::Rejected
        }
    }

    fn allocated(&self) -> u32 {
        self.client.license(&self.license_id).allocated_seats
    }

    fn total(&self) -> u32 {
        self.client.license(&self.license_id).total_seats
    }

    fn revoked(&self) -> bool {
        self.client.license(&self.license_id).status == LicenseStatus::Revoked
    }
}

fn run_sequence(seed: u64, total_seats: u32) {
    let h = harness(total_seats);
    let mut model = Model::new(total_seats, h.env.ledger().timestamp());
    let mut rng = Rng(seed);

    for step in 0..SEQUENCE_LEN {
        let action = generate(&mut rng);
        let expected = model.apply(action);
        let actual = h.execute(action);

        assert_eq!(
            expected, actual,
            "seed {seed} step {step}: model and contract disagree on {action:?}"
        );

        model.assert_invariants(seed, step);

        // The contract's own state must track the model after every action,
        // including the rejected ones — a failed call that nudged the seat
        // counter would surface here rather than at the end.
        assert_eq!(
            h.allocated(),
            model.allocated,
            "seed {seed} step {step}: seat count diverged after {action:?}"
        );
        assert_eq!(
            h.total(),
            model.total_seats,
            "seed {seed} step {step}: total seats changed"
        );
        assert_eq!(
            h.revoked(),
            model.revoked,
            "seed {seed} step {step}: revocation state diverged"
        );
        assert!(
            h.allocated() <= h.total(),
            "seed {seed} step {step}: contract allocated more seats than it has"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Generated sequences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn seat_lifecycle_holds_across_randomized_sequences() {
    for seed in [1u64, 3, 11, 97, 1_009, 65_537] {
        // A one-seat license spends most of its time at the boundary; a larger
        // one exercises the interior.
        for total_seats in [1u32, 2, 5] {
            run_sequence(seed, total_seats);
        }
    }
}

#[test]
fn a_seed_replays_to_the_same_result() {
    run_sequence(20_250_924, 3);
    run_sequence(20_250_924, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// Named boundary and terminal-state properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn allocation_never_exceeds_total_seats() {
    let h = harness(2);

    h.client.allocate_seat(&h.licensee, &h.license_id);
    h.client.allocate_seat(&h.licensee, &h.license_id);

    assert_eq!(
        h.client.try_allocate_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::NoSeatsAvailable))
    );
    assert_eq!(h.allocated(), 2);
}

#[test]
fn release_at_zero_is_refused_and_cannot_underflow() {
    let h = harness(2);

    assert_eq!(
        h.client.try_release_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::NoSeatsAllocated))
    );
    assert_eq!(h.allocated(), 0);
}

#[test]
fn a_rejected_allocation_leaves_the_seat_count_untouched() {
    let h = harness(1);
    h.client.allocate_seat(&h.licensee, &h.license_id);

    for _ in 0..5 {
        assert!(h.client.try_allocate_seat(&h.licensee, &h.license_id).is_err());
    }
    assert_eq!(h.allocated(), 1);

    // And the single seat is still releasable exactly once.
    h.client.release_seat(&h.licensee, &h.license_id);
    assert_eq!(h.allocated(), 0);
    assert!(h.client.try_release_seat(&h.licensee, &h.license_id).is_err());
}

#[test]
fn revocation_is_terminal_and_happens_at_most_once() {
    let h = harness(3);

    h.client.revoke_license(&h.admin, &h.license_id);
    assert!(h.revoked());

    assert_eq!(
        h.client.try_revoke_license(&h.admin, &h.license_id),
        Err(Ok(LicenseError::LicenseRevoked))
    );
    assert!(h.revoked());
}

#[test]
fn a_revoked_license_allocates_no_further_seats() {
    let h = harness(3);
    h.client.allocate_seat(&h.licensee, &h.license_id);
    h.client.revoke_license(&h.admin, &h.license_id);

    assert_eq!(
        h.client.try_allocate_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::LicenseRevoked))
    );
    assert_eq!(h.allocated(), 1);
}

#[test]
fn seats_can_be_reclaimed_after_revocation() {
    // Release stays open on a dead license so supply is never stranded.
    let h = harness(3);
    h.client.allocate_seat(&h.licensee, &h.license_id);
    h.client.allocate_seat(&h.licensee, &h.license_id);
    h.client.revoke_license(&h.admin, &h.license_id);

    h.client.release_seat(&h.licensee, &h.license_id);
    h.client.release_seat(&h.licensee, &h.license_id);
    assert_eq!(h.allocated(), 0);
}

#[test]
fn seats_can_be_reclaimed_after_expiry() {
    let h = harness(3);
    h.client.allocate_seat(&h.licensee, &h.license_id);

    h.env.ledger().set_timestamp(EXPIRES_AT);
    assert_eq!(
        h.client.try_allocate_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::Expired))
    );

    h.client.release_seat(&h.licensee, &h.license_id);
    assert_eq!(h.allocated(), 0);
}

#[test]
fn the_validity_window_is_closed_at_the_start_and_open_at_the_end() {
    let h = harness(3);

    // One second before `not_before`: not yet active.
    h.env.ledger().set_timestamp(NOT_BEFORE - 1);
    assert_eq!(
        h.client.try_allocate_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::NotYetActive))
    );

    // Exactly `not_before`: active.
    h.env.ledger().set_timestamp(NOT_BEFORE);
    assert!(h.client.try_allocate_seat(&h.licensee, &h.license_id).is_ok());

    // Exactly `expires_at`: already expired (the contract compares with `>=`).
    h.env.ledger().set_timestamp(EXPIRES_AT);
    assert_eq!(
        h.client.try_allocate_seat(&h.licensee, &h.license_id),
        Err(Ok(LicenseError::Expired))
    );
}

#[test]
fn a_stranger_can_never_move_the_seat_counter() {
    let h = harness(3);

    assert_eq!(
        h.client.try_allocate_seat(&h.stranger, &h.license_id),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_eq!(
        h.client.try_release_seat(&h.stranger, &h.license_id),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_eq!(h.allocated(), 0);
}
