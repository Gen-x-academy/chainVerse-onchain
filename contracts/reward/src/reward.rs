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
    token_client.transfer(&treasury, &user, &reward_amount);

    set_rewarded(&env, &user);
    emit_reward_claimed(&env, &user, reward_amount);

    Ok(())
}
