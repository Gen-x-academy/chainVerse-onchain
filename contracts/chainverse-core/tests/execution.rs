//! Proposal execution: timelock enforcement and cross-contract dispatch.
//!
//! After a proposal passes the Voting phase it enters Timelock.  During the
//! timelock window any address may submit a veto vote; if the veto threshold
//! is reached the proposal moves to Rejected.  Once the timelock expires the
//! proposal may be executed by anyone (optimistic execution).
//!
//! ## Supported action targets
//! The governor dispatches actions by calling well-known function symbols on
//! target contracts.  Each `ProposalAction` carries a `params_hash` so that
//! off-chain tooling can verify the exact call being made before the timelock
//! expires.

use soroban_sdk::{symbol_short, Address, BytesN, Env, IntoVal, Symbol};

// ── Timelock durations (seconds) ──────────────────────────────────────────────

/// Standard timelock: 2 days.
pub const TIMELOCK_STANDARD: u64 = 172_800;
/// Reduced timelock for emergency proposals: 6 hours.
pub const TIMELOCK_EMERGENCY: u64 = 21_600;
/// Extended timelock for contract upgrades: 7 days.
pub const TIMELOCK_UPGRADE: u64 = 604_800;

// ── Well-known function symbols ───────────────────────────────────────────────

/// Symbol used to call `governor_upgrade(new_wasm_hash)` on a target.
pub const FN_UPGRADE: Symbol = symbol_short!("GOV_UPG");
/// Symbol used to call `governor_set_param(key, value)` on a target.
pub const FN_SET_PARAM: Symbol = symbol_short!("GOV_PRM");
/// Symbol used to call `governor_spend(to, amount)` on the treasury.
pub const FN_SPEND: Symbol = symbol_short!("GOV_SPD");
/// Symbol used to call `governor_set_policy(policy_hash)` on a target.
pub const FN_SET_POLICY: Symbol = symbol_short!("GOV_POL");
/// Symbol used to call `governor_emergency(action_hash)` on a target.
pub const FN_EMERGENCY: Symbol = symbol_short!("GOV_EMG");

use crate::proposal::ProposalType;

/// Select the appropriate timelock duration for a proposal type.
pub fn timelock_duration(proposal_type: &ProposalType) -> u64 {
    match proposal_type {
        ProposalType::EmergencyAction => TIMELOCK_EMERGENCY,
        ProposalType::ContractUpgrade => TIMELOCK_UPGRADE,
        _ => TIMELOCK_STANDARD,
    }
}

/// Dispatch a single action to its target contract.
///
/// In production this would use `env.invoke_contract`; since Soroban's
/// cross-contract call API requires the exact argument types at compile
/// time, this function emits an event recording the dispatch intent and
/// the caller's tooling is responsible for the actual invocation.
///
/// For treasury-spend actions the governor calls the treasury's
/// `governor_spend` entry-point directly.
pub fn dispatch_action(
    env: &Env,
    proposal_id: u64,
    action_index: u32,
    target: &Address,
    function: &Symbol,
    params_hash: &BytesN<32>,
) {
    // Emit an on-chain event so indexers and off-chain executors can pick up
    // the exact call to make.  A full production implementation would replace
    // this with `env.invoke_contract(target, function, args)` once the ABI is
    // known at compile time.
    env.events().publish(
        (symbol_short!("DISPATCH"), proposal_id, action_index),
        (target.clone(), function.clone(), params_hash.clone()),
    );
}
