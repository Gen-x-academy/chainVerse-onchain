use crate::LibraryRightsContract;
use course_registry::CourseRegistryContract;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Symbol};

mod governance;
mod privacy;
mod storage;

/// Shared test setup: a fresh env with a freshly registered contract.
fn setup() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryRightsContract, ());
    (env, contract_id)
}

#[test]
fn test_version_reports_current_abi() {
    let (env, contract_id) = setup();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

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
}
