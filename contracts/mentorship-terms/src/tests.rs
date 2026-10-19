#![cfg(test)]

use crate::{ContractError, MentorshipTermsContract, TermsHashes};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _}, symbol_short, Address, BytesN, Env, Symbol,
};

fn setup() -> (Env, Address, Address, Address, BytesN<32>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipTermsContract, ());
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let program = hash(&env, 1);
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.register_program(&admin, &program, &owner);
    (env, contract_id, admin, owner, program)
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn hashes(env: &Env) -> TermsHashes {
    TermsHashes {
        eligibility: hash(env, 1),
        fees: hash(env, 2),
        cancellation: hash(env, 3),
        conduct: hash(env, 4),
        privacy: hash(env, 5),
        evidence: hash(env, 6),
        completion: hash(env, 7),
    }
}

fn publish(
    env: &Env,
    client: &crate::MentorshipTermsContractClient,
    owner: &Address,
    program: &BytesN<32>,
    version_hashes: &TermsHashes,
) -> u32 {
    client.publish_terms(owner, program, version_hashes, &(env.ledger().timestamp() + 500))
}

#[test]
fn program_registration_emits_owner_event() {
    let (env, _contract_id, _admin, owner, program) = setup();
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("PROGREG"));
    let (event_program, event_owner): (BytesN<32>, Address) = data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(event_owner, owner);
}

#[test]
fn publishes_immutable_incrementing_versions_and_event_schema() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    let first_hashes = hashes(&env);
    assert_eq!(publish(&env, &client, &owner, &program, &first_hashes), 1);
    let second_hashes = TermsHashes {
        eligibility: hash(&env, 11),
        fees: hash(&env, 12),
        cancellation: hash(&env, 13),
        conduct: hash(&env, 14),
        privacy: hash(&env, 15),
        evidence: hash(&env, 16),
        completion: hash(&env, 17),
    };
    assert_eq!(publish(&env, &client, &owner, &program, &second_hashes), 2);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("TERMPUB"));
    let (event_program, version, event_hashes, expiry):
        (BytesN<32>, u32, TermsHashes, u64) = data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(version, 2);
    assert_eq!(event_hashes, second_hashes);
    assert!(expiry > env.ledger().timestamp());

    let first = client.get_terms(&program, &1);
    assert_eq!(first.version, 1);
    assert_eq!(first.hashes, first_hashes);
    assert_eq!(client.get_latest_version(&program), 2);
}

#[test]
fn only_registered_owner_can_publish() {
    let (env, contract_id, _admin, _owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_publish_terms(&attacker, &program, &hashes(&env), &500),
        Err(Ok(ContractError::NotProgramOwner))
    );
}

#[test]
fn accepts_active_version_and_preserves_snapshot_after_republication() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    let first_hashes = hashes(&env);
    publish(&env, &client, &owner, &program, &first_hashes);
    let mentee = Address::generate(&env);
    let application = hash(&env, 40);
    assert_eq!(client.accept_terms(&mentee, &program, &application), 1);
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("TERMACK"));
    let (event_program, event_application, event_mentee, event_version, accepted_at):
        (BytesN<32>, BytesN<32>, Address, u32, u64) = data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(event_application, application);
    assert_eq!(event_mentee, mentee);
    assert_eq!(event_version, 1);
    assert_eq!(accepted_at, env.ledger().timestamp());

    let second_hashes = TermsHashes {
        eligibility: hash(&env, 11),
        fees: hash(&env, 12),
        cancellation: hash(&env, 13),
        conduct: hash(&env, 14),
        privacy: hash(&env, 15),
        evidence: hash(&env, 16),
        completion: hash(&env, 17),
    };
    publish(&env, &client, &owner, &program, &second_hashes);
    let saved = client.get_acceptance(&program, &application);
    assert_eq!(saved.mentee, mentee);
    assert_eq!(saved.terms.version, 1);
    assert_eq!(saved.terms.hashes, first_hashes);
}

