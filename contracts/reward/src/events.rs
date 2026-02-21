use soroban_sdk::{Env, Symbol, Address};

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