#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{Env, testutils::Address as _};

    #[test]
    fn test_initialize_and_supply() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CHVToken);
        let client = CHVTokenClient::new(&env, &contract_id);

        let admin = soroban_sdk::Address::generate(&env);

        client.initialize(&admin);

        assert_eq!(client.total_supply(), 100_000_000 * 10_i128.pow(7));
        assert_eq!(client.balance(&admin), 100_000_000 * 10_i128.pow(7));
    }
}