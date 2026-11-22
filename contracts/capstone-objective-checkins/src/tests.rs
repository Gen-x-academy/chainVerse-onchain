use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec,
};

use crate::{
    CapstoneObjectiveCheckinsContract, CapstoneObjectiveCheckinsContractClient, FileCheckinInput,
    CheckinStatus, ContractError, RegisterObjectiveInput, Role,
};

const NOW: u64 = 1_700_000_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    CapstoneObjectiveCheckinsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let id = env.register(CapstoneObjectiveCheckinsContract, ());
    let client = CapstoneObjectiveCheckinsContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let planner = Address::generate(&env);
    let owner = Address::generate(&env);
    let delegate = Address::generate(&env);
    client.initialize(&admin);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.set_role(&admin, &planner, &tenant, &project, &Role::Planner, &true);
    client.set_role(&admin, &owner, &tenant, &project, &Role::Participant, &true);
    client.set_role(
        &admin,
        &delegate,
        &tenant,
        &project,
        &Role::Participant,
        &true,
    );
    (env, admin, planner, owner, delegate, client)
}

fn register_input(
    env: &Env,
    owner: &Address,
    delegate: &Address,
    cadence: u32,
) -> RegisterObjectiveInput {
    RegisterObjectiveInput {
        owner: owner.clone(),
        delegate: Some(delegate.clone()),
        cadence_seconds: cadence,
        spec_hash: hash(env, 0x70),
        expected_version: 0,
    }
}

fn checkin_input(
    env: &Env,
    byte: u8,
    status: CheckinStatus,
    confidence: u32,
    supersedes: Option<BytesN<32>>,
) -> FileCheckinInput {
    FileCheckinInput {
        checkin_id: hash(env, byte),
        status,
        confidence_bps: confidence,
        evidence_hash: hash(env, 0x71),
        report_hash: hash(env, 0x72),
        supersedes,
        expected_version: 0,
    }
}

#[test]
fn roles_tenant_scope_and_owner_delegate_auth() {
    let (env, admin, planner, owner, delegate, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let objective_id = hash(&env, 80);
    let outsider = Address::generate(&env);
    let register = register_input(&env, &owner, &delegate, 60);
    let err = client
        .try_register_objective(&outsider, &tenant, &project, &objective_id, &register)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = client
        .try_register_objective(
            &planner,
            &hash(&env, 0x73),
            &project,
            &objective_id,
            &register,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    client
        .register_objective(&planner, &tenant, &project, &objective_id, &register)
        ;

    let first = checkin_input(&env, 81, CheckinStatus::OnTrack, 9_000, None);
    let err = client
        .try_file_checkin(&outsider, &tenant, &project, &objective_id, &first)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let other_owner = Address::generate(&env);
    client.set_role(
        &admin,
        &other_owner,
        &tenant,
        &project,
        &Role::Participant,
        &true,
    );
    let err = client
        .try_file_checkin(&other_owner, &tenant, &project, &objective_id, &first)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::WrongOwner);
    let checkin = client
        .file_checkin(&delegate, &tenant, &project, &objective_id, &first)
        ;
    assert_eq!(checkin.filer, delegate);
    assert_eq!(client.get_latest(&tenant, &project, &objective_id), checkin);
}

#[test]
fn replay_is_idempotent_and_stale_versions_rejected() {
    let (env, _admin, planner, owner, delegate, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let objective_id = hash(&env, 82);
    let register = register_input(&env, &owner, &delegate, 60);
    let first_objective = client
        .register_objective(&planner, &tenant, &project, &objective_id, &register)
        ;
    let replay_objective = client
        .register_objective(&planner, &tenant, &project, &objective_id, &register)
        ;
    assert_eq!(first_objective.expected_version, replay_objective.expected_version);

    let input = checkin_input(&env, 83, CheckinStatus::AtRisk, 7_500, None);
    let first = client
        .file_checkin(&owner, &tenant, &project, &objective_id, &input)
        ;
    let replay = client
        .file_checkin(&owner, &tenant, &project, &objective_id, &input)
        ;
    assert_eq!(first.recorded_at, replay.recorded_at);

    let mut stale = checkin_input(&env, 84, CheckinStatus::OnTrack, 8_000, None);
    stale.expected_version = 1;
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &stale)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);
    let mut conflict = checkin_input(&env, 83, CheckinStatus::Blocked, 2_000, None);
    conflict.expected_version = 0;
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &conflict)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);
}

