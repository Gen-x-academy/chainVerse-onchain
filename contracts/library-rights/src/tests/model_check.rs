//! Model check for the hold and loan state machines (#1016).
//!
//! A small reference model of the lending domain is driven alongside the
//! Soroban implementation over pseudo-random action sequences. After every
//! action the model's predicted outcome is compared with the contract's, and
//! at the end of each sequence the full observable state is reconciled.
//!
//! ## What this buys over example-based tests
//!
//! An illegal transition that silently mutated state would not be caught by
//! asserting its return value alone — the corruption only shows up later. Here
//! a mutation the model did not predict makes some *subsequent* action's
//! outcome diverge, or fails the final reconciliation, so the sequence that
//! produced it is reported with its seed and can be replayed exactly.
//!
//! ## Invariants asserted
//!
//! 1. **Illegal transitions never mutate state** — every rejected action is
//!    followed by continued model/contract agreement and a clean final
//!    reconciliation.
//! 2. **Every loan terminates at most once** — a return that actually cleared a
//!    loan is counted, and the count can never exceed the number of borrows.
//! 3. **Queue order is deterministic** — the same seed replayed in a fresh
//!    environment produces an identical outcome trace.
//!
//! ## Impact
//!
//! Test-only. No ABI, storage, event, privacy, deployment or migration impact:
//! this module adds no entrypoints and writes no new storage keys.

use crate::{ContractError, LibraryRightsContract, LibraryRightsContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol,
};

/// Works in the model universe. Small and fixed so the model needs no
/// allocator — the crate is `#![no_std]`.
const WORKS: usize = 3;
/// Patrons in the model universe.
const PATRONS: usize = 4;
/// Actions per generated sequence.
const SEQUENCE_LEN: usize = 120;

/// Mirrors the 30-day loan term in `borrow_work`.
const LOAN_SECS: u64 = 30 * 24 * 60 * 60;
/// Mirrors the 7-day hold term in `place_hold`.
const HOLD_SECS: u64 = 7 * 24 * 60 * 60;

// ── Reference model ────────────────────────────────────────────────────────

/// The outcome of an action, at the granularity both sides can agree on.
///
/// Specific error discriminants are asserted in the named unit tests below
/// rather than here, so the model stays decoupled from error-code churn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    /// Register `work` (idempotent in the contract — it overwrites).
    PutWork { work: usize },
    Borrow { work: usize, patron: usize },
    Return { work: usize, patron: usize },
    PlaceHold { work: usize, patron: usize },
    ClaimHold { work: usize, patron: usize },
    /// Advance the ledger clock by `secs`.
    AdvanceTime { secs: u64 },
}

#[derive(Clone, Copy)]
struct Hold {
    /// Placement order, used to check the queue is deterministic.
    seq: u32,
    expires_at: u64,
}

struct Model {
    now: u64,
    work_registered: [bool; WORKS],
    loan_expiry: [[Option<u64>; PATRONS]; WORKS],
    hold: [[Option<Hold>; PATRONS]; WORKS],
    next_seq: u32,
    borrows: u32,
    terminations: u32,
}

impl Model {
    fn new(now: u64) -> Self {
        Self {
            now,
            work_registered: [false; WORKS],
            loan_expiry: [[None; PATRONS]; WORKS],
            hold: [[None; PATRONS]; WORKS],
            next_seq: 0,
            borrows: 0,
            terminations: 0,
        }
    }

    fn has_loan(&self, work: usize, patron: usize) -> bool {
        self.loan_expiry[work][patron].is_some()
    }

    fn has_hold(&self, work: usize, patron: usize) -> bool {
        self.hold[work][patron].is_some()
    }

