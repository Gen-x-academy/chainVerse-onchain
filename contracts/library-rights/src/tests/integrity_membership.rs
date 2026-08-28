use crate::{ContentStatus, ContractError, LibraryRightsContractClient, MembershipStatus};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

fn bootstrapped(env: &Env) -> (LibraryRightsContractClient<'_>, Address, Address) {
    env.mock_all_auths();
    let contract_id = env.register(crate::LibraryRightsContract, ());
    let client = LibraryRightsContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let policy = Address::generate(env);
    let emergency = Address::generate(env);
    client.bootstrap(&admin, &treasury, &policy, &emergency);
    (client, policy, emergency)
}

fn bytes(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

#[test]
fn quarantine_is_immediate_and_preserves_evidence() {
    let env = Env::default();
    let (client, policy, emergency) = bootstrapped(&env);
    let work = bytes(&env, 1);
    client.put_work(&policy, &work, &bytes(&env, 2), &Address::generate(&env));
    env.ledger().set_timestamp(100);

    client.quarantine_work(&emergency, &work, &bytes(&env, 3));

    assert_eq!(client.content_status(&work), ContentStatus::Quarantined);
    assert!(!client.is_work_accessible(&work));
    let record = client.quarantine_record(&work);
    assert_eq!(record.reason_hash, bytes(&env, 3));
    assert_eq!(record.quarantined_at, 100);
    assert_eq!(record.restored_at, None);
}

#[test]
fn quarantine_requires_emergency_and_cannot_repeat() {
    let env = Env::default();
    let (client, policy, emergency) = bootstrapped(&env);
    let work = bytes(&env, 4);
    client.put_work(&policy, &work, &bytes(&env, 5), &Address::generate(&env));
    let attacker = Address::generate(&env);

    assert_eq!(
        client.try_quarantine_work(&attacker, &work, &bytes(&env, 6)),
        Err(Ok(ContractError::NotAdmin))
    );
    client.quarantine_work(&emergency, &work, &bytes(&env, 6));
    assert_eq!(
        client.try_quarantine_work(&emergency, &work, &bytes(&env, 7)),
        Err(Ok(ContractError::AlreadyQuarantined))
    );
}

#[test]
fn quarantine_is_distinct_from_legal_takedown_and_restoration_requires_review() {
    let env = Env::default();
    let (client, policy, emergency) = bootstrapped(&env);
    let work = bytes(&env, 8);
    client.put_work(&policy, &work, &bytes(&env, 9), &Address::generate(&env));
    client.legal_takedown_work(&policy, &work);
    assert_eq!(client.content_status(&work), ContentStatus::LegalTakedown);
    assert_eq!(
        client.try_quarantine_work(&emergency, &work, &bytes(&env, 10)),
        Err(Ok(ContractError::InvalidStateTransition))
    );

    let work2 = bytes(&env, 11);
    client.put_work(&policy, &work2, &bytes(&env, 12), &Address::generate(&env));
    client.quarantine_work(&emergency, &work2, &bytes(&env, 13));
    client.restore_quarantined_work(&policy, &work2, &bytes(&env, 14));
    assert_eq!(client.content_status(&work2), ContentStatus::Active);
    assert!(client.is_work_accessible(&work2));
    assert_eq!(
        client.quarantine_record(&work2).restoration_review_hash,
        Some(bytes(&env, 14))
    );
}

#[test]
fn membership_is_scoped_expires_and_rotates() {
    let env = Env::default();
    let (client, policy, _) = bootstrapped(&env);
    let wallet = Address::generate(&env);
    let claim = bytes(&env, 20);
    let domain = bytes(&env, 21);
    let network = bytes(&env, 22);
    env.ledger().set_timestamp(1000);

    let first = client.attest_membership(&policy, &wallet, &claim, &domain, &network, &1100);
    assert!(client.is_membership_active(&wallet, &claim, &domain, &network));
    assert!(!client.is_membership_active(&wallet, &claim, &bytes(&env, 23), &network));
    assert!(!client.is_membership_active(&wallet, &claim, &domain, &bytes(&env, 24)));
    env.ledger().set_timestamp(1100);
    assert!(!client.is_membership_active(&wallet, &claim, &domain, &network));

    env.ledger().set_timestamp(1050);
    let second =
        client.attest_membership(&policy, &wallet, &bytes(&env, 25), &domain, &network, &1200);
    assert_ne!(first, second);
    assert_eq!(
        client.membership_attestation(&first).status,
        MembershipStatus::Revoked
    );
    assert!(client.is_membership_active(&wallet, &bytes(&env, 25), &domain, &network));
}

#[test]
fn membership_rejects_past_expiry_and_unauthorized_issuer() {
    let env = Env::default();
    let (client, policy, _) = bootstrapped(&env);
    env.ledger().set_timestamp(500);
    let wallet = Address::generate(&env);
    let attacker = Address::generate(&env);
    let args = (
        &wallet,
        &bytes(&env, 30),
        &bytes(&env, 31),
        &bytes(&env, 32),
        &400u64,
    );
    assert_eq!(
        client.try_attest_membership(&policy, args.0, args.1, args.2, args.3, args.4),
        Err(Ok(ContractError::InvalidStateTransition))
    );
    assert_eq!(
        client.try_attest_membership(&attacker, args.0, args.1, args.2, args.3, &600),
        Err(Ok(ContractError::NotAdmin))
    );
}
