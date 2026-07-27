use soroban_sdk::{symbol_short, token::Client, Address, Env, Vec};
use crate::storage::*;
use crate::events::*;
use crate::errors::Error;
use crate::eligibility::assert_eligible;

/// Maximum number of recipients per batch, to stay within the Soroban compute
/// budget for a single transaction.
const MAX_BATCH: u32 = 50;

/// Admin-only: update the per-student reward amount for all subsequent claims.
/// The reward amount is stored (not hardcoded), so the admin can adjust it as
/// the token value changes without redeploying the contract.
pub fn update_reward_amount(env: Env, new_amount: i128) -> Result<(), Error> {
    crate::admin::require_admin(&env)?;
    if new_amount <= 0 {
        panic!("reward amount must be positive");
    }
    crate::storage::set_reward_amount(&env, new_amount);
    env.events()
        .publish((symbol_short!("reward_upd"),), new_amount);
    Ok(())
}

/// Returns the current per-student reward amount.
pub fn current_reward_amount(env: Env) -> Result<i128, Error> {
    crate::storage::get_reward_amount(&env)
}

/// Admin-only: distribute the reward to many students in one transaction.
/// Already-rewarded students in the batch are skipped silently (no error, no
/// double reward). Batches larger than `MAX_BATCH` are rejected.
pub fn batch_claim_reward(env: Env, recipients: Vec<Address>) -> Result<(), Error> {
    crate::admin::require_admin(&env)?;
    if recipients.len() > MAX_BATCH {
        panic!("batch too large");
    }

    let treasury = get_treasury(&env)?;
    let token_address = get_token(&env)?;
    let amount_per = get_reward_amount(&env)?;
    let token_client = Client::new(&env, &token_address);

    for student in recipients.iter() {
        if !has_been_rewarded(&env, &student) {
            set_rewarded(&env, &student);
            token_client.transfer(&treasury, &student, &amount_per);
            emit_reward_claimed(&env, &student, amount_per);
        }
    }
    Ok(())
}

pub fn claim_reward(env: Env, user: Address) -> Result<(), Error> {
    user.require_auth();

    // Eligibility is fully validated here before any state mutation or transfer.
    assert_eligible(&env, &user)?;

    let treasury = get_treasury(&env)?;
    let token_address = get_token(&env)?;
    let reward_amount = get_reward_amount(&env)?;

    let token_client = Client::new(&env, &token_address);
    let allowance = token_client.allowance(&treasury, &env.current_contract_address());
    if allowance < reward_amount {
        return Err(Error::InsufficientTreasuryAllowance);
    }
    token_client.transfer(&treasury, &user, &reward_amount);

    // Optimistic locking: set the flag BEFORE the transfer so that a
    // panicking transfer cannot leave the flag unset and allow re-claims.
    set_rewarded(&env, &user);
    token_client.transfer(&treasury, &user, &reward_amount);
    emit_reward_claimed(&env, &user, reward_amount);

    Ok(())
}
