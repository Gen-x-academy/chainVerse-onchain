use soroban_sdk::{Env, Address, token::Client};
use crate::storage::*;
use crate::events::*;
use shared::errors::ContractError;

pub fn claim_reward(env: Env, user: Address) -> Result<(), ContractError> {
    user.require_auth();

    // 1️⃣ Check not already rewarded
    if has_been_rewarded(&env, &user) {
        return Err(ContractError::AlreadyRewarded);
    }

    // 2️⃣ Get reward details
    let treasury = get_treasury(&env);
    let token_address = get_token(&env);
    let reward_amount = get_reward_amount(&env);

    // 3️⃣ Transfer from treasury to user
    let token_client = Client::new(&env, &token_address);

    token_client.transfer(
        &treasury,
        &user,
        &reward_amount,
    );

    // 4️⃣ Mark as rewarded
    set_rewarded(&env, &user);

    // 5️⃣ Emit event
    emit_reward_claimed(&env, &user, reward_amount);

    Ok(())
}