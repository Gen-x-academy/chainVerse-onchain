//! Positive, negative, authorization, and boundary tests for the
//! governed-change timelock (#995).

use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

const DAY: u64 = 24 * 60 * 60;
const MIN_DELAY: u64 = 7 * DAY;
const MAX_DELAY: u64 = 30 * DAY;

fn setup() -> (
    Env,
    LibraryRightsContractClient,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LibraryRightsContract);
    let client = LibraryRightsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let librarian = Address::generate(&env);

    client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);

    (
        env,
        client,
        admin,
        treasury,
        policy_manager,
        emergency,
        librarian,
    )
}

fn eta(env: &Env, from_now_days: u64) -> u64 {
    env.ledger().timestamp() + from_now_days * DAY
}

fn transfer_action(to: Address) -> crate::timelock::GovernedAction {
    crate::timelock::GovernedAction::TransferRole {
        role: crate::keys::Role::Emergency,
        to,
    }
}

#[test]
fn test_default_config_is_bounded_and_queryable() {
    let (env, client, _a, _t, _pm, _e, _l) = setup();
    let cfg = client.timelock_config();
    assert_eq!(cfg.min_delay, MIN_DELAY);
    assert_eq!(cfg.max_delay, MAX_DELAY);
    // No changes queued yet.
    assert_eq!(client.pending_changes().len(), 0);
    let _ = env;
}

#[test]
fn test_queue_rejects_eta_outside_window_and_round_trips() {
    let (env, client, admin, _t, _pm, emergency, _l) = setup();

    // Below the 7-day floor.
    assert_eq!(
        client.try_queue_change(&admin, &transfer_action(emergency.clone()), &eta(&env, 6)),
        Err(Ok(ContractError::TimelockEtaInvalid))
    );
    // Above the 30-day ceiling.
    assert_eq!(
        client.try_queue_change(&admin, &transfer_action(emergency.clone()), &eta(&env, 31)),
        Err(Ok(ContractError::TimelockEtaInvalid))
    );

    // A valid 8-day eta is accepted and exposes the activation time.
    let id = client.queue_change(&admin, &transfer_action(emergency.clone()), &eta(&env, 8));
    let queued = client.pending_change(&id).expect("queued change present");
    assert_eq!(queued.eta, eta(&env, 8));
    assert!(!queued.executed);
    assert!(!queued.cancelled);
    assert_eq!(client.pending_changes().len(), 1);
}

#[test]
fn test_duplicate_pending_proposal_rejected() {
    let (env, client, admin, _t, _pm, emergency, _l) = setup();
    let target = Address::generate(&env);

    client.queue_change(&admin, &transfer_action(target.clone()), &eta(&env, 8));
    // Reproposing the same action+proposer while pending is rejected.
    assert_eq!(
        client.try_queue_change(&admin, &transfer_action(target), &eta(&env, 9)),
        Err(Ok(ContractError::TimelockAlreadyQueued))
    );
}

#[test]
fn test_execute_before_eta_rejected_then_succeeds_after() {
    let (env, client, admin, _t, _pm, emergency, _l) = setup();
    let new_emergency = Address::generate(&env);

    let id = client.queue_change(&admin, &transfer_action(new_emergency.clone()), &eta(&env, 8));

    // Advance 7 days: still before eta.
    env.ledger().with_mut(|li| li.timestamp += 7 * DAY);
    assert_eq!(
        client.try_execute_change(&Address::generate(&env), &id),
        Err(Ok(ContractError::TimelockNotReady))
    );

    // Advance past eta: any authorized caller can execute.
    env.ledger().with_mut(|li| li.timestamp += 2 * DAY);
    let relayer = Address::generate(&env);
    client.execute_change(&relayer, &id);

    // The role really moved, and the tombstone blocks replay.
    let holder = client.get_role(&crate::keys::Role::Emergency).expect("role present");
    assert_eq!(holder, new_emergency);
    assert_eq!(
        client.try_execute_change(&relayer, &id),
        Err(Ok(ContractError::TimelockAlreadyExecuted))
    );
}

#[test]
fn test_reproposal_cannot_bypass_the_delay() {
    let (env, client, admin, _t, _pm, _emergency, _l) = setup();
    let target = Address::generate(&env);

    let first = client.queue_change(&admin, &transfer_action(target.clone()), &eta(&env, 8));
    env.ledger().with_mut(|li| li.timestamp += 10 * DAY);
    client.execute_change(&admin, &first);

    // A fresh proposal for the same target gets a fresh id and must serve
    // its own full delay again -- no shortcut via reproposal.
    let second = client.queue_change(&admin, &transfer_action(target.clone()), &eta(&env, 8));
    assert_ne!(first, second);
    env.ledger().with_mut(|li| li.timestamp += 7 * DAY);
    assert_eq!(
        client.try_execute_change(&admin, &second),
        Err(Ok(ContractError::TimelockNotReady))
    );
    env.ledger().with_mut(|li| li.timestamp += 2 * DAY);
    client.execute_change(&admin, &second);
}

#[test]
fn test_cancel_requires_authorization_and_blocks_execution() {
    let (env, client, admin, _t, pm, emergency, _l) = setup();
    let target = Address::generate(&env);

    let id = client.queue_change(&admin, &transfer_action(target.clone()), &eta(&env, 8));
    let relayer = Address::generate(&env);

    // A non-proposer, non-admin caller cannot cancel.
    assert_eq!(
        client.try_cancel_change(&relayer, &id),
        Err(Ok(ContractError::NotAdmin))
    );

    // The proposer (admin) cancels; the change can never execute.
    client.cancel_change(&admin, &id);
    env.ledger().with_mut(|li| li.timestamp += 9 * DAY);
    assert_eq!(
        client.try_execute_change(&relayer, &id),
        Err(Ok(ContractError::TimelockCancelled))
    );

    let _ = pm;
    let _ = emergency;
}

#[test]
fn test_update_timelock_config_via_queued_action() {
    let (env, client, admin, _t, _pm, _e, _l) = setup();

    // Reconfigure the window to 2..=14 days through the timelock itself.
    let action = crate::timelock::GovernedAction::UpdateTimelockConfig {
        min_delay: 2 * DAY,
        max_delay: 14 * DAY,
    };
    let id = client.queue_change(&admin, &action, &eta(&env, 8));
    env.ledger().with_mut(|li| li.timestamp += 9 * DAY);
    client.execute_change(&admin, &id);

    let cfg = client.timelock_config();
    assert_eq!(cfg.min_delay, 2 * DAY);
    assert_eq!(cfg.max_delay, 14 * DAY);

    // An invalid window (min > max) fails deterministically at execution.
    let bad = crate::timelock::GovernedAction::UpdateTimelockConfig {
        min_delay: 14 * DAY,
        max_delay: 2 * DAY,
    };
    let bad_id = client.queue_change(&admin, &bad, &eta(&env, 8));
    env.ledger().with_mut(|li| li.timestamp += 9 * DAY);
    assert_eq!(
        client.try_execute_change(&admin, &bad_id),
        Err(Ok(ContractError::TimelockConfigInvalid))
    );
}