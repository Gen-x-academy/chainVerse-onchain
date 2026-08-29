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
use crate::LibraryRightsContract;
use course_registry::CourseRegistryContract;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Symbol};

mod classifications;
mod content;
mod governance;
mod metadata;
mod integrity_membership;
mod privacy;
mod provenance;
mod registry;
mod storage;

/// Shared test setup: a fresh env with a freshly registered contract.
fn setup() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryRightsContract, ());
    (env, contract_id)
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

    assert_eq!(client.version(), String::from_str(&env, "0.5.0"));
}

fn id(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

#[test]
fn test_policy_versions_are_append_only_and_loans_snapshot_version() {
    let (env, library_id) = setup();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let institution = Address::generate(&env);
    let library = crate::LibraryRightsContractClient::new(&env, &library_id);
    library.bootstrap(&admin, &treasury, &policy_manager, &emergency);
    let scope = crate::PolicyScope {
        institution: institution.clone(),
        role: Symbol::new(&env, "student"),
        format: Symbol::new(&env, "ebook"),
        collection: None,
    };
    assert_eq!(
        library.append_policy(&policy_manager, &id(&env, 1), &scope, &100, &2, &1, &10, &3),
        1
    );
    assert_eq!(
        library.append_policy(&policy_manager, &id(&env, 1), &scope, &200, &2, &1, &10, &3),
        2
    );
    assert_eq!(
        library
            .get_policy_version(&id(&env, 1), &1)
            .policy
            .loan_duration_secs,
        100
    );
    assert_eq!(library.latest_policy(&id(&env, 1)).version, 2);
}

#[test]
fn test_checkout_requires_enrollment_and_allocates_seat_atomically() {
    let (env, library_id) = setup();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let institution = Address::generate(&env);
    let borrower = Address::generate(&env);
    let registry_id = env.register(CourseRegistryContract, ());
    let registry = course_registry::CourseRegistryContractClient::new(&env, &registry_id);
    registry.initialize(&admin);
    let course = Symbol::new(&env, "RUST101");
    registry.set_enrollment(&borrower, &course, &true);
    let library = crate::LibraryRightsContractClient::new(&env, &library_id);
    library.bootstrap(&admin, &treasury, &policy_manager, &emergency);
    let scope = crate::PolicyScope {
        institution: institution.clone(),
        role: Symbol::new(&env, "student"),
        format: Symbol::new(&env, "ebook"),
        collection: None,
    };
    library.append_policy(&policy_manager, &id(&env, 1), &scope, &100, &1, &1, &10, &3);
    library.register_license(
        &policy_manager,
        &id(&env, 2),
        &id(&env, 3),
        &institution,
        &(env.ledger().timestamp() + 1000),
    );
    library.register_rendition(
        &policy_manager,
        &id(&env, 4),
        &id(&env, 3),
        &Symbol::new(&env, "ebook"),
    );
    library.register_seat(&policy_manager, &id(&env, 5), &institution);
    let loan_id = library.checkout(
        &borrower,
        &institution,
        &registry_id,
        &course,
        &Symbol::new(&env, "student"),
        &None,
        &id(&env, 1),
        &id(&env, 2),
        &id(&env, 4),
        &id(&env, 5),
    );
    let loan = library.get_loan(&loan_id);
    assert_eq!(loan.policy_version, 1);
    assert!(loan.active);
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
    assert_eq!(client.version(), String::from_str(&env, "0.6.0"));
    assert_eq!(client.version(), String::from_str(&env, "0.5.0"));
}

#[cfg(test)]
mod race_condition_tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, IntoVal, symbol_short};

    #[test]
    #[should_panic]
    fn test_last_seat_race_condition() {
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
        let student1 = Address::random(&env);
        let student2 = Address::random(&env);

        client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);
        course_registry_client.initialize(&admin);
        client.set_course_registry(&admin, &course_registry_id);

        let work_id: BytesN<32> = BytesN::from_array(&env, &[1; 32]);
        let work_hash: BytesN<32> = BytesN::from_array(&env, &[2; 32]);
        let custodian = Address::random(&env);

        client.put_work(&policy_manager, &work_id, &work_hash, &custodian);

        let course_id: BytesN<32> = BytesN::from_array(&env, &[3; 32]);
        course_registry_client.upsert_course(&admin, &course_id, &100, &100, &true);

        client.create_reserve(&policy_manager, &work_id, &course_id, &1, &0);

        course_registry_client.enroll(&student1, &course_id, &student1);
        course_registry_client.enroll(&student2, &course_id, &student2);

        // Simulate a race condition where two users try to borrow the last seat
        client.borrow_from_reserve(&student1, &work_id, &course_id, &student1);
        client.borrow_from_reserve(&student2, &work_id, &course_id, &student2);
    }
}