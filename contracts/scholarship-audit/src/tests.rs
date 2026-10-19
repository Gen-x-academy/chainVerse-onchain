#![cfg(test)]
extern crate std;

use crate::{
    AuditAction, AuditDomain, ContractError, ScholarshipAuditContract,
    ScholarshipAuditContractClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAuditContract, ());
    let admin = Address::generate(&env);
    let source = Address::generate(&env);
    let client = ScholarshipAuditContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.register_source(&admin, &source);
    (env, contract_id, admin, source)
}

fn commitment(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

#[test]
fn records_form_an_append_only_hash_chain() {
    let (env, contract_id, _admin, source) = setup();
    let client = ScholarshipAuditContractClient::new(&env, &contract_id);

    let first = client.record(
        &source,
        &AuditDomain::Program,
        &AuditAction::Create,
        &commitment(&env, 1),
        &commitment(&env, 2),
        &commitment(&env, 3),
    );
    let second = client.record(
        &source,
        &AuditDomain::SecurityRead,
        &AuditAction::Read,
        &commitment(&env, 4),
        &commitment(&env, 5),
        &commitment(&env, 6),
    );

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert_eq!(second.previous_hash, first.record_hash);
    assert_eq!(client.get_head(), (2, second.record_hash));
}

#[test]
fn unregistered_sources_cannot_append() {
    let (env, contract_id, _admin, _) = setup();
    let client = ScholarshipAuditContractClient::new(&env, &contract_id);
    let unknown_source = Address::generate(&env);

    assert_eq!(
        client.try_record(
            &unknown_source,
            &AuditDomain::Finance,
            &AuditAction::Deposit,
            &commitment(&env, 1),
            &commitment(&env, 2),
            &commitment(&env, 3),
        ),
        Err(Ok(ContractError::SourceNotRegistered))
    );
}

#[test]
fn revoked_sources_cannot_append() {
    let (env, contract_id, admin, source) = setup();
    let client = ScholarshipAuditContractClient::new(&env, &contract_id);
    client.revoke_source(&admin, &source);

    assert_eq!(
        client.try_record(
            &source,
            &AuditDomain::Award,
            &AuditAction::Accept,
            &commitment(&env, 1),
            &commitment(&env, 2),
            &commitment(&env, 3),
        ),
        Err(Ok(ContractError::SourceNotRegistered))
    );
}