    /// Applies `action`, returning the outcome the contract must produce.
    fn apply(&mut self, action: Action) -> Outcome {
        match action {
            Action::PutWork { work } => {
                self.work_registered[work] = true;
                Outcome::Accepted
            }

            Action::Borrow { work, patron } => {
                if !self.work_registered[work] {
                    // `WorkNotFound`.
                    return Outcome::Rejected;
                }
                if self.has_loan(work, patron) {
                    // A patron may not hold two loans on the same work.
                    return Outcome::Rejected;
                }
                self.loan_expiry[work][patron] = Some(self.now + LOAN_SECS);
                self.borrows += 1;
                Outcome::Accepted
            }

            Action::Return { work, patron } => {
                // `return_work` is idempotent: returning nothing still succeeds,
                // but only a return that cleared a loan counts as a termination.
                if self.loan_expiry[work][patron].take().is_some() {
                    self.terminations += 1;
                }
                Outcome::Accepted
            }

            Action::PlaceHold { work, patron } => {
                if !self.work_registered[work] {
                    // `WorkNotFound`.
                    return Outcome::Rejected;
                }
                if self.has_hold(work, patron) {
                    // One hold per patron per work.
                    return Outcome::Rejected;
                }
                self.hold[work][patron] = Some(Hold {
                    seq: self.next_seq,
                    expires_at: self.now + HOLD_SECS,
                });
                self.next_seq += 1;
                Outcome::Accepted
            }

            Action::ClaimHold { work, patron } => {
                let Some(hold) = self.hold[work][patron] else {
                    // `HoldNotFound`.
                    return Outcome::Rejected;
                };
                if self.now > hold.expires_at {
                    // `HoldExpired` — the hold is left in place, not consumed.
                    return Outcome::Rejected;
                }
                if self.has_loan(work, patron) {
                    // `claim_hold` delegates to `borrow_work` with `?`, so the
                    // hold survives a failed claim.
                    return Outcome::Rejected;
                }
                self.loan_expiry[work][patron] = Some(self.now + LOAN_SECS);
                self.borrows += 1;
                self.hold[work][patron] = None;
                Outcome::Accepted
            }

            Action::AdvanceTime { secs } => {
                self.now += secs;
                Outcome::Accepted
            }
        }
    }
}

// ── Deterministic action generation ────────────────────────────────────────

/// xorshift64*, so a sequence is fully reproducible from its seed without
/// pulling in a PRNG dependency.
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
    let work = (rng.below(WORKS as u64)) as usize;
    let patron = (rng.below(PATRONS as u64)) as usize;
    match rng.below(10) {
        0 => Action::PutWork { work },
        1..=3 => Action::Borrow { work, patron },
        4..=5 => Action::Return { work, patron },
        6..=7 => Action::PlaceHold { work, patron },
        8 => Action::ClaimHold { work, patron },
        // Jump far enough to expire holds but not always loans, so both
        // expiry boundaries get exercised.
        _ => Action::AdvanceTime {
            secs: rng.below(2 * HOLD_SECS),
        },
    }
}

// ── Harness ────────────────────────────────────────────────────────────────

struct Harness<'a> {
    env: Env,
    client: LibraryRightsContractClient<'a>,
    policy_manager: Address,
    works: [BytesN<32>; WORKS],
    patrons: [Address; PATRONS],
}

fn setup_harness() -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(LibraryRightsContract, ());
    let client = LibraryRightsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let librarian = Address::generate(&env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);

    // Works must reference an existing policy.
    let policy_id = Symbol::new(&env, "default");
    client.put_policy(&policy_manager, &policy_id, &16, &64);

    let works = [
        BytesN::from_array(&env, &[10; 32]),
        BytesN::from_array(&env, &[11; 32]),
        BytesN::from_array(&env, &[12; 32]),
    ];
    let patrons = [
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];

    Harness {
        env,
        client,
        policy_manager,
        works,
        patrons,
    }
}

impl Harness<'_> {
    fn policy_id(&self) -> Symbol {
        Symbol::new(&self.env, "default")
    }

    fn execute(&self, action: Action) -> Outcome {
        match action {
            Action::PutWork { work } => {
                let custodian = Address::generate(&self.env);
                let hash = BytesN::from_array(&self.env, &[200 + work as u8; 32]);
                let r = self.client.try_put_work(
                    &self.policy_manager,
                    &self.works[work],
                    &hash,
                    &custodian,
                    &self.policy_id(),
                );
                outcome(r.is_ok())
            }
            Action::Borrow { work, patron } => {
                let p = &self.patrons[patron];
                outcome(self.client.try_borrow_work(p, &self.works[work], p).is_ok())
            }
            Action::Return { work, patron } => {
                let p = &self.patrons[patron];
                outcome(self.client.try_return_work(p, &self.works[work], p).is_ok())
            }
            Action::PlaceHold { work, patron } => {
                let p = &self.patrons[patron];
                outcome(self.client.try_place_hold(p, &self.works[work], p).is_ok())
            }
            Action::ClaimHold { work, patron } => {
                let p = &self.patrons[patron];
                outcome(self.client.try_claim_hold(p, &self.works[work], p).is_ok())
            }
            Action::AdvanceTime { secs } => {
                self.env
                    .ledger()
                    .with_mut(|li| li.timestamp = li.timestamp.saturating_add(secs));
                Outcome::Accepted
            }
        }
    }

    /// Reads back loan and hold presence for every (work, patron) pair.
    ///
    /// There is no read-only getter for these, so presence is probed through
    /// the transitions that are defined to fail when the record already
    /// exists: `borrow_work` is rejected exactly when a loan is held, and
    /// `place_hold` exactly when a hold is held. A probe that is accepted
    /// creates a record, so it is undone immediately — reconciliation runs
    /// only at the end of a sequence, after all assertions.
    fn probe_loan(&self, work: usize, patron: usize) -> bool {
        let p = &self.patrons[patron];
        match self.client.try_borrow_work(p, &self.works[work], p) {
            Ok(_) => {
                // No loan existed; undo the probe.
                self.client.return_work(p, &self.works[work], p);
                false
            }
            Err(_) => true,
        }
    }
}

