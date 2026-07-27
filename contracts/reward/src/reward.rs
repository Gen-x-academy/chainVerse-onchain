use soroban_sdk::{Env, Address, token::Client};
use crate::storage::*;
use crate::events::*;
use crate::errors::Error;
use crate::eligibility::assert_eligible;

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
