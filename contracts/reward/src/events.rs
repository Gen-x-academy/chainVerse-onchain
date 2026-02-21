use soroban_sdk::{Env, Symbol, Address,symbol_short};

pub fn reward_claimed(
    env: &Env,
    user: Address,
    course_id: u32,
    amount: i128,
) {
    env.events().publish(
        (Symbol::new(env, "RewardClaimed"), user),
        (course_id, amount, env.ledger().timestamp()),
    );
}

pub fn emit_reward_claimed(env: &Env, user: &Address, amount: i128) {
    let topics = (symbol_short!("RewardClaimed"), user.clone());
    env.events().publish(topics, amount);
}