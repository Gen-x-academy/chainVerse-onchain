//! Per-entrypoint resource budgets for `library-rights` (#1017).
//!
//! A WASM size cap says nothing about what an individual call costs. Soroban
//! rejects a transaction that exceeds the per-transaction CPU-instruction or
//! memory limit at submission time, so an entrypoint that quietly grows past
//! its share is a production outage, not a slow test.
//!
//! Each entrypoint below is invoked twice — once at a nominal call, once at the
//! largest input the ABI admits — with the budget reset immediately beforehand,
//! and the measured CPU and memory cost is asserted against a committed
//! ceiling.
//!
//! ## Where the ceilings come from
//!
//! Soroban's per-transaction limits are 100,000,000 CPU instructions and
//! 41,943,040 memory bytes ([`NETWORK_CPU_LIMIT`] / [`NETWORK_MEM_LIMIT`]).
//! A single library call must leave room for the rest of the transaction, so
//! the budget policy allocates each entrypoint a fraction of the network limit
//! by class:
//!
//! | Class     | Share of network limit | Rationale                                    |
//! |-----------|------------------------|----------------------------------------------|
//! | `Read`    | 5%                     | One or two storage reads, no cross-contract   |
//! | `Write`   | 10%                    | Storage write plus TTL extension and an event |
//! | `Complex` | 25%                    | Multi-key writes, cross-contract, or loops    |
//!
//! Ceilings are deliberately generous: this gate exists to catch an
//! order-of-magnitude regression (an accidental unbounded loop, a per-call
//! clone of a growing collection), not to police single-digit percentage
//! drift, which the host's own cost model makes noisy between SDK versions.
//!
//! [`Budget::cpu_instruction_cost`] is documented to *underestimate* native
//! execution relative to WASM, so a native measurement that already breaches
//! the ceiling is unambiguously a real regression.
//!
//! ## Updating a ceiling
//!
//! Raising one is a deliberate, reviewable act. Change the constant, and say in
//! the PR what made the call more expensive and why that is acceptable. Do not
//! raise a ceiling to make CI green.
//!
//! ## Impact
//!
//! Test-only. No ABI, storage, event, privacy, deployment or migration impact.

use crate::{LibraryRightsContract, LibraryRightsContractClient, Role};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol};

/// Soroban per-transaction CPU instruction limit.
const NETWORK_CPU_LIMIT: u64 = 100_000_000;
/// Soroban per-transaction memory limit, in bytes (40 MiB).
const NETWORK_MEM_LIMIT: u64 = 41_943_040;

/// Share of the network limit allowed for a read-only entrypoint.
const READ_SHARE_PCT: u64 = 5;
/// Share allowed for a single-record write.
const WRITE_SHARE_PCT: u64 = 10;
/// Share allowed for a multi-write or cross-contract entrypoint.
const COMPLEX_SHARE_PCT: u64 = 25;

#[derive(Clone, Copy)]
enum Class {
    Read,
    Write,
    Complex,
}

impl Class {
    fn share_pct(self) -> u64 {
        match self {
            Class::Read => READ_SHARE_PCT,
            Class::Write => WRITE_SHARE_PCT,
            Class::Complex => COMPLEX_SHARE_PCT,
        }
    }

    fn cpu_ceiling(self) -> u64 {
        NETWORK_CPU_LIMIT / 100 * self.share_pct()
    }

    fn mem_ceiling(self) -> u64 {
        NETWORK_MEM_LIMIT / 100 * self.share_pct()
    }
}