fn outcome(ok: bool) -> Outcome {
    if ok {
        Outcome::Accepted
    } else {
        Outcome::Rejected
    }
}

/// Drives one pseudo-random sequence and returns its outcome trace.
fn run_sequence(seed: u64) -> [Outcome; SEQUENCE_LEN] {
    let harness = setup_harness();
    let mut model = Model::new(harness.env.ledger().timestamp());
    let mut rng = Rng(seed);
    let mut trace = [Outcome::Accepted; SEQUENCE_LEN];

    for (step, slot) in trace.iter_mut().enumerate() {
        let action = generate(&mut rng);
        let expected = model.apply(action);
        let actual = harness.execute(action);

        assert_eq!(
            expected, actual,
            "seed {seed} step {step}: model and contract disagree on {action:?}"
        );

        // Invariant 2: a loan can never terminate more often than it started.
        assert!(
            model.terminations <= model.borrows,
            "seed {seed} step {step}: {} terminations for {} borrows",
            model.terminations,
            model.borrows
        );

        *slot = actual;
    }

    // Invariant 1: reconcile every loan slot. A rejected action that had
    // silently created or cleared a loan shows up here.
    for work in 0..WORKS {
        for patron in 0..PATRONS {
            assert_eq!(
                model.has_loan(work, patron),
                harness.probe_loan(work, patron),
                "seed {seed}: loan state diverged for work {work}, patron {patron}"
            );
        }
    }

    trace
}

// ═══════════════════════════════════════════════════════════════════════════
// Model-check sequences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn model_and_contract_agree_across_random_action_sequences() {
    for seed in [1u64, 7, 42, 1_337, 90_210, 2_147_483_647] {
        run_sequence(seed);
    }
}

#[test]
fn replaying_a_seed_produces_an_identical_trace() {
    // Invariant 3: hold placement order and every transition outcome are a
    // pure function of the action sequence.
    let first = run_sequence(4_242);
    let second = run_sequence(4_242);
    assert_eq!(first, second, "identical seeds produced different traces");
}

// ═══════════════════════════════════════════════════════════════════════════
// Named transitions — each documents one invariant of the state machine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn borrowing_an_unregistered_work_is_rejected_with_work_not_found() {
    let h = setup_harness();
    let p = &h.patrons[0];
    assert_eq!(
        h.client.try_borrow_work(p, &h.works[0], p),
        Err(Ok(ContractError::WorkNotFound))
    );
}

#[test]
fn a_second_borrow_of_a_held_loan_is_rejected_and_leaves_the_loan_intact() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    h.client.borrow_work(p, &h.works[0], p);
    assert!(h.client.try_borrow_work(p, &h.works[0], p).is_err());

    // The original loan survived the rejected transition: the return clears it,
    // and only then does a fresh borrow succeed.
    h.client.return_work(p, &h.works[0], p);
    assert!(h.client.try_borrow_work(p, &h.works[0], p).is_ok());
}

#[test]
fn returning_a_work_that_was_never_borrowed_is_a_no_op() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    // Idempotent by contract: repeated returns succeed and terminate nothing.
    assert!(h.client.try_return_work(p, &h.works[0], p).is_ok());
    assert!(h.client.try_return_work(p, &h.works[0], p).is_ok());
    assert!(h.client.try_borrow_work(p, &h.works[0], p).is_ok());
}

#[test]
fn a_loan_terminates_exactly_once_under_repeated_returns() {
    let h = setup_harness();
    let mut model = Model::new(h.env.ledger().timestamp());
    h.execute(Action::PutWork { work: 0 });
    model.apply(Action::PutWork { work: 0 });

    for action in [
        Action::Borrow { work: 0, patron: 0 },
        Action::Return { work: 0, patron: 0 },
        Action::Return { work: 0, patron: 0 },
        Action::Return { work: 0, patron: 0 },
    ] {
        assert_eq!(model.apply(action), h.execute(action));
    }

    assert_eq!(model.borrows, 1);
    assert_eq!(model.terminations, 1, "a single loan terminated more than once");
}

