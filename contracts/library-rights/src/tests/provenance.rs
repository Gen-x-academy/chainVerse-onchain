use crate::{ContractError, LibraryRightsContractClient, ProvenanceRecord, ProvenanceType};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

/// Bootstraps a fresh contract and returns a client plus the
/// PolicyManager address (the role allowed to attest provenance).
fn bootstrapped<'a>(
    env: &'a Env,
    contract_id: &Address,
) -> (LibraryRightsContractClient<'a>, Address) {
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(env, contract_id);
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let policy_manager = Address::generate(env);
    let emergency = Address::generate(env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);
    (client, policy_manager)
}

fn work_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// -- Positive --

#[test]
fn test_attest_and_read_provenance_round_trips() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work = work_id(&env, 1);
    let h = hash(&env, 2);

    client.attest_provenance(&policy_manager, &work, &ProvenanceType::Purchase, &h);

    assert_eq!(client.provenance_len(&work), 1);
    let record = client.get_provenance(&work, &1u64);
    assert_eq!(record.work_id, work);
    assert_eq!(record.provenance_type, ProvenanceType::Purchase);
    assert_eq!(record.provenance_hash, h);
    assert_eq!(record.attested_by, policy_manager);
    assert_eq!(record.previous_hash, None);
}

#[test]
fn test_corrections_append_instead_of_overwrite() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work = work_id(&env, 1);

    let first = hash(&env, 2);
    let corrected = hash(&env, 3);
    client.attest_provenance(&policy_manager, &work, &ProvenanceType::Donation, &first);
    client.attest_provenance(
        &policy_manager,
        &work,
        &ProvenanceType::Donation,
        &corrected,
    );

    // Both records remain; the latest links back to the previous one.
    assert_eq!(client.provenance_len(&work), 2);
    let first_record = client.get_provenance(&work, &1u64);
    assert_eq!(first_record.provenance_hash, first);
    assert_eq!(first_record.previous_hash, None);
    let second_record = client.get_provenance(&work, &2u64);
    assert_eq!(second_record.provenance_hash, corrected);
    assert_eq!(second_record.previous_hash, Some(first));
}

#[test]
fn test_provenance_is_per_work() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let work_a = work_id(&env, 1);
    let work_b = work_id(&env, 2);
    client.attest_provenance(
        &policy_manager,
        &work_a,
        &ProvenanceType::InstitutionalAcquisition,
        &hash(&env, 3),
    );

    assert_eq!(client.provenance_len(&work_a), 1);
    assert_eq!(client.provenance_len(&work_b), 0);
    assert_eq!(
        client.try_get_provenance(&work_b, &1u64),
        Err(Ok(ContractError::ProvenanceNotFound))
    );
}

// -- Negative --

#[test]
fn test_zero_hash_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work = work_id(&env, 1);
    let zero = BytesN::from_array(&env, &[0u8; 32]);

    let res =
        client.try_attest_provenance(&policy_manager, &work, &ProvenanceType::Purchase, &zero);
    assert_eq!(res, Err(Ok(ContractError::InvalidHash)));
    assert_eq!(client.provenance_len(&work), 0);
}

// -- Authorization --

#[test]
fn test_attest_requires_policy_manager_role() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);

    let work = work_id(&env, 1);
    let res =
        client.try_attest_provenance(&admin, &work, &ProvenanceType::Purchase, &hash(&env, 2));
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_attest_before_bootstrap_fails() {
    let (env, contract_id) = super::setup();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let caller = Address::generate(&env);
    let work = work_id(&env, 1);

    let res =
        client.try_attest_provenance(&caller, &work, &ProvenanceType::Purchase, &hash(&env, 2));
    assert_eq!(res, Err(Ok(ContractError::NotInitialized)));
}

// -- Boundary --

#[test]
fn test_history_queryable_only_within_bounds() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work = work_id(&env, 1);

    client.attest_provenance(
        &policy_manager,
        &work,
        &ProvenanceType::Purchase,
        &hash(&env, 2),
    );

    assert_eq!(
        client.try_get_provenance(&work, &0u64),
        Err(Ok(ContractError::ProvenanceNotFound))
    );
    assert_eq!(
        client.try_get_provenance(&work, &2u64),
        Err(Ok(ContractError::ProvenanceNotFound))
    );
}

#[test]
fn test_attestation_carries_ledger_timestamp() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    env.ledger().set_timestamp(1_700_000_000);
    let work = work_id(&env, 1);

    client.attest_provenance(
        &policy_manager,
        &work,
        &ProvenanceType::Donation,
        &hash(&env, 2),
    );

    let record: ProvenanceRecord = client.get_provenance(&work, &1u64);
    assert_eq!(record.attested_at, 1_700_000_000);
}

// -- Privacy --

/// Structural guarantee (mirrors `test_work_record_holds_only_hash_and_custodian`):
/// `ProvenanceRecord` carries no donor/invoice/identity fields -- only
/// the work id, acquisition type, document hash, attestor, timestamp,
/// and history link. Destructuring against the full field set stops
/// compiling the moment a privacy-violating field is added without
/// updating this test to justify it against `types.rs`.
#[test]
fn test_provenance_record_holds_only_public_facts() {
    let env = Env::default();
    let attested_by = Address::generate(&env);
    let record = ProvenanceRecord {
        work_id: work_id(&env, 1),
        provenance_type: ProvenanceType::Purchase,
        provenance_hash: hash(&env, 2),
        attested_by: attested_by.clone(),
        attested_at: 1_700_000_000,
        previous_hash: None,
    };

    let ProvenanceRecord {
        work_id: record_work_id,
        provenance_type,
        provenance_hash,
        attested_by: record_attestor,
        attested_at,
        previous_hash,
    } = record;
    assert_eq!(record_work_id, work_id(&env, 1));
    assert_eq!(provenance_type, ProvenanceType::Purchase);
    assert_eq!(provenance_hash, hash(&env, 2));
    assert_eq!(record_attestor, attested_by);
    assert_eq!(attested_at, 1_700_000_000);
    assert_eq!(previous_hash, None);
}