#[test]
fn retries_cannot_duplicate_acceptance_or_event() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    publish(&env, &client, &owner, &program, &hashes(&env));
    let mentee = Address::generate(&env);
    let application = hash(&env, 41);
    client.accept_terms(&mentee, &program, &application);
    let events_before = env.events().all().len();

    assert_eq!(
        client.try_accept_terms(&mentee, &program, &application),
        Err(Ok(ContractError::ApplicationAlreadyAccepted))
    );
    assert_eq!(env.events().all().len(), events_before);
}

#[test]
fn expired_terms_reject_new_acceptance_but_keep_prior_receipt() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    let expiry = env.ledger().timestamp() + 2;
    client.publish_terms(&owner, &program, &hashes(&env), &expiry);
    let mentee = Address::generate(&env);
    let accepted_application = hash(&env, 42);
    client.accept_terms(&mentee, &program, &accepted_application);

    env.ledger().set_timestamp(expiry);
    assert_eq!(
        client.try_accept_terms(&mentee, &program, &hash(&env, 43)),
        Err(Ok(ContractError::InvalidExpiry))
    );
    let receipt = client.get_acceptance(&program, &accepted_application);
    assert_eq!(receipt.terms.expires_at, expiry);
    assert_eq!(receipt.terms.hashes, hashes(&env));
}

#[test]
fn rejects_missing_or_invalid_terms_and_duplicate_program_registration() {
    let (env, contract_id, admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_get_latest_version(&hash(&env, 99)),
        Err(Ok(ContractError::NoTermsPublished))
    );
    assert_eq!(
        client.try_get_terms(&program, &0),
        Err(Ok(ContractError::TermsVersionNotFound))
    );
    assert_eq!(
        client.try_publish_terms(&owner, &hash(&env, 98), &hashes(&env), &500),
        Err(Ok(ContractError::ProgramNotFound))
    );
    assert_eq!(
        client.try_register_program(&admin, &program, &owner),
        Err(Ok(ContractError::ProgramAlreadyExists))
    );

    let mut invalid = hashes(&env);
    invalid.privacy = hash(&env, 0);
    assert_eq!(
        client.try_publish_terms(&owner, &program, &invalid, &500),
        Err(Ok(ContractError::InvalidTermsHash))
    );
    assert_eq!(
        client.try_publish_terms(&owner, &program, &hashes(&env), &0),
        Err(Ok(ContractError::InvalidExpiry))
    );
    assert_eq!(
        client.try_get_acceptance(&program, &hash(&env, 44)),
        Err(Ok(ContractError::AcceptanceNotFound))
    );
}

#[test]
fn publication_storage_is_bounded() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    for _ in 0..64 {
        publish(&env, &client, &owner, &program, &hashes(&env));
    }
    assert_eq!(client.get_latest_version(&program), 64);
    assert_eq!(
        client.try_publish_terms(&owner, &program, &hashes(&env), &500),
        Err(Ok(ContractError::VersionLimitReached))
    );
}

#[test]
fn storage_schema_version_and_persisted_records_are_decodable() {
    let (env, contract_id, _admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    let version_hashes = hashes(&env);
    publish(&env, &client, &owner, &program, &version_hashes);
    let mentee = Address::generate(&env);
    let application = hash(&env, 45);
    client.accept_terms(&mentee, &program, &application);

    assert_eq!(client.storage_version(), 1);
    assert_eq!(client.get_terms(&program, &1).hashes, version_hashes);
    assert_eq!(
        client.get_acceptance(&program, &application).terms.hashes,
        version_hashes
    );
}

#[test]
fn initialization_is_one_time_and_admin_errors_are_stable() {
    let (env, contract_id, admin, owner, program) = setup();
    let client = crate::MentorshipTermsContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_register_program(&attacker, &hash(&env, 88), &owner),
        Err(Ok(ContractError::NotAdmin))
    );

    let uninitialized_env = Env::default();
    uninitialized_env.mock_all_auths();
    let uninitialized_id = uninitialized_env.register(MentorshipTermsContract, ());
    let uninitialized =
        crate::MentorshipTermsContractClient::new(&uninitialized_env, &uninitialized_id);
    assert_eq!(
        uninitialized.try_register_program(&admin, &program, &owner),
        Err(Ok(ContractError::NotInitialized))
    );
}