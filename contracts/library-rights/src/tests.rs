#[cfg(test)]
mod return_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, IntoVal, symbol_short};

    #[test]
    fn test_return_work() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LibraryRightsContract);
        let client = LibraryRightsContractClient::new(&env, &contract_id);

        let admin = Address::random(&env);
        let treasury = Address::random(&env);
        let policy_manager = Address::random(&env);
        let emergency = Address::random(&env);
        let librarian = Address::random(&env);
        let borrower = Address::random(&env);

        client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);

        let work_id: BytesN<32> = BytesN::from_array(&env, &[1; 32]);
        let work_hash: BytesN<32> = BytesN::from_array(&env, &[2; 32]);
        let custodian = Address::random(&env);

        client.put_work(&policy_manager, &work_id, &work_hash, &custodian);
        client.borrow_work(&borrower, &work_id, &borrower);

        // A borrower can return a work
        client.return_work(&borrower, &work_id, &borrower);
        let loan_key = DataKey::Loan(work_id.clone(), borrower.clone());
        assert!(!env.storage().persistent().has(&loan_key));

        // A librarian can return a work on behalf of a borrower
        client.borrow_work(&borrower, &work_id, &borrower);
        client.return_work(&librarian, &work_id, &borrower);
        assert!(!env.storage().persistent().has(&loan_key));

        // A user cannot return a work that has not been borrowed
        let res = client.try_return_work(&borrower, &work_id, &borrower);
        assert_eq!(res, Err(Ok(ContractError::LoanNotFound)));
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, IntoVal, symbol_short};

    #[test]
    fn test_hold_lifecycle() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LibraryRightsContract);
        let client = LibraryRightsContractClient::new(&env, &contract_id);

        let admin = Address::random(&env);
        let treasury = Address::random(&env);
        let policy_manager = Address::random(&env);
        let emergency = Address::random(&env);
        let librarian = Address::random(&env);
        let holder = Address::random(&env);
        let other_user = Address::random(&env);

        client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);

        let work_id: BytesN<32> = BytesN::from_array(&env, &[1; 32]);
        let work_hash: BytesN<32> = BytesN::from_array(&env, &[2; 32]);
        let custodian = Address::random(&env);

        client.put_work(&policy_manager, &work_id, &work_hash, &custodian);

        // A user can place a hold on a work
        client.place_hold(&holder, &work_id, &holder);
        let hold_key = DataKey::Hold(work_id.clone(), holder.clone());
        assert!(env.storage().persistent().has(&hold_key));

        // A user can claim a hold and borrow the work
        client.claim_hold(&holder, &work_id, &holder);
        let loan_key = DataKey::Loan(work_id.clone(), holder.clone());
        assert!(env.storage().persistent().has(&loan_key));
        assert!(!env.storage().persistent().has(&hold_key));

        // A user cannot claim a hold that has expired
        client.place_hold(&holder, &work_id, &holder);
        env.ledger().with_mut(|li| li.timestamp = li.timestamp + (8 * 24 * 60 * 60));
        let res = client.try_claim_hold(&holder, &work_id, &holder);
        assert_eq!(res, Err(Ok(ContractError::HoldExpired)));

        // A user cannot claim a hold for another user
        client.place_hold(&holder, &work_id, &holder);
        let res = client.try_claim_hold(&other_user, &work_id, &holder);
        assert_eq!(res, Err(Ok(ContractError::Unauthorized)));

        // A user cannot place a hold on a work that does not exist
        let non_existent_work_id: BytesN<32> = BytesN::from_array(&env, &[3; 32]);
        let res = client.try_place_hold(&holder, &non_existent_work_id, &holder);
        assert_eq!(res, Err(Ok(ContractError::WorkNotFound)));
    }
}

#[cfg(test)]
mod reserve_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, IntoVal, symbol_short};

    #[test]
    fn test_reserve_lifecycle() {
        let env = Env::default();
        let contract_id = env.register_contract(None, LibraryRightsContract);
        let client = LibraryRightsContractClient::new(&env, &contract_id);

        let course_registry_id = env.register_contract(None, course_registry::CourseRegistryContract);
        let course_registry_client = course_registry::CourseRegistryContractClient::new(&env, &course_registry_id);

        let admin = Address::random(&env);
        let treasury = Address::random(&env);
        let policy_manager = Address::random(&env);
        let emergency = Address::random(&env);
        let librarian = Address::random(&env);
        let student = Address::random(&env);
        let other_user = Address::random(&env);

        client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);
        course_registry_client.initialize(&admin);
        client.set_course_registry(&admin, &course_registry_id);

        let work_id: BytesN<32> = BytesN::from_array(&env, &[1; 32]);
        let work_hash: BytesN<32> = BytesN::from_array(&env, &[2; 32]);
        let custodian = Address::random(&env);

        client.put_work(&policy_manager, &work_id, &work_hash, &custodian);

        let course_id: BytesN<32> = BytesN::from_array(&env, &[3; 32]);
        course_registry_client.upsert_course(&admin, &course_id, &100, &100, &true);

        // A policy manager can create a reserve
        client.create_reserve(&policy_manager, &work_id, &course_id, &1, &0);
        let reserve_key = DataKey::Reserve(work_id.clone(), course_id.clone());
        assert!(env.storage().persistent().has(&reserve_key));

        // A student enrolled in the course can borrow from the reserve
        course_registry_client.enroll(&student, &course_id, &student);
        client.borrow_from_reserve(&student, &work_id, &course_id, &student);
        let loan_key = DataKey::Loan(work_id.clone(), student.clone());
        assert!(env.storage().persistent().has(&loan_key));

        // A student cannot borrow from a reserve with no available seats
        let res = client.try_borrow_from_reserve(&other_user, &work_id, &course_id, &other_user);
        assert_eq!(res, Err(Ok(ContractError::NoSeatsAvailable)));

        // A student can return a work to the reserve
        client.return_to_reserve(&student, &work_id, &course_id, &student);
        assert!(!env.storage().persistent().has(&loan_key));

        // A student not enrolled in the course cannot borrow from the reserve
        let res = client.try_borrow_from_reserve(&other_user, &work_id, &course_id, &other_user);
        assert_eq!(res, Err(Ok(ContractError::NotEnrolled)));
    }
}