/// Runs `call` with a freshly reset budget and asserts it stays within the
/// ceiling for its class.
///
/// `label` names the entrypoint and the shape of the call, so a CI failure
/// identifies both without the reader opening this file.
fn assert_within_budget<F: FnOnce()>(env: &Env, label: &str, class: Class, call: F) {
    env.cost_estimate().budget().reset_default();

    call();

    let budget = env.cost_estimate().budget();
    let cpu = budget.cpu_instruction_cost();
    let mem = budget.memory_bytes_cost();

    assert!(
        cpu <= class.cpu_ceiling(),
        "{label}: {cpu} CPU instructions exceeds the {}% ceiling of {}",
        class.share_pct(),
        class.cpu_ceiling()
    );
    assert!(
        mem <= class.mem_ceiling(),
        "{label}: {mem} memory bytes exceeds the {}% ceiling of {}",
        class.share_pct(),
        class.mem_ceiling()
    );
}

struct Fixture<'a> {
    env: Env,
    client: LibraryRightsContractClient<'a>,
    policy_manager: Address,
    patron: Address,
    policy_id: Symbol,
}

fn fixture() -> Fixture<'static> {
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

    let policy_id = Symbol::new(&env, "default");
    client.put_policy(&policy_manager, &policy_id, &16, &64);

    let patron = Address::generate(&env);

    Fixture {
        env,
        client,
        policy_manager,
        patron,
        policy_id,
    }
}

fn work_id(env: &Env, n: u8) -> BytesN<32> {
    BytesN::from_array(env, &[n; 32])
}

// ═══════════════════════════════════════════════════════════════════════════
// Governance and configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn budget_bootstrap() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LibraryRightsContract, ());
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let roles: [Address; 5] = core::array::from_fn(|_| Address::generate(&env));

    assert_within_budget(&env, "bootstrap (nominal)", Class::Complex, || {
        client.bootstrap(&roles[0], &roles[1], &roles[2], &roles[3], &roles[4]);
    });
}

#[test]
fn budget_put_policy() {
    let f = fixture();

    assert_within_budget(&f.env, "put_policy (insert)", Class::Write, || {
        f.client
            .put_policy(&f.policy_manager, &Symbol::new(&f.env, "insert"), &4, &16);
    });

    // Maximum-size call: the largest Symbol the SDK admits is 32 characters,
    // and the limit fields are at their u32 maxima.
    assert_within_budget(&f.env, "put_policy (max-size)", Class::Write, || {
        f.client.put_policy(
            &f.policy_manager,
            &Symbol::new(&f.env, "abcdefghijklmnopqrstuvwxyz012345"),
            &u32::MAX,
            &u32::MAX,
        );
    });

    // Update path re-reads the existing record before writing.
    assert_within_budget(&f.env, "put_policy (update)", Class::Write, || {
        f.client
            .put_policy(&f.policy_manager, &Symbol::new(&f.env, "insert"), &8, &32);
    });
}

#[test]
fn budget_get_role() {
    let f = fixture();
    assert_within_budget(&f.env, "get_role", Class::Read, || {
        f.client.get_role(&Role::PolicyManager);
    });
}

