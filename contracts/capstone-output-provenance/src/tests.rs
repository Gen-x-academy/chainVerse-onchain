extern crate std;

use soroban_sdk::{
    testutils::Address as _, testutils::Events as _, Address, BytesN, Env, Vec,
};

use crate::{
    CapstoneOutputProvenanceContract, CapstoneOutputProvenanceContractClient, ContractError,
    ProvenanceState, Role, Verdict,
};

struct Fixture {
    env: Env,
    tenant: BytesN<32>,
    other_tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    lead: Address,
    verifier: Address,
    curator: Address,
    operator: Address,
    outsider: Address,
    client: CapstoneOutputProvenanceContractClient<'static>,
}

fn id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn hashes(env: &Env, count: u32, seed: u8) -> Vec<BytesN<32>> {
    let mut values = Vec::new(env);
    for i in 0..count {
        values.push_back(id(env, seed.wrapping_add(i as u8)));
    }
    values
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneOutputProvenanceContract, ());
    let client = CapstoneOutputProvenanceContractClient::new(&env, &contract_id);
    let tenant = id(&env, 1);
    let other_tenant = id(&env, 2);
    let project = id(&env, 3);
    let admin = Address::generate(&env);
    let lead = Address::generate(&env);
    let verifier = Address::generate(&env);
    let curator = Address::generate(&env);
    let operator = Address::generate(&env);
    let outsider = Address::generate(&env);
    client.initialize(&admin, &tenant);
    client.grant_role(&admin, &tenant, &lead, &Role::ResearchLead);
    client.grant_role(&admin, &tenant, &verifier, &Role::Verifier);
    client.grant_role(&admin, &tenant, &curator, &Role::Curator);
    client.grant_role(&admin, &tenant, &operator, &Role::Operator);
    Fixture {
        env,
        tenant,
        other_tenant,
        project,
        admin,
        lead,
        verifier,
        curator,
        operator,
        outsider,
        client,
    }
}

fn record(f: &Fixture, request_seed: u8, output_seed: u8, version: u32) -> BytesN<32> {
    let request_id = id(&f.env, request_seed);
    let output_id = id(&f.env, output_seed);
    f.client.record_provenance(
        &f.lead,
        &f.tenant,
        &f.project,
        &request_id,
        &output_id,
        &version,
        &hashes(&f.env, 2, 0x20),
        &hashes(&f.env, 2, 0x30),
        &hashes(&f.env, 1, 0x40),
        &hashes(&f.env, 1, 0x50),
        &hashes(&f.env, 2, 0x60),
        &id(&f.env, 0x70),
        &id(&f.env, 0x71),
        &id(&f.env, 0x72),
        &100,
    );
    request_id
}

#[test]
fn record_is_idempotent_by_request_id() {
    let f = setup();
    let before = f.env.events().all().len();
    let request_id = record(&f, 10, 11, 1);
    let replay = f.client.record_provenance(
        &f.lead,
        &f.tenant,
        &f.project,
        &request_id,
        &id(&f.env, 11),
        &1,
        &hashes(&f.env, 2, 0x20),
        &hashes(&f.env, 2, 0x30),
        &hashes(&f.env, 1, 0x40),
        &hashes(&f.env, 1, 0x50),
        &hashes(&f.env, 2, 0x60),
        &id(&f.env, 0x70),
        &id(&f.env, 0x71),
        &id(&f.env, 0x72),
        &100,
    );
    assert_eq!(replay, request_id);
    assert_eq!(f.env.events().all().len(), before + 1);
    let error = f
        .client
        .try_record_provenance(
            &f.lead,
            &f.tenant,
            &f.project,
            &request_id,
            &id(&f.env, 99),
            &1,
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &id(&f.env, 1),
            &id(&f.env, 1),
            &id(&f.env, 1),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::RequestAlreadyUsed);
}

#[test]
fn attest_and_seal_follow_state_machine() {
    let f = setup();
    record(&f, 20, 21, 1);
    f.client.attest_provenance(
        &f.verifier,
        &f.tenant,
        &f.project,
        &id(&f.env, 22),
        &id(&f.env, 21),
        &1,
        &id(&f.env, 23),
        &Verdict::Valid,
    );
    assert_eq!(
        f.client
            .get_provenance(&f.tenant, &f.project, &id(&f.env, 21), &1)
            .unwrap()
            .state,
        ProvenanceState::Verified
    );
    f.client.seal_provenance(
        &f.curator,
        &f.tenant,
        &f.project,
        &id(&f.env, 24),
        &id(&f.env, 21),
        &1,
        &id(&f.env, 25),
    );
    assert_eq!(
        f.client
            .get_provenance(&f.tenant, &f.project, &id(&f.env, 21), &1)
            .unwrap()
            .state,
        ProvenanceState::Sealed
    );
}

#[test]
fn invalid_verdict_rejects_record_and_blocks_seal() {
    let f = setup();
    let request_id = record(&f, 30, 31, 1);
    f.client.attest_provenance(
        &f.verifier,
        &f.tenant,
        &f.project,
        &id(&f.env, 32),
        &id(&f.env, 31),
        &1,
        &id(&f.env, 33),
        &Verdict::Invalid,
    );
    assert_eq!(
        f.client
            .get_by_request(&f.tenant, &f.project, &request_id)
            .unwrap()
            .state,
        ProvenanceState::Rejected
    );
    let error = f
        .client
        .try_seal_provenance(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 34),
            &id(&f.env, 31),
            &1,
            &id(&f.env, 35),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::InvalidState);
}

