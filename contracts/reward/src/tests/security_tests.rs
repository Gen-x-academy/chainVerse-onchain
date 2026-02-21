#![cfg(test)]

use soroban_sdk::{Env, Address};
use crate::{RewardContract, RewardContractClient};

#[test]
#[should_panic(expected = "AlreadyRewarded")]
fn test_duplicate_reward_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RewardContract);
    let client = RewardContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let course_id = 1;

    env.mock_all_auths();

    client.claim_reward(&user, &course_id);

    // Second attempt
    client.claim_reward(&user, &course_id);
}