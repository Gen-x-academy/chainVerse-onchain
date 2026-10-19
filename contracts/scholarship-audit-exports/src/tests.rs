#![cfg(test)]

use crate::{
    AuditExport, ContractError, EvidenceHashes, ScholarshipAuditExportsContract,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _}, symbol_short, Address, BytesN, Env, Symbol,
};

fn setup() -> (Env, Address, Address, Address, BytesN<32>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAuditExportsContract, ());
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let program = hash(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &program, &sponsor);
    (env, contract_id, admin, sponsor, program)
}

fn hash(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

fn evidence(env: &Env) -> EvidenceHashes {
    EvidenceHashes {
        governance: hash(env, 1),
        decisions: hash(env, 2),
        consent: hash(env, 3),
        funds: hash(env, 4),
        payments: hash(env, 5),
    }
}

fn publish(
    env: &Env,
    client: &crate::ScholarshipAuditExportsContractClient,
    sponsor: &Address,
    program: &BytesN<32>,
    export_id: &BytesN<32>,
) {
    client.publish_export(
        sponsor,
        export_id,
        program,
        &10,
        &20,
        &hash(env, 6),
        &hash(env, 7),
        &evidence(env),
    );
}

#[test]
fn program_registration_event_identifies_sponsor() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    assert_eq!(&events.get(0).unwrap().0, &contract_id);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("AUDPROG"));
    let (event_program, event_sponsor): (BytesN<32>, Address) = data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(event_sponsor, sponsor);
}

#[test]
fn sponsor_publishes_scoped_manifest_and_event_schema() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    let export_id = hash(&env, 8);
    publish(&env, &client, &sponsor, &program, &export_id);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("AUDEXPORT"));
    let (event_id, event_program, package_hash, start, end, created_at):
        (BytesN<32>, BytesN<32>, BytesN<32>, u64, u64, u64) = data.try_into_val(&env).unwrap();
    assert_eq!(event_id, export_id);
    assert_eq!(event_program, program);
    assert_eq!(package_hash, hash(&env, 6));
    assert_eq!((start, end), (10, 20));
    assert_eq!(created_at, env.ledger().timestamp());

    let record = client.get_export(&sponsor, &program, &export_id);
    assert_eq!(record.evidence_hashes, evidence(&env));
    assert_eq!(record.schema_version, 1);
}

#[test]
fn regulator_access_is_program_scoped_and_revocable() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    let export_id = hash(&env, 9);
    publish(&env, &client, &sponsor, &program, &export_id);
    let regulator = Address::generate(&env);

    assert_eq!(
        client.try_get_export(&regulator, &program, &export_id),
        Err(Ok(ContractError::Unauthorized))
    );
    client.grant_regulator(&sponsor, &program, &regulator);
    let events = env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("AUDGRNT"));
    let (event_program, event_regulator): (BytesN<32>, Address) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(event_regulator, regulator);
    let record: AuditExport = client.get_export(&regulator, &program, &export_id);
    assert_eq!(record.export_id, export_id);

    client.revoke_regulator(&sponsor, &program, &regulator);
    let events = env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("AUDRVK"));
    let (event_program, event_regulator): (BytesN<32>, Address) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_program, program);
    assert_eq!(event_regulator, regulator);
    assert_eq!(
        client.try_get_export(&regulator, &program, &export_id),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn unauthorized_publisher_and_grantor_are_denied() {
    let (env, contract_id, _admin, _sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_publish_export(
            &attacker,
            &hash(&env, 10),
            &program,
            &10,
            &20,
            &hash(&env, 6),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::NotProgramSponsor))
    );
    assert_eq!(
        client.try_grant_regulator(&attacker, &program, &Address::generate(&env)),
        Err(Ok(ContractError::Unauthorized))
    );
}

#[test]
fn exact_retries_are_idempotent_and_conflicting_ids_fail() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    let export_id = hash(&env, 11);
    publish(&env, &client, &sponsor, &program, &export_id);
    assert_eq!(client.export_count(&program), 1);

    let later_timestamp = env.ledger().timestamp() + 1;
    env.ledger().set_timestamp(later_timestamp);
    publish(&env, &client, &sponsor, &program, &export_id);
    assert_eq!(client.export_count(&program), 1);
    assert!(env.events().all().is_empty());

    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &export_id,
            &program,
            &10,
            &20,
            &hash(&env, 12),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::ExportIdConflict))
    );
}

#[test]
fn rejects_invalid_periods_hashes_and_missing_records() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &hash(&env, 13),
            &program,
            &21,
            &20,
            &hash(&env, 6),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::InvalidPeriod))
    );
    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &hash(&env, 14),
            &program,
            &10,
            &(env.ledger().timestamp() + 1),
            &hash(&env, 6),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::InvalidPeriod))
    );

    let mut hashes = evidence(&env);
    hashes.consent = hash(&env, 0);
    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &hash(&env, 15),
            &program,
            &10,
            &20,
            &hash(&env, 6),
            &hash(&env, 7),
            &hashes,
        ),
        Err(Ok(ContractError::InvalidEvidenceHash))
    );
    assert_eq!(
        client.try_get_export(&sponsor, &program, &hash(&env, 16)),
        Err(Ok(ContractError::ExportNotFound))
    );
    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &hash(&env, 17),
            &hash(&env, 99),
            &10,
            &20,
            &hash(&env, 6),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::ProgramNotFound))
    );
}

#[test]
fn per_program_export_count_is_bounded() {
    let (env, contract_id, _admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    for index in 1..=64 {
        publish(&env, &client, &sponsor, &program, &hash(&env, index));
    }
    assert_eq!(client.export_count(&program), 64);
    assert_eq!(
        client.try_publish_export(
            &sponsor,
            &hash(&env, 100),
            &program,
            &10,
            &20,
            &hash(&env, 6),
            &hash(&env, 7),
            &evidence(&env),
        ),
        Err(Ok(ContractError::ExportLimitReached))
    );
}

#[test]
fn initialization_and_registration_errors_are_stable() {
    let (env, contract_id, admin, sponsor, program) = setup();
    let client = crate::ScholarshipAuditExportsContractClient::new(&env, &contract_id);
    assert_eq!(client.version(), 1);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    assert_eq!(
        client.try_register_program(&Address::generate(&env), &hash(&env, 101), &sponsor),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_register_program(&admin, &program, &sponsor),
        Err(Ok(ContractError::ProgramAlreadyExists))
    );

    let uninitialized_env = Env::default();
    uninitialized_env.mock_all_auths();
    let id = uninitialized_env.register(ScholarshipAuditExportsContract, ());
    let uninitialized = crate::ScholarshipAuditExportsContractClient::new(&uninitialized_env, &id);
    assert_eq!(
        uninitialized.try_register_program(&admin, &program, &sponsor),
        Err(Ok(ContractError::NotInitialized))
    );
}