#[test]
fn cadence_supersedes_confidence_and_closed_are_enforced() {
    let (env, _admin, planner, owner, delegate, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let objective_id = hash(&env, 85);
    client
        .register_objective(
            &planner,
            &tenant,
            &project,
            &objective_id,
            &register_input(&env, &owner, &delegate, 60),
        )
        ;

    let bad_confidence = checkin_input(&env, 86, CheckinStatus::OnTrack, 10_001, None);
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &bad_confidence)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidConfidence);

    client
        .file_checkin(
            &owner,
            &tenant,
            &project,
            &objective_id,
            &checkin_input(&env, 87, CheckinStatus::OnTrack, 9_000, None),
        )
        ;
    env.ledger().set_timestamp(NOW + 10);
    let early = checkin_input(&env, 88, CheckinStatus::AtRisk, 8_000, Some(hash(&env, 87)));
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &early)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CadenceViolation);

    env.ledger().set_timestamp(NOW + 60);
    let bad_supersedes = checkin_input(&env, 88, CheckinStatus::AtRisk, 8_000, Some(hash(&env, 99)));
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &bad_supersedes)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidSupersedes);
    let second = client
        .file_checkin(
            &delegate,
            &tenant,
            &project,
            &objective_id,
            &checkin_input(&env, 88, CheckinStatus::AtRisk, 8_000, Some(hash(&env, 87))),
        )
        ;
    assert_eq!(second.supersedes, Some(hash(&env, 87)));
    assert_eq!(client.get_checkin_ids(&tenant, &project, &objective_id).len(), 2);

    env.ledger().set_timestamp(NOW + 120);
    client
        .file_checkin(
            &owner,
            &tenant,
            &project,
            &objective_id,
            &checkin_input(&env, 89, CheckinStatus::Closed, 5_000, Some(hash(&env, 88))),
        )
        ;
    env.ledger().set_timestamp(NOW + 180);
    let err = client
        .try_file_checkin(
            &owner,
            &tenant,
            &project,
            &objective_id,
            &checkin_input(&env, 90, CheckinStatus::OnTrack, 9_000, None),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::ObjectiveClosed);
}

#[test]
fn bounded_storage_events_and_version() {
    let (env, _admin, planner, owner, delegate, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let objective_id = hash(&env, 91);
    assert_eq!(client.version(), 1);
    client
        .register_objective(
            &planner,
            &tenant,
            &project,
            &objective_id,
            &register_input(&env, &owner, &delegate, 1),
        )
        ;
    for i in 0..64u8 {
        env.ledger().set_timestamp(NOW + u64::from(i));
        client
            .file_checkin(
                &owner,
                &tenant,
                &project,
                &objective_id,
                &checkin_input(&env, 100 + i, CheckinStatus::OnTrack, 8_000, None),
            )
            ;
    }
    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, _) = event;
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic == symbol_short!("OBC_CHK") {
            found = true;
        }
    }
    assert!(found);
    assert_eq!(client.get_checkin_ids(&tenant, &project, &objective_id).len(), 64);
    env.ledger().set_timestamp(NOW + 64);
    let overflow = checkin_input(&env, 200, CheckinStatus::OnTrack, 8_000, None);
    let err = client
        .try_file_checkin(&owner, &tenant, &project, &objective_id, &overflow)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CheckinLimitReached);
    let err = client
        .try_get_checkin(&tenant, &project, &objective_id, &hash(&env, 0x20))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CheckinNotFound);
    let _ = Vec::<BytesN<32>>::new(&env);
}
