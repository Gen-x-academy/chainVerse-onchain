#![cfg(test)]
use crate::{ContractError, FundingModel, ProgramStatus, ScholarshipCoreContract};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Symbol};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipCoreContract, ());
    let owner = Address::generate(&env);
    (env, contract_id, owner)
}

fn ids(env: &Env, byte: u8) -> (BytesN<32>, BytesN<32>) {
    (
        BytesN::from_array(env, &[byte; 32]),
        BytesN::from_array(env, &[byte.wrapping_add(100); 32]),
    )
}

fn create_test_program(
    env: &Env,
    client: &crate::ScholarshipCoreContractClient,
    owner: &Address,
    program_id: &BytesN<32>,
    sponsor_id: &BytesN<32>,
) {
    client.create_program(
        owner,
        program_id,
        sponsor_id,
        &String::from_str(env, "STEM Bursary"),
        &String::from_str(env, "For undergraduate STEM students"),
        &Symbol::new(env, "USD"),
        &FundingModel::FixedAward,
    );
}

// ── #1058 — program data model ──────────────────────────────────────────────

#[test]
fn test_create_program_starts_in_draft() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    let program = client.get_program(&pid);
    assert_eq!(program.status, ProgramStatus::Draft);
    assert_eq!(program.id, pid);
    assert_eq!(program.sponsor_id, sid);
    assert_eq!(program.owner, owner);
}

#[test]
fn test_create_program_rejects_duplicate_id() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    let result = client.try_create_program(
        &owner,
        &pid,
        &sid,
        &String::from_str(&env, "Different Title"),
        &String::from_str(&env, "Different description"),
        &Symbol::new(&env, "USD"),
        &FundingModel::MatchingFund,
    );
    assert_eq!(result, Err(Ok(ContractError::ProgramAlreadyExists)));
}

#[test]
fn test_create_program_rejects_empty_title() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);

    let result = client.try_create_program(
        &owner,
        &pid,
        &sid,
        &String::from_str(&env, ""),
        &String::from_str(&env, "desc"),
        &Symbol::new(&env, "USD"),
        &FundingModel::FixedAward,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidTitle)));
}

#[test]
fn test_get_nonexistent_program_fails() {
    let (env, contract_id, _owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, _sid) = ids(&env, 1);

    let result = client.try_get_program(&pid);
    assert_eq!(result, Err(Ok(ContractError::ProgramNotFound)));
}

// ── #1061 — explicit lifecycle ───────────────────────────────────────────────

#[test]
fn test_draft_to_published_is_legal() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    client.transition_program(&owner, &pid, &ProgramStatus::Published);
    assert_eq!(client.get_program_status(&pid), ProgramStatus::Published);
}

#[test]
fn test_published_to_draft_is_illegal() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);
    client.transition_program(&owner, &pid, &ProgramStatus::Published);

    let result = client.try_transition_program(&owner, &pid, &ProgramStatus::Draft);
    assert_eq!(result, Err(Ok(ContractError::InvalidTransition)));
}

#[test]
fn test_full_legal_lifecycle_published_paused_closed_archived() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    client.transition_program(&owner, &pid, &ProgramStatus::Published);
    client.transition_program(&owner, &pid, &ProgramStatus::Paused);
    client.transition_program(&owner, &pid, &ProgramStatus::Published); // resume
    client.transition_program(&owner, &pid, &ProgramStatus::Closed);
    client.transition_program(&owner, &pid, &ProgramStatus::Archived);

    assert_eq!(client.get_program_status(&pid), ProgramStatus::Archived);
}

#[test]
fn test_draft_can_be_archived_directly() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    client.transition_program(&owner, &pid, &ProgramStatus::Archived);
    assert_eq!(client.get_program_status(&pid), ProgramStatus::Archived);
}

#[test]
fn test_archived_is_terminal() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);
    client.transition_program(&owner, &pid, &ProgramStatus::Archived);

    let result = client.try_transition_program(&owner, &pid, &ProgramStatus::Published);
    assert_eq!(result, Err(Ok(ContractError::InvalidTransition)));
}

#[test]
fn test_only_owner_can_transition() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);
    let stranger = Address::generate(&env);

    let result = client.try_transition_program(&stranger, &pid, &ProgramStatus::Published);
    assert_eq!(result, Err(Ok(ContractError::NotProgramOwner)));
}

#[test]
fn test_transition_records_actor_and_time() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);

    env.ledger().set_timestamp(777);
    client.transition_program(&owner, &pid, &ProgramStatus::Published);

    let program = client.get_program(&pid);
    assert_eq!(program.last_transitioned_by, owner);
    assert_eq!(program.last_transitioned_at, 777);
}

#[test]
fn test_archived_program_remains_readable() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);
    client.transition_program(&owner, &pid, &ProgramStatus::Archived);

    // Archived programs remain auditable/retrievable, not deleted.
    let program = client.get_program(&pid);
    assert_eq!(program.status, ProgramStatus::Archived);
    assert_eq!(program.id, pid);
}

#[test]
fn test_closed_to_published_is_illegal() {
    let (env, contract_id, owner) = setup();
    let client = crate::ScholarshipCoreContractClient::new(&env, &contract_id);
    let (pid, sid) = ids(&env, 1);
    create_test_program(&env, &client, &owner, &pid, &sid);
    client.transition_program(&owner, &pid, &ProgramStatus::Published);
    client.transition_program(&owner, &pid, &ProgramStatus::Closed);

    let result = client.try_transition_program(&owner, &pid, &ProgramStatus::Published);
    assert_eq!(result, Err(Ok(ContractError::InvalidTransition)));
}
