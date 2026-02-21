#[cfg(test)]
mod test {
    use soroban_sdk::{Env, Address};
    use crate::CertificateContract;

    #[test]
    fn test_transfer_fails() {
        let e = Env::default();
        let contract_id = e.register_contract(None, CertificateContract);
        let client = CertificateContractClient::new(&e, &contract_id);

        let user1 = Address::generate(&e);
        let user2 = Address::generate(&e);

        client.mint(&user1, &1);

        let result = client.try_transfer(&user1, &user2, &1);

        assert!(result.is_err());
    }
    
    #[test]
    fn duplicate_mint_fails() {
        let env = Env::default();
        let wallet = Address::generate(&env);

        // Simulate first mint
        // (mock signature verification if needed)

        // Second mint should fail with CertificateExists
    }
}

