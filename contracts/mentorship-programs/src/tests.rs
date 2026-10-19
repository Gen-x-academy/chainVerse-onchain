#![cfg(test)]

use crate::{
    ContractError, DeliveryMode, MentorshipProgramsContract, ProgramStatus, ProgramVisibility,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _}, Address, BytesN, Env,
};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipProgramsContract, ());
    let organization = Address::generate(&env);
    (env, contract_id, organization)
}

fn bytes(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

fn create_program(
    env: &Env,
    client: &crate::MentorshipProgramsContractClient,
    organization: &Address,
    program_id: &BytesN<32>,
) {
    client.create_program(
        organization,
        program_id,
        &bytes(env, 2),
        &bytes(env, 3),
        &12,
        &90,
        &DeliveryMode::Remote,
        &1_800_000_000,
        &1_810_000_000,
        &ProgramVisibility::Public,
    );
}

#[test]
fn creates_org_owned_draft_with_privacy_commitments() {
    let (env, contract_id, organization) = setup();
    let client = crate::MentorshipProgramsContractClient::new(&env, &contract_id);
    let program_id = bytes(&env, 1);

    create_program(&env, &client, &organization, &program_id);

    let program = client.get_program(&program_id);
    assert_eq!(program.organization, organization);
    assert_eq!(program.audience_commitment, bytes(&env, 2));
    assert_eq!(program.outcomes_commitment, bytes(&env, 3));
    assert_eq!(program.capacity, 12);
    assert_eq!(program.duration_days, 90);
    assert_eq!(program.status, ProgramStatus::Draft);
    assert_eq!(program.visibility, ProgramVisibility::Public);
}

#[test]
fn rejects_duplicate_ids_and_invalid_configuration() {
    let (env, contract_id, organization) = setup();
    let client = crate::MentorshipProgramsContractClient::new(&env, &contract_id);
    let program_id = bytes(&env, 1);
    create_program(&env, &client, &organization, &program_id);

    let duplicate = client.try_create_program(
        &organization,
        &program_id,
        &bytes(&env, 2),
        &bytes(&env, 3),
        &12,
        &90,
        &DeliveryMode::Remote,
        &1_800_000_000,
        &1_810_000_000,
        &ProgramVisibility::Public,
    );
    assert_eq!(duplicate, Err(Ok(ContractError::ProgramAlreadyExists)));

    let invalid_dates = client.try_create_program(
        &organization,
        &bytes(&env, 4),
        &bytes(&env, 2),
        &bytes(&env, 3),
        &12,
        &90,
        &DeliveryMode::Remote,
        &1_810_000_000,
        &1_800_000_000,
        &ProgramVisibility::Public,
    );
    assert_eq!(invalid_dates, Err(Ok(ContractError::InvalidDates)));
}

#[test]
fn only_owner_can_edit_draft_and_published_terms_are_immutable() {
    let (env, contract_id, organization) = setup();
    let client = crate::MentorshipProgramsContractClient::new(&env, &contract_id);
    let program_id = bytes(&env, 1);
    create_program(&env, &client, &organization, &program_id);

    let stranger = Address::generate(&env);
    let unauthorized = client.try_update_draft(
        &stranger,
        &program_id,
        &bytes(&env, 4),
        &bytes(&env, 5),
        &10,
        &30,
        &DeliveryMode::Hybrid,
        &1_800_000_000,
        &1_810_000_000,
        &ProgramVisibility::Private,
    );
    assert_eq!(unauthorized, Err(Ok(ContractError::NotProgramOwner)));

    client.update_draft(
        &organization,
        &program_id,
        &bytes(&env, 6),
        &bytes(&env, 7),
        &15,
        &60,
        &DeliveryMode::Hybrid,
        &1_800_000_000,
        &1_820_000_000,
        &ProgramVisibility::Unlisted,
    );
    let updated = client.get_program(&program_id);
    assert_eq!(updated.audience_commitment, bytes(&env, 6));
    assert_eq!(updated.outcomes_commitment, bytes(&env, 7));
    assert_eq!(updated.capacity, 15);
    assert_eq!(updated.visibility, ProgramVisibility::Unlisted);

    client.transition_program(&organization, &program_id, &ProgramStatus::Published);
    let immutable = client.try_update_draft(
        &organization,
        &program_id,
        &bytes(&env, 4),
        &bytes(&env, 5),
        &10,
        &30,
        &DeliveryMode::Hybrid,
        &1_800_000_000,
        &1_810_000_000,
        &ProgramVisibility::Private,
    );
    assert_eq!(immutable, Err(Ok(ContractError::ProgramNotDraft)));
}

#[test]
fn lifecycle_rejects_illegal_transitions_and_records_update_time() {
    let (env, contract_id, organization) = setup();
    let client = crate::MentorshipProgramsContractClient::new(&env, &contract_id);
    let program_id = bytes(&env, 1);
    create_program(&env, &client, &organization, &program_id);

    client.transition_program(&organization, &program_id, &ProgramStatus::Published);
    let invalid = client.try_transition_program(
        &organization,
        &program_id,
        &ProgramStatus::Draft,
    );
    assert_eq!(invalid, Err(Ok(ContractError::InvalidTransition)));

    env.ledger().set_timestamp(777);
    client.transition_program(&organization, &program_id, &ProgramStatus::Paused);
    let program = client.get_program(&program_id);
    assert_eq!(program.status, ProgramStatus::Paused);
    assert_eq!(program.updated_at, 777);
}