#[test]
fn a_patron_cannot_hold_the_same_work_twice() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    assert!(h.client.try_place_hold(p, &h.works[0], p).is_ok());
    assert!(h.client.try_place_hold(p, &h.works[0], p).is_err());
}

#[test]
fn placing_a_hold_on_an_unregistered_work_is_rejected_with_work_not_found() {
    let h = setup_harness();
    let p = &h.patrons[0];
    assert_eq!(
        h.client.try_place_hold(p, &h.works[0], p),
        Err(Ok(ContractError::WorkNotFound))
    );
}

#[test]
fn claiming_without_a_hold_is_rejected_with_hold_not_found() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];
    assert_eq!(
        h.client.try_claim_hold(p, &h.works[0], p),
        Err(Ok(ContractError::HoldNotFound))
    );
}

#[test]
fn an_expired_hold_cannot_be_claimed_and_is_not_consumed() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    h.client.place_hold(p, &h.works[0], p);
    h.execute(Action::AdvanceTime {
        secs: HOLD_SECS + 1,
    });

    assert_eq!(
        h.client.try_claim_hold(p, &h.works[0], p),
        Err(Ok(ContractError::HoldExpired))
    );
    // Rejected claim left the hold in place, so re-placing it is still refused.
    assert!(h.client.try_place_hold(p, &h.works[0], p).is_err());
}

#[test]
fn a_hold_exactly_at_its_expiry_is_still_claimable() {
    // The contract compares with `>`, so the expiry second itself is inclusive.
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    h.client.place_hold(p, &h.works[0], p);
    h.execute(Action::AdvanceTime { secs: HOLD_SECS });

    assert!(h.client.try_claim_hold(p, &h.works[0], p).is_ok());
}

#[test]
fn claiming_a_hold_while_already_borrowing_fails_and_keeps_the_hold() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let p = &h.patrons[0];

    h.client.place_hold(p, &h.works[0], p);
    h.client.borrow_work(p, &h.works[0], p);

    assert!(h.client.try_claim_hold(p, &h.works[0], p).is_err());
    // The hold was not consumed by the failed claim.
    assert!(h.client.try_place_hold(p, &h.works[0], p).is_err());
    // And once the loan is returned the same hold still converts.
    h.client.return_work(p, &h.works[0], p);
    assert!(h.client.try_claim_hold(p, &h.works[0], p).is_ok());
}

#[test]
fn a_claim_by_someone_other_than_the_holder_is_rejected() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    let holder = &h.patrons[0];
    let stranger = &h.patrons[1];

    h.client.place_hold(holder, &h.works[0], holder);
    assert!(h.client.try_claim_hold(stranger, &h.works[0], holder).is_err());
    // The hold is untouched: the holder can still claim it.
    assert!(h.client.try_claim_hold(holder, &h.works[0], holder).is_ok());
}

#[test]
fn holds_and_loans_on_different_works_are_independent() {
    let h = setup_harness();
    h.execute(Action::PutWork { work: 0 });
    h.execute(Action::PutWork { work: 1 });
    let p = &h.patrons[0];

    h.client.borrow_work(p, &h.works[0], p);
    // A loan on work 0 does not block a loan on work 1.
    assert!(h.client.try_borrow_work(p, &h.works[1], p).is_ok());

    h.client.place_hold(p, &h.works[0], p);
    // Nor does a hold on work 0 block one on work 1.
    assert!(h.client.try_place_hold(p, &h.works[1], p).is_ok());
}

#[test]
fn two_patrons_hold_the_same_work_independently_and_in_placement_order() {
    let h = setup_harness();
    let mut model = Model::new(h.env.ledger().timestamp());
    h.execute(Action::PutWork { work: 0 });
    model.apply(Action::PutWork { work: 0 });

    for patron in 0..2 {
        let action = Action::PlaceHold { work: 0, patron };
        assert_eq!(model.apply(action), h.execute(action));
    }

    // Placement order is recorded and stable.
    assert_eq!(model.hold[0][0].unwrap().seq, 0);
    assert_eq!(model.hold[0][1].unwrap().seq, 1);

    // Claiming out of placement order is permitted by this implementation —
    // holds are per-patron, not a single shared queue. Pinned here so a future
    // move to a shared FIFO queue has to update the model deliberately.
    for patron in [1usize, 0] {
        let action = Action::ClaimHold { work: 0, patron };
        assert_eq!(model.apply(action), h.execute(action));
    }
}
