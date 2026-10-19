extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, DeliveryMode, LocationVisibility, MentorDeliveryContract,
    MentorDeliveryContractClient,
};

fn setup() -> (Env, Address, MentorDeliveryContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorDeliveryContract, ());
    let client = MentorDeliveryContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn remote_configuration_excludes_physical_location_metadata() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let program_id = hash(&env, 1);
    assert_eq!(
        client.configure_program(
            &owner,
            &program_id,
            &DeliveryMode::Remote,
            &LocationVisibility::NotApplicable,
            &None,
            &hash(&env, 2),
            &false,
            &1,
        ),
        1
    );
    let config = client.get_config(&program_id);
    assert_eq!(config.mode, DeliveryMode::Remote);
    assert_eq!(config.location_commitment, None);
    assert!(!config.physical_safety_confirmed);
}

#[test]
fn in_person_and_hybrid_require_location_and_safety_commitments() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let in_person = hash(&env, 3);
    let hybrid = hash(&env, 4);

    assert_eq!(
        client.try_configure_program(
            &owner,
            &in_person,
            &DeliveryMode::InPerson,
            &LocationVisibility::MatchedOnly,
            &None,
            &hash(&env, 5),
            &true,
            &1,
        ),
        Err(Ok(ContractError::InvalidModeConfiguration))
    );
    assert_eq!(
        client.try_configure_program(
            &owner,
            &in_person,
            &DeliveryMode::InPerson,
            &LocationVisibility::MatchedOnly,
            &Some(hash(&env, 6)),
            &hash(&env, 5),
            &false,
            &1,
        ),
        Err(Ok(ContractError::InvalidModeConfiguration))
    );
    client.configure_program(
        &owner,
        &in_person,
        &DeliveryMode::InPerson,
        &LocationVisibility::MatchedOnly,
        &Some(hash(&env, 6)),
        &hash(&env, 5),
        &true,
        &1,
    );
    client.configure_program(
        &owner,
        &hybrid,
        &DeliveryMode::Hybrid,
        &LocationVisibility::PublicArea,
        &Some(hash(&env, 7)),
        &hash(&env, 8),
        &true,
        &1,
    );
    assert_eq!(client.get_config(&hybrid).location_visibility, LocationVisibility::PublicArea);
}

#[test]
fn owner_updates_are_versioned_and_auditable() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let program_id = hash(&env, 9);
    client.configure_program(
        &owner,
        &program_id,
        &DeliveryMode::Remote,
        &LocationVisibility::NotApplicable,
        &None,
        &hash(&env, 10),
        &false,
        &1,
    );
    assert_eq!(
        client.update_program(
            &owner,
            &program_id,
            &DeliveryMode::Hybrid,
            &LocationVisibility::Hidden,
            &Some(hash(&env, 11)),
            &hash(&env, 12),
            &true,
            &2,
        ),
        2
    );
    let current = client.get_config(&program_id);
    assert_eq!(current.version, 2);
    assert_eq!(current.mode, DeliveryMode::Hybrid);
    assert_eq!(current.created_at, client.get_revision(&program_id, &1).updated_at);
    let first = client.get_revision(&program_id, &1);
    assert_eq!(first.mode, DeliveryMode::Remote);
    assert_eq!(first.location_commitment, None);
}

#[test]
fn validation_rejects_zero_commitments_and_wrong_mode_combinations() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let program_id = hash(&env, 13);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    assert_eq!(
        client.try_configure_program(
            &owner,
            &program_id,
            &DeliveryMode::Remote,
            &LocationVisibility::NotApplicable,
            &None,
            &zero,
            &false,
            &1,
        ),
        Err(Ok(ContractError::InvalidCommitment))
    );
    assert_eq!(
        client.try_configure_program(
            &owner,
            &program_id,
            &DeliveryMode::Remote,
            &LocationVisibility::PublicArea,
            &Some(hash(&env, 14)),
            &hash(&env, 15),
            &false,
            &1,
        ),
        Err(Ok(ContractError::InvalidModeConfiguration))
    );
}

#[test]
fn another_address_cannot_update_the_program() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let program_id = hash(&env, 16);
    client.configure_program(
        &owner,
        &program_id,
        &DeliveryMode::Remote,
        &LocationVisibility::NotApplicable,
        &None,
        &hash(&env, 17),
        &false,
        &1,
    );
    env.mock_auths(&[]);
    assert!(client
        .try_update_program(
            &stranger,
            &program_id,
            &DeliveryMode::Remote,
            &LocationVisibility::NotApplicable,
            &None,
            &hash(&env, 18),
            &false,
            &1,
        )
        .is_err());
    assert_eq!(client.get_config(&program_id).version, 1);
}