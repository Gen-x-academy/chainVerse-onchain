#[cfg(test)]
mod test {
    use soroban_sdk::{testutils::Address as _, Address, Env};
    use crate::{RewardContract, RewardContractClient};

    fn setup() -> (Env, RewardContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let recipient = Address::generate(&env);

        // Register a mock token contract
        let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();

        let cid = env.register(RewardContract, ());
        let client = RewardContractClient::new(&env, &cid);
        client.initialize(&admin, &treasury, &token_id, &1_000_000i128);

        (env, client, admin, treasury, recipient)
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
        let (_env, client, admin, _treasury, recipient) = setup();
        client.record_penalty(&1_000i128);
        assert_eq!(client.get_penalty_pool(), 1_000i128);

        client.withdraw_penalties(&admin, &recipient).unwrap();
        assert_eq!(client.get_penalty_pool(), 0i128);
    }

    #[test]
    #[should_panic(expected = "no penalties to withdraw")]
    fn test_withdraw_empty_pool_panics() {
        let (_env, client, admin, _treasury, recipient) = setup();
        client.withdraw_penalties(&admin, &recipient).unwrap();
    }

    #[test]
    fn test_withdraw_penalties_unauthorized_fails() {
        let (env, client, _admin, _treasury, recipient) = setup();
        let rando = Address::generate(&env);
        client.record_penalty(&500i128);
        let result = client.try_withdraw_penalties(&rando, &recipient);
        assert!(result.is_err());
    }
}