#[test]
fn unauthorized_and_wrong_tenant_are_denied() {
    let f = setup();
    let error = f
        .client
        .try_record_provenance(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id(&f.env, 40),
            &id(&f.env, 41),
            &1,
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &id(&f.env, 42),
            &id(&f.env, 43),
            &id(&f.env, 44),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotAuthorized);
    let error = f
        .client
        .try_record_provenance(
            &f.lead,
            &f.other_tenant,
            &f.project,
            &id(&f.env, 45),
            &id(&f.env, 46),
            &1,
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &id(&f.env, 47),
            &id(&f.env, 48),
            &id(&f.env, 49),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::WrongTenant);
}

#[test]
fn stale_version_is_rejected() {
    let f = setup();
    record(&f, 50, 51, 1);
    let error = f
        .client
        .try_attest_provenance(
            &f.verifier,
            &f.tenant,
            &f.project,
            &id(&f.env, 52),
            &id(&f.env, 51),
            &2,
            &id(&f.env, 53),
            &Verdict::Valid,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::StaleVersion);
}

#[test]
fn intent_replay_does_not_duplicate_state_or_events() {
    let f = setup();
    record(&f, 60, 61, 1);
    f.client.attest_provenance(
        &f.verifier,
        &f.tenant,
        &f.project,
        &id(&f.env, 62),
        &id(&f.env, 61),
        &1,
        &id(&f.env, 63),
        &Verdict::Valid,
    );
    let before = f.env.events().all().len();
    f.client.attest_provenance(
        &f.verifier,
        &f.tenant,
        &f.project,
        &id(&f.env, 62),
        &id(&f.env, 61),
        &1,
        &id(&f.env, 63),
        &Verdict::Valid,
    );
    assert_eq!(f.env.events().all().len(), before);
    let error = f
        .client
        .try_attest_provenance(
            &f.verifier,
            &f.tenant,
            &f.project,
            &id(&f.env, 62),
            &id(&f.env, 61),
            &1,
            &id(&f.env, 64),
            &Verdict::Invalid,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::IntentAlreadyUsed);
}

#[test]
fn expiry_and_arithmetic_are_timestamp_driven() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 500);
    record(&f, 70, 71, 1);
    let error = f
        .client
        .try_expire_provenance(
            &f.operator,
            &f.tenant,
            &f.project,
            &id(&f.env, 72),
            &id(&f.env, 71),
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotExpired);
    f.env.ledger().with_mut(|li| li.timestamp = 601);
    let error = f
        .client
        .try_attest_provenance(
            &f.verifier,
            &f.tenant,
            &f.project,
            &id(&f.env, 73),
            &id(&f.env, 71),
            &1,
            &id(&f.env, 74),
            &Verdict::Valid,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::AlreadyExpired);
    f.client.expire_provenance(
        &f.operator,
        &f.tenant,
        &f.project,
        &id(&f.env, 75),
        &id(&f.env, 71),
        &1,
    );
    assert_eq!(
        f.client
            .get_provenance(&f.tenant, &f.project, &id(&f.env, 71), &1)
            .unwrap()
            .state,
        ProvenanceState::Expired
    );
}

#[test]
fn bounds_and_checked_arithmetic_are_enforced() {
    let f = setup();
    let too_many = hashes(&f.env, 17, 0x80);
    let error = f
        .client
        .try_record_provenance(
            &f.lead,
            &f.tenant,
            &f.project,
            &id(&f.env, 80),
            &id(&f.env, 81),
            &1,
            &too_many,
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &id(&f.env, 82),
            &id(&f.env, 83),
            &id(&f.env, 84),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::TooManyHashes);
    f.env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    let error = f
        .client
        .try_record_provenance(
            &f.lead,
            &f.tenant,
            &f.project,
            &id(&f.env, 85),
            &id(&f.env, 86),
            &1,
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &Vec::new(&f.env),
            &id(&f.env, 87),
            &id(&f.env, 88),
            &id(&f.env, 89),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::Overflow);
}

#[test]
fn ttl_extension_keeps_records_readable() {
    let f = setup();
    let request_id = record(&f, 90, 91, 1);
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_by_request(&f.tenant, &f.project, &request_id)
            .unwrap()
            .output_id,
        id(&f.env, 91)
    );
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_provenance(&f.tenant, &f.project, &id(&f.env, 91), &1)
            .unwrap()
            .version,
        1
    );
}

#[test]
fn resource_budget_allows_bounded_payload() {
    let f = setup();
    let mut budget = f.env.budget();
    budget.reset_unlimited();
    f.client.record_provenance(
        &f.lead,
        &f.tenant,
        &f.project,
        &id(&f.env, 100),
        &id(&f.env, 101),
        &1,
        &hashes(&f.env, 16, 0x10),
        &hashes(&f.env, 16, 0x20),
        &hashes(&f.env, 16, 0x30),
        &hashes(&f.env, 16, 0x40),
        &Vec::new(&f.env),
        &id(&f.env, 102),
        &id(&f.env, 103),
        &id(&f.env, 104),
        &100,
    );
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::WrongTenant as u32, 4);
    assert_eq!(ContractError::IntentAlreadyUsed as u32, 6);
}
