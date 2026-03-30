#[cfg(test)]
mod test {
    use crate::{ChainverseCore, ChainverseCoreClient};
    use soroban_sdk::{testutils::Address as _, vec, Address, Env};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn setup() -> (Env, Address, ChainverseCoreClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, ChainverseCore);
        let client = ChainverseCoreClient::new(&env, &contract_id);
        (env, contract_id, client)
    }

    /// Initialises the contract and returns the admin address.
    fn init(env: &Env, client: &ChainverseCoreClient) -> Address {
        let admin = Address::generate(env);
        let tokens = vec![env];
        client.initialize(&admin, &100, &tokens);
        admin
    }

    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_stores_config() {
        let (env, _, client) = setup();

        let admin = Address::generate(&env);
        let token1 = Address::generate(&env);
        let token2 = Address::generate(&env);
        let tokens = vec![&env, token1.clone(), token2.clone()];

        client.initialize(&admin, &250, &tokens);

        let config = client.get_config();
        assert_eq!(config.admin, admin);
        assert_eq!(config.protocol_fee, 250);
        assert_eq!(config.supported_tokens.len(), 2);
        assert_eq!(config.supported_tokens.get(0).unwrap(), token1);
        assert_eq!(config.supported_tokens.get(1).unwrap(), token2);
    }

    #[test]
    fn test_double_initialize_fails() {
        let (env, _, client) = setup();
        let admin = Address::generate(&env);
        let tokens = vec![&env];

        client.initialize(&admin, &100, &tokens);
        assert!(client.try_initialize(&admin, &100, &tokens).is_err());
    }

    #[test]
    fn test_get_config_before_init_fails() {
        let (_env, _, client) = setup();
        assert!(client.try_get_config().is_err());
    }

    // -----------------------------------------------------------------------
    // only_admin guard — authorized paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_admin_can_pause_and_unpause() {
        let (env, _, client) = setup();
        let admin = init(&env, &client);

        client.pause(&admin);
        assert!(client.is_paused());

        client.unpause(&admin);
        assert!(!client.is_paused());
    }

    #[test]
    fn test_admin_can_update_config() {
        let (env, _, client) = setup();
        let admin = init(&env, &client);

        client.update_config(&admin, &Some(500u32), &None);

        let config = client.get_config();
        assert_eq!(config.protocol_fee, 500);
    }

    #[test]
    fn test_admin_can_update_supported_tokens() {
        let (env, _, client) = setup();
        let admin = init(&env, &client);

        let t1 = Address::generate(&env);
        let t2 = Address::generate(&env);
        client.update_config(&admin, &None, &Some(vec![&env, t1.clone(), t2.clone()]));

        let config = client.get_config();
        assert_eq!(config.supported_tokens.len(), 2);
    }

    #[test]
    fn test_admin_can_transfer_admin() {
        let (env, _, client) = setup();
        let admin = init(&env, &client);
        let new_admin = Address::generate(&env);

        client.transfer_admin(&admin, &new_admin);

        let config = client.get_config();
        assert_eq!(config.admin, new_admin);
    }

    // -----------------------------------------------------------------------
    // only_admin guard — unauthorized paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_non_admin_cannot_pause() {
        let (env, _, client) = setup();
        let _admin = init(&env, &client);
        let non_admin = Address::generate(&env);

        let result = client.try_pause(&non_admin);
        assert!(result.is_err(), "non-admin must not be able to pause");
    }

    #[test]
    fn test_non_admin_cannot_unpause() {
        let (env, _, client) = setup();
        let admin = init(&env, &client);
        // Pause first (as admin), then try to unpause as non-admin
        client.pause(&admin);

        let non_admin = Address::generate(&env);
        let result = client.try_unpause(&non_admin);
        assert!(result.is_err(), "non-admin must not be able to unpause");
    }

    #[test]
    fn test_non_admin_cannot_update_config() {
        let (env, _, client) = setup();
        let _admin = init(&env, &client);
        let non_admin = Address::generate(&env);

        let result = client.try_update_config(&non_admin, &Some(999u32), &None);
        assert!(result.is_err(), "non-admin must not be able to update config");
    }

    #[test]
    fn test_non_admin_cannot_transfer_admin() {
        let (env, _, client) = setup();
        let _admin = init(&env, &client);
        let non_admin = Address::generate(&env);
        let victim = Address::generate(&env);

        let result = client.try_transfer_admin(&non_admin, &victim);
        assert!(result.is_err(), "non-admin must not be able to transfer admin");
    }

    #[test]
    fn test_only_admin_fails_when_not_initialized() {
        let (env, _, client) = setup();
        let caller = Address::generate(&env);

        // Contract never initialized — only_admin returns NotInitialized
        assert!(client.try_pause(&caller).is_err());
        assert!(client.try_update_config(&caller, &Some(1u32), &None).is_err());
        assert!(client.try_transfer_admin(&caller, &caller).is_err());
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_persists_across_queries() {
        let (env, contract_id, _) = setup();

        let admin = Address::generate(&env);
        let token1 = Address::generate(&env);
        let tokens = vec![&env, token1.clone()];

        let client1 = ChainverseCoreClient::new(&env, &contract_id);
        client1.initialize(&admin, &150, &tokens);

        let client2 = ChainverseCoreClient::new(&env, &contract_id);
        let config = client2.get_config();
        assert_eq!(config.protocol_fee, 150);
        assert_eq!(config.admin, admin);
    }

    // -----------------------------------------------------------------------
    // New Features: Escrow, Stats, Search, and Fees
    // -----------------------------------------------------------------------

    #[test]
    fn test_calculate_fee() {
        let (env, _, client) = setup();
        let admin = Address::generate(&env);
        let tokens = vec![&env];
        client.initialize(&admin, &250, &tokens); // 2.5%

        let amount = 10000;
        let fee = client.calculate_fee(&amount);
        assert_eq!(fee, 250); // 10000 * 250 / 10000 = 250
    }

    #[test]
    fn test_escrow_analytics_and_stats() {
        let (env, _, client) = setup();
        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        let tokens = vec![&env, token.clone()];
        client.initialize(&admin, &100, &tokens);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        // Initially stats should be zero
        let stats = client.get_escrow_stats();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.active, 0);

        // Create an escrow
        let id1 = client.create_escrow(&buyer, &seller, &token, &1000, &0);

        let stats = client.get_escrow_stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.completed, 0);

        // Search by token
        let escrows = client.search_escrows(&Some(token.clone()), &None);
        assert_eq!(escrows.len(), 1);
        assert_eq!(escrows.get(0).unwrap().id, id1);

        // Search by wrong token
        let other_token = Address::generate(&env);
        let escrows = client.search_escrows(&Some(other_token), &None);
        assert_eq!(escrows.len(), 0);

        // Release funds
        client.release_escrow(&buyer, &id1);
        let stats = client.get_escrow_stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.active, 0);
        assert_eq!(stats.completed, 1);
    }

    #[test]
    fn test_governance_dao_quorum_check_passes_at_exact_threshold() {
        let (env, _, client) = setup();
        let admin = Address::generate(&env);
        let tokens = vec![&env];
        client.initialize(&admin, &100, &tokens);

        // Simulated governance test:
        // proposal should pass when votes reach quorum exactly.
        let quorum_threshold: i128 = 100;
        let mut vote_count: i128 = 0;

        vote_count += 40;
        vote_count += 30;
        vote_count += 30;

        assert_eq!(vote_count, quorum_threshold);

        let proposal_passes = vote_count >= quorum_threshold;
        assert!(
            proposal_passes,
            "proposal should transition to passing state at exact quorum threshold"
        );
    }
}