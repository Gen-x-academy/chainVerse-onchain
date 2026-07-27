#[cfg(test)]
mod test {
    use crate::storage::{self, DataKey};
    use crate::{Error, RewardContract, RewardContractClient};
    use soroban_sdk::{
        testutils::Address as _,
        token::{StellarAssetClient, TokenClient},
        Address, BytesN, Env,
    };

    fn setup() -> (Env, RewardContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let recipient = Address::generate(&env);

        let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();

        let cid = env.register(RewardContract, ());
        let client = RewardContractClient::new(&env, &cid);
        client.initialize(&admin, &treasury, &token_id, &1_000_000i128);

        (env, client, admin, treasury, recipient)
    }

    /// Clears instance storage to simulate post-upgrade instance wipe.
    fn simulate_upgrade_clear_instance(env: &Env, contract: &Address) {
        env.as_contract(contract, || {
            env.storage().instance().remove(&DataKey::Admin);
            env.storage().instance().remove(&DataKey::Initialized);
            env.storage().instance().remove(&DataKey::BackendPubKey);
            env.storage().instance().remove(&DataKey::Paused);
            env.storage().instance().remove(&DataKey::Treasury);
            env.storage().instance().remove(&DataKey::Token);
            env.storage().instance().remove(&DataKey::RewardAmount);
            env.storage().instance().remove(&DataKey::PenaltyPool);
        });
    }

    #[test]
    fn test_penalty_pool_starts_at_zero() {
        let (_env, client, _admin, _treasury, _recipient) = setup();
        assert_eq!(client.get_penalty_pool(), 0i128);
    }

    #[test]
    fn test_record_penalty_accumulates() {
        let (_env, client, _admin, _treasury, _recipient) = setup();
        client.record_penalty(&500i128);
        client.record_penalty(&300i128);
        assert_eq!(client.get_penalty_pool(), 800i128);
    }

    #[test]
    fn test_withdraw_penalties_resets_pool() {
        let (env, client, admin, treasury, recipient) = setup();
        let token = client.get_token().unwrap();
        StellarAssetClient::new(&env, &token).mint(&treasury, &1_000);
        TokenClient::new(&env, &token).approve(
            &treasury,
            &client.address,
            &1_000,
            &1_000_000u32,
        );

        client.record_penalty(&1_000i128);
        assert_eq!(client.get_penalty_pool(), 1_000i128);

        client.withdraw_penalties(&admin, &recipient);
        assert_eq!(client.get_penalty_pool(), 0i128);
    }

    #[test]
    fn test_withdraw_empty_pool_returns_error() {
        let (_env, client, admin, _treasury, recipient) = setup();
        let result = client.try_withdraw_penalties(&admin, &recipient);
        assert_eq!(result, Err(Ok(Error::NoPenaltiesToWithdraw)));
    }

    #[test]
    fn test_withdraw_penalties_unauthorized_fails() {
        let (env, client, _admin, _treasury, recipient) = setup();
        let rando = Address::generate(&env);
        client.record_penalty(&500i128);
        let result = client.try_withdraw_penalties(&rando, &recipient);
        assert!(result.is_err());
    }

    #[test]
    fn rotate_backend_pubkey_before_initialize_returns_not_initialized() {
        let env = Env::default();
        let contract_id = env.register(RewardContract, ());
        let client = RewardContractClient::new(&env, &contract_id);
        let new_pubkey = BytesN::from_array(&env, &[7; 32]);

        let result = client.try_rotate_backend_pubkey(&new_pubkey);

        assert_eq!(result, Err(Ok(Error::NotInitialized)));
    }

    /// initialize → clear instance storage (simulated upgrade) → treasury readable
    /// and claim_reward still succeeds.
    #[test]
    fn test_config_and_claim_survive_simulated_upgrade() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let user = Address::generate(&env);
        let reward_amount = 500_i128;

        let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let cid = env.register(RewardContract, ());
        let client = RewardContractClient::new(&env, &cid);

        client.initialize(&admin, &treasury, &token_id, &reward_amount);

        // Fund treasury and approve the reward contract to pull rewards.
        StellarAssetClient::new(&env, &token_id).mint(&treasury, &reward_amount);
        TokenClient::new(&env, &token_id).approve(
            &treasury,
            &client.address,
            &reward_amount,
            &1_000_000u32,
        );

        // Simulate upgrade: wipe instance storage (config must live in persistent).
        simulate_upgrade_clear_instance(&env, &client.address);

        // Treasury still readable from persistent storage.
        assert_eq!(client.get_treasury().unwrap(), treasury);
        assert_eq!(client.get_token().unwrap(), token_id);
        assert!(env.as_contract(&client.address, || storage::is_initialized(&env)));

        // Claim still works after the instance wipe.
        client.claim_reward(&user);
        assert_eq!(TokenClient::new(&env, &token_id).balance(&user), reward_amount);
    }

    #[test]
    fn test_treasury_readable_after_simulated_upgrade() {
        let (env, client, _admin, treasury, _recipient) = setup();
        simulate_upgrade_clear_instance(&env, &client.address);
        assert_eq!(client.get_treasury().unwrap(), treasury);
    }
}
