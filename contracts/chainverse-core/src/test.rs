#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{Env, Address};

    #[test]
    fn test_prevent_duplicate_purchase() {
        let env = Env::default();
        let contract_id = env.register_contract(None, CourseContract);
        let client = CourseContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);
        let course_id = 1u64;

        client.purchase_course(&user, &course_id);

        let result = std::panic::catch_unwind(|| {
            client.purchase_course(&user, &course_id);
        });

        assert!(result.is_err());
    }
}