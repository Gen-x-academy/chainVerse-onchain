use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{
    CapstoneMilestoneDeliverablesContract, CapstoneMilestoneDeliverablesContractClient,
    ContractError, DeliverableKind, Role,
};

fn setup() -> (
    Env,
    Address,
    Address,
    CapstoneMilestoneDeliverablesContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(100);
    let id = env.register(CapstoneMilestoneDeliverablesContract, ());
    let client = CapstoneMilestoneDeliverablesContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let submitter = Address::generate(&env);
    client.initialize(&admin);
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    client.set_role(&admin, &submitter, &tenant, &project, &Role::Submitter, &true);
    (env, admin, submitter, client)
}

#[test]
fn submit_is_versioned_and_idempotent() {
    let (env, _admin, submitter, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let milestone = BytesN::from_array(&env, &[3u8; 32]);
    let deliverable = BytesN::from_array(&env, &[4u8; 32]);
    let content = BytesN::from_array(&env, &[5u8; 32]);
    let req = BytesN::from_array(&env, &[6u8; 32]);
    let first = client.submit_deliverable(
        &submitter,
        &tenant,
        &project,
        &milestone,
        &deliverable,
        &1,
        &DeliverableKind::File,
        &content,
        &req,
        &0,
    );
    let replay = client.submit_deliverable(
        &submitter,
        &tenant,
        &project,
        &milestone,
        &deliverable,
        &1,
        &DeliverableKind::File,
        &content,
        &req,
        &0,
    );
    assert_eq!(first, replay);
    assert_eq!(client.latest_version(&tenant, &project, &milestone), 1);
    let stale = client
        .try_submit_deliverable(
            &submitter,
            &tenant,
            &project,
            &milestone,
            &BytesN::from_array(&env, &[7u8; 32]),
            &1,
            &DeliverableKind::Link,
            &content,
            &req,
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(stale, ContractError::VersionMismatch);
}

#[test]
fn submit_requires_role_and_records_immutable_versions() {
    let (env, _admin, submitter, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let milestone = BytesN::from_array(&env, &[3u8; 32]);
    let d1 = BytesN::from_array(&env, &[8u8; 32]);
    let d2 = BytesN::from_array(&env, &[9u8; 32]);
    let content = BytesN::from_array(&env, &[10u8; 32]);
    let req = BytesN::from_array(&env, &[11u8; 32]);
    client.submit_deliverable(&submitter, &tenant, &project, &milestone, &d1, &1, &DeliverableKind::Dataset, &content, &req, &0);
    client.submit_deliverable(&submitter, &tenant, &project, &milestone, &d2, &2, &DeliverableKind::Commit, &content, &req, &1);
    let got1 = client.get_deliverable(&tenant, &project, &milestone, &d1).unwrap();
    let got2 = client.get_deliverable(&tenant, &project, &milestone, &d2).unwrap();
    assert_eq!(got1.version, 1);
    assert_eq!(got2.version, 2);
    let intruder = Address::generate(&env);
    let err = client
        .try_submit_deliverable(&intruder, &tenant, &project, &milestone, &BytesN::from_array(&env, &[12u8; 32]), &3, &DeliverableKind::Text, &content, &req, &2)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}
