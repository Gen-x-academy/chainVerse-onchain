use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    AuditKind, CapstoneGovernanceExportContract, CapstoneGovernanceExportContractClient,
    ContractError, Role, SnapshotKind,
};

const BASE: u64 = 1_000;
const MAX_PER_WINDOW: u32 = 2;
const WINDOW: u32 = 600;
const GRANT_TTL: u32 = 300;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    registrar: Address,
    participant: Address,
    coordinator: Address,
    outsider: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(CapstoneGovernanceExportContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let registrar = Address::generate(&env);
    let participant = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let outsider = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    c.initialize(&admin, &MAX_PER_WINDOW, &WINDOW, &GRANT_TTL);
    c.set_role(&admin, &registrar, &tenant, &project, &Role::Registrar, &true);
    c.set_role(&admin, &participant, &tenant, &project, &Role::Participant, &true);
    c.set_role(&admin, &coordinator, &tenant, &project, &Role::Coordinator, &true);
    c.register_project(&registrar, &tenant, &project, &hash(&env, 10));
    Fixture {
        env,
        contract_id,
        admin,
        registrar,
        participant,
        coordinator,
        outsider,
        tenant,
        project,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> CapstoneGovernanceExportContractClient<'a> {
    CapstoneGovernanceExportContractClient::new(env, contract_id)
}

#[test]
fn initialize_roles_and_project_registration_boundaries() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_initialize(&f.admin, &MAX_PER_WINDOW, &WINDOW, &GRANT_TTL)
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        c.try_set_role(
            &f.outsider,
            &f.outsider,
            &f.tenant,
            &f.project,
            &Role::Registrar,
            &true
        )
        .unwrap_err()
        .unwrap(),
        ContractError::NotAdmin
    );
    assert_eq!(
        c.try_register_project(
            &f.participant,
            &f.tenant,
            &f.project,
            &hash(&f.env, 20)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::NotRegistrar
    );
    assert_eq!(
        c.try_register_project(
            &f.registrar,
            &f.tenant,
            &f.project,
            &hash(&f.env, 21)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ProjectAlreadyRegistered
    );
    assert_eq!(
        c.try_register_project(
            &f.admin,
            &f.tenant,
            &hash(&f.env, 22),
            &BytesN::from_array(&f.env, &[0u8; 32])
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    let other_tenant = hash(&f.env, 23);
    assert_eq!(
        c.try_request_export(
            &f.participant,
            &other_tenant,
            &f.project,
            &hash(&f.env, 24)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ProjectNotFound
    );
    assert_eq!(
        c.try_request_export(
            &f.outsider,
            &f.tenant,
            &f.project,
            &hash(&f.env, 25)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(c.version(), 1);
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneGovernanceExportContract, ());
    let fresh = client(&env, &contract_id);
    assert_eq!(
        fresh
            .try_initialize(&Address::generate(&env), &0, &WINDOW, &GRANT_TTL)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPolicy
    );
}

#[test]
fn grant_replay_quota_and_window_rollover() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let grant_id = hash(&f.env, 30);
    let grant = c.request_export(&f.participant, &f.tenant, &f.project, &grant_id);
    assert_eq!(grant.requester, f.participant);
    assert_eq!(grant.expires_at, BASE + u64::from(GRANT_TTL));
    let replay = c.request_export(&f.participant, &f.tenant, &f.project, &grant_id);
    assert_eq!(replay, grant);
    assert_eq!(c.get_audit(&f.participant, &f.tenant, &f.project).len(), 1);
    assert_eq!(
        c.try_request_export(
            &f.coordinator,
            &f.tenant,
            &f.project,
            &grant_id
        )
        .unwrap_err()
        .unwrap(),
        ContractError::GrantAlreadyExists
    );
    let other_project = hash(&f.env, 31);
    c.register_project(&f.admin, &f.tenant, &other_project, &hash(&f.env, 32));
    c.set_role(
        &f.admin,
        &f.participant,
        &f.tenant,
        &other_project,
        &Role::Participant,
        &true,
    );
    assert_eq!(
        c.try_request_export(
            &f.participant,
            &f.tenant,
            &other_project,
            &grant_id
        )
        .unwrap_err()
        .unwrap(),
        ContractError::GrantAlreadyExists
    );
    c.request_export(
        &f.participant,
        &f.tenant,
        &f.project,
        &hash(&f.env, 33),
    );
    assert_eq!(
        c.try_request_export(
            &f.participant,
            &f.tenant,
            &f.project,
            &hash(&f.env, 34)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ExportRateLimited
    );
    assert_eq!(c.get_audit(&f.participant, &f.tenant, &f.project).len(), 2);
    let grants = c.get_grant(&f.participant, &grant_id);
    assert_eq!(grants, grant);
    f.env.ledger().set_timestamp(BASE + u64::from(WINDOW));
    c.request_export(
        &f.participant,
        &f.tenant,
        &f.project,
        &hash(&f.env, 35),
    );
}

#[test]
fn download_is_personal_expires_and_audit_is_scoped() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let grant_id = hash(&f.env, 40);
    c.request_export(&f.coordinator, &f.tenant, &f.project, &grant_id);
    assert_eq!(
        c.try_get_export(&f.participant, &grant_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        c.try_get_grant(&f.participant, &grant_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        c.try_get_audit(&f.outsider, &f.tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    let manifest = c.get_export(&f.coordinator, &grant_id);
    assert_eq!(manifest.generated_for, f.coordinator);
    assert_eq!(manifest.tenant_id, f.tenant);
    assert_eq!(manifest.terms_hash, hash(&f.env, 10));
    let audit = c.get_audit(&f.coordinator, &f.tenant, &f.project);
    assert_eq!(audit.len(), 2);
    assert_eq!(audit.get(0).unwrap().kind, AuditKind::Requested);
    assert_eq!(audit.get(1).unwrap().kind, AuditKind::Downloaded);
    f.env
        .ledger()
        .set_timestamp(BASE + u64::from(GRANT_TTL) + 1);
    assert_eq!(
        c.try_get_export(&f.coordinator, &grant_id)
            .unwrap_err()
            .unwrap(),
        ContractError::GrantExpired
    );
    assert_eq!(
        c.try_get_export(&f.coordinator, &hash(&f.env, 41))
            .unwrap_err()
            .unwrap(),
        ContractError::GrantNotFound
    );
}

#[test]
fn snapshots_are_bounded_and_events_are_typed() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let topic = |index: u32| -> Symbol {
        let (_, topics, _) = f.env.events().all().get(index).unwrap();
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap()
    };
    c.record_snapshot(
        &f.registrar,
        &f.tenant,
        &f.project,
        &hash(&f.env, 50),
        &SnapshotKind::AcceptedTerms,
        &hash(&f.env, 51),
    );
    assert_eq!(topic(0), symbol_short!("GVSNAP"));
    c.record_snapshot(
        &f.admin,
        &f.tenant,
        &f.project,
        &hash(&f.env, 52),
        &SnapshotKind::FinancialSummary,
        &hash(&f.env, 53),
    );
    let grant_id = hash(&f.env, 54);
    c.request_export(&f.participant, &f.tenant, &f.project, &grant_id);
    assert_eq!(topic(0), symbol_short!("GVEXREQ"));
    let manifest = c.get_export(&f.participant, &grant_id);
    assert_eq!(manifest.snapshot_count, 2);
    assert_eq!(manifest.snapshots.len(), 2);
    assert_eq!(
        manifest.snapshots.get(0).unwrap().kind,
        SnapshotKind::AcceptedTerms
    );
    assert_eq!(topic(0), symbol_short!("GVEXDL"));
    assert_eq!(
        c.try_record_snapshot(
            &f.registrar,
            &f.tenant,
            &f.project,
            &hash(&f.env, 50),
            &SnapshotKind::Approvals,
            &hash(&f.env, 55)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::SnapshotAlreadyExists
    );
    assert_eq!(
        c.try_record_snapshot(
            &f.registrar,
            &f.tenant,
            &f.project,
            &hash(&f.env, 56),
            &SnapshotKind::Approvals,
            &BytesN::from_array(&f.env, &[0u8; 32])
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    for byte in 60u8..=121u8 {
        c.record_snapshot(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, byte),
            &SnapshotKind::Membership,
            &hash(&f.env, byte ^ 0xFF),
        );
    }
    assert_eq!(c.get_project(&f.tenant, &f.project).snapshot_ids.len(), 64);
    assert_eq!(
        c.try_record_snapshot(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, 124),
            &SnapshotKind::Membership,
            &hash(&f.env, 125)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::SnapshotLimitReached
    );
}