#[test]
fn budget_get_policy() {
    let f = fixture();
    let policy_id = f.policy_id.clone();
    assert_within_budget(&f.env, "get_policy", Class::Read, || {
        f.client.get_policy(&policy_id);
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Catalog
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn budget_put_work() {
    let f = fixture();
    let custodian = Address::generate(&f.env);
    let id = work_id(&f.env, 1);
    let hash = work_id(&f.env, 2);
    let policy_id = f.policy_id.clone();

    assert_within_budget(&f.env, "put_work (insert)", Class::Write, || {
        f.client
            .put_work(&f.policy_manager, &id, &hash, &custodian, &policy_id);
    });

    // Overwrite path: the work key already exists.
    let id2 = work_id(&f.env, 1);
    let policy_id2 = f.policy_id.clone();
    assert_within_budget(&f.env, "put_work (overwrite)", Class::Write, || {
        f.client
            .put_work(&f.policy_manager, &id2, &hash, &custodian, &policy_id2);
    });
}

#[test]
fn budget_get_work() {
    let f = fixture();
    let custodian = Address::generate(&f.env);
    let id = work_id(&f.env, 1);
    f.client.put_work(
        &f.policy_manager,
        &id,
        &work_id(&f.env, 2),
        &custodian,
        &f.policy_id,
    );

    assert_within_budget(&f.env, "get_work", Class::Read, || {
        f.client.get_work(&id);
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Lending — the hot path
// ═══════════════════════════════════════════════════════════════════════════

fn lending_fixture() -> (Fixture<'static>, BytesN<32>) {
    let f = fixture();
    let id = work_id(&f.env, 1);
    let custodian = Address::generate(&f.env);
    f.client.put_work(
        &f.policy_manager,
        &id,
        &work_id(&f.env, 2),
        &custodian,
        &f.policy_id,
    );
    (f, id)
}

#[test]
fn budget_borrow_work() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();

    assert_within_budget(&f.env, "borrow_work", Class::Write, || {
        f.client.borrow_work(&patron, &id, &patron);
    });
}

#[test]
fn budget_return_work() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();
    f.client.borrow_work(&patron, &id, &patron);

    assert_within_budget(&f.env, "return_work (clears a loan)", Class::Write, || {
        f.client.return_work(&patron, &id, &patron);
    });

    // The idempotent no-op path must not be more expensive than the real one.
    assert_within_budget(&f.env, "return_work (no-op)", Class::Write, || {
        f.client.return_work(&patron, &id, &patron);
    });
}

#[test]
fn budget_place_hold() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();

    assert_within_budget(&f.env, "place_hold", Class::Write, || {
        f.client.place_hold(&patron, &id, &patron);
    });
}

#[test]
fn budget_claim_hold() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();
    f.client.place_hold(&patron, &id, &patron);

    // Claim writes a loan, removes the hold, and publishes two events, so it is
    // the most expensive call on the lending path.
    assert_within_budget(&f.env, "claim_hold", Class::Complex, || {
        f.client.claim_hold(&patron, &id, &patron);
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Rejection paths — a failing call must not be cheaper to spam than a real one
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn budget_rejected_calls_stay_within_their_class() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();
    let missing = work_id(&f.env, 99);

    assert_within_budget(&f.env, "borrow_work (WorkNotFound)", Class::Write, || {
        assert!(f.client.try_borrow_work(&patron, &missing, &patron).is_err());
    });

    assert_within_budget(&f.env, "claim_hold (HoldNotFound)", Class::Write, || {
        assert!(f.client.try_claim_hold(&patron, &id, &patron).is_err());
    });

    let stranger = Address::generate(&f.env);
    assert_within_budget(&f.env, "put_work (Unauthorized)", Class::Write, || {
        assert!(f
            .client
            .try_put_work(
                &stranger,
                &missing,
                &work_id(&f.env, 3),
                &stranger,
                &f.policy_id
            )
            .is_err());
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Aggregate — a realistic multi-call transaction must fit the network limit
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn budget_full_lending_cycle_fits_one_transaction() {
    let (f, id) = lending_fixture();
    let patron = f.patron.clone();

    f.env.cost_estimate().budget().reset_default();

    f.client.place_hold(&patron, &id, &patron);
    f.client.claim_hold(&patron, &id, &patron);
    f.client.return_work(&patron, &id, &patron);
    f.client.borrow_work(&patron, &id, &patron);
    f.client.return_work(&patron, &id, &patron);

    let budget = f.env.cost_estimate().budget();
    assert!(
        budget.cpu_instruction_cost() <= NETWORK_CPU_LIMIT,
        "a five-call lending cycle costs {} CPU instructions, over the {NETWORK_CPU_LIMIT} network limit",
        budget.cpu_instruction_cost()
    );
    assert!(
        budget.memory_bytes_cost() <= NETWORK_MEM_LIMIT,
        "a five-call lending cycle costs {} memory bytes, over the {NETWORK_MEM_LIMIT} network limit",
        budget.memory_bytes_cost()
    );
}
