use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{
    CapstoneRevisionRequestsContract, CapstoneRevisionRequestsContractClient, ContractError, RevisionStatus, Role,
};

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    CapstoneRevisionRequestsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let id = env.register(CapstoneRevisionRequestsContract, ());
    let client = CapstoneRevisionRequestsContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let requester = Address::generate(&env);
    let responder = Address::generate(&env);
    client.initialize(&admin);
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    client.set_role(&admin, &requester, &tenant, &project, &Role::Requester, &true);
    client.set_role(&admin, &responder, &tenant, &project, &Role::Responder, &true);
    (env, admin, requester, responder, client)
}

#[test]
fn request_revision_is_idempotent_and_tenant_scoped() {
    let (env, _admin, requester, responder, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let request_id = BytesN::from_array(&env, &[3u8; 32]);
    let req_hash = BytesN::from_array(&env, &[4u8; 32]);
    let r1 = client.request_revision(
        &requester,
        &tenant,
        &project,
        &request_id,
        &1,
        &req_hash,
        &2_000,
        &responder,
        &0,
    );
    let r2 = client.request_revision(
        &requester,
        &tenant,
        &project,
        &request_id,
        &1,
        &req_hash,
        &2_000,
        &responder,
        &0,
    );
    assert_eq!(r1, r2);
    assert_eq!(r1.status, RevisionStatus::Open);
    assert_eq!(r1.expected_version, 1);
    let other = Address::generate(&env);
    let err = client
        .try_request_revision(
            &other,
            &tenant,
            &project,
            &BytesN::from_array(&env, &[5u8; 32]),
            &1,
            &req_hash,
            &2_000,
            &responder,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn response_and_resubmission_enforce_state_deadline_and_versions() {
    let (env, _admin, requester, responder, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let request_id = BytesN::from_array(&env, &[6u8; 32]);
    let req_hash = BytesN::from_array(&env, &[7u8; 32]);
    let resp_hash = BytesN::from_array(&env, &[8u8; 32]);
    client.request_revision(&requester, &tenant, &project, &request_id, &1, &req_hash, &2_000, &responder, &0);
    let responded = client.respond_revision(&responder, &tenant, &project, &request_id, &resp_hash, &1);
    assert_eq!(responded.status, RevisionStatus::Responded);
    let replay = client.respond_revision(&responder, &tenant, &project, &request_id, &resp_hash, &1);
    assert_eq!(replay.expected_version, responded.expected_version);
    let resub = client.resubmit_proposal(&responder, &tenant, &project, &request_id, &2, &2);
    assert_eq!(resub.status, RevisionStatus::Resubmitted);
    let stale = client
        .try_resubmit_proposal(&responder, &tenant, &project, &request_id, &2, &2)
        .unwrap_err()
        .unwrap();
    assert_eq!(stale, ContractError::VersionMismatch);

    let expired_id = BytesN::from_array(&env, &[9u8; 32]);
    client.request_revision(&requester, &tenant, &project, &expired_id, &1, &req_hash, &1_500, &responder, &0);
    env.ledger().set_timestamp(1_501);
    let expired = client
        .try_respond_revision(&responder, &tenant, &project, &expired_id, &resp_hash, &1)
        .unwrap_err()
        .unwrap();
    assert_eq!(expired, ContractError::DeadlineExpired);
}

#[test]
fn close_requires_requester_and_correct_state() {
    let (env, _admin, requester, responder, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let request_id = BytesN::from_array(&env, &[10u8; 32]);
    let req_hash = BytesN::from_array(&env, &[11u8; 32]);
    let resp_hash = BytesN::from_array(&env, &[12u8; 32]);
    client.request_revision(&requester, &tenant, &project, &request_id, &1, &req_hash, &2_000, &responder, &0);
    client.respond_revision(&responder, &tenant, &project, &request_id, &resp_hash, &1);
    let closed = client.close_request(&requester, &tenant, &project, &request_id, &2);
    assert_eq!(closed.status, RevisionStatus::Closed);
    let replay = client.close_request(&requester, &tenant, &project, &request_id, &2);
    assert_eq!(replay.status, RevisionStatus::Closed);
    let bad_actor = client
        .try_close_request(&responder, &tenant, &project, &request_id, &2)
        .unwrap_err()
        .unwrap();
    assert_eq!(bad_actor, ContractError::Unauthorized);
}
