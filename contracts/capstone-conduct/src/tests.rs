use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec as SdkVec,
};

use crate::{
    Audience, BreachState, CapstoneConductContract, CapstoneConductContractClient, ContractError,
    PolicyState, Role, Severity,
};

const BASE: u64 = 1_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
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
    let contract_id = env.register(CapstoneConductContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let participant = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let outsider = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    c.initialize(&admin);
    c.set_role(&admin, &participant, &tenant, &project, &Role::Participant, &true);
    c.set_role(&admin, &coordinator, &tenant, &project, &Role::Coordinator, &true);
    c.publish_policy(
        &admin,
        &tenant,
        &project,
        &hash(&env, 10),
        &BASE,
        &true,
        &Audience::Both,
    );
    Fixture {
        env,
        contract_id,
        admin,
        participant,
        coordinator,
        outsider,
        tenant,
        project,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> CapstoneConductContractClient<'a> {
    CapstoneConductContractClient::new(env, contract_id)
}

#[test]
fn admin_and_tenant_roles_are_enforced() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        c.try_publish_policy(
            &f.outsider,
            &f.tenant,
            &f.project,
            &hash(&f.env, 20),
            &BASE,
            &true,
            &Audience::Both
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    let other_tenant = hash(&f.env, 21);
    assert_eq!(
        c.try_accept_policy(
            &f.participant,
            &other_tenant,
            &f.project,
            &1,
            &Role::Participant
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    assert_eq!(
        c.try_accept_policy(
            &f.outsider,
            &f.tenant,
            &f.project,
            &1,
            &Role::Participant
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    assert_eq!(
        c.try_publish_policy(
            &f.admin,
            &f.tenant,
            &f.project,
            &BytesN::from_array(&f.env, &[0u8; 32]),
            &BASE,
            &true,
            &Audience::Both
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    assert_eq!(c.version(), 1);
}

#[test]
fn mandatory_policy_invalidation_does_not_mutate_history() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.requires_acknowledgement(
            &f.participant,
            &f.tenant,
            &f.project,
            &Role::Participant
        ),
        SdkVec::from_array(&f.env, [1u32])
    );
    assert!(!c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
    let first = c.accept_policy(
        &f.participant,
        &f.tenant,
        &f.project,
        &1,
        &Role::Participant,
    );
    assert!(c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
    c.publish_policy(
        &f.admin,
        &f.tenant,
        &f.project,
        &hash(&f.env, 11),
        &BASE,
        &true,
        &Audience::Both,
    );
    assert_eq!(
        c.requires_acknowledgement(
            &f.participant,
            &f.tenant,
            &f.project,
            &Role::Participant
        ),
        SdkVec::from_array(&f.env, [2u32])
    );
    assert!(!c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
    let stored = c.get_acceptance(&f.participant, &f.tenant, &f.project, &1);
    assert_eq!(stored, first);
    assert_eq!(stored.policy_hash, hash(&f.env, 10));
    c.accept_policy(
        &f.participant,
        &f.tenant,
        &f.project,
        &2,
        &Role::Participant,
    );
    assert!(c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
    let retired = c.retire_policy(&f.admin, &f.tenant, &f.project, &2);
    assert_eq!(retired.state, PolicyState::Retired);
    assert_eq!(
        c.retire_policy(&f.admin, &f.tenant, &f.project, &2)
            .state,
        PolicyState::Retired
    );
}

#[test]
fn audience_applicability_and_effective_time_are_checked() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.retire_policy(&f.admin, &f.tenant, &f.project, &1);
    c.publish_policy(
        &f.admin,
        &f.tenant,
        &f.project,
        &hash(&f.env, 30),
        &(BASE + 500),
        &true,
        &Audience::Participant,
    );
    assert_eq!(
        c.requires_acknowledgement(
            &f.coordinator,
            &f.tenant,
            &f.project,
            &Role::Coordinator
        )
        .len(),
        0
    );
    assert_eq!(
        c.requires_acknowledgement(
            &f.participant,
            &f.tenant,
            &f.project,
            &Role::Participant
        )
        .len(),
        0
    );
    assert_eq!(
        c.try_accept_policy(
            &f.participant,
            &f.tenant,
            &f.project,
            &2,
            &Role::Participant
        )
        .unwrap_err()
        .unwrap(),
        ContractError::EffectiveAtNotReached
    );
    c.publish_policy(
        &f.admin,
        &f.tenant,
        &f.project,
        &hash(&f.env, 31),
        &BASE,
        &true,
        &Audience::Coordinator,
    );
    assert_eq!(
        c.try_accept_policy(
            &f.participant,
            &f.tenant,
            &f.project,
            &3,
            &Role::Participant
        )
        .unwrap_err()
        .unwrap(),
        ContractError::NotApplicable
    );
    assert_eq!(
        c.try_accept_policy(
            &f.coordinator,
            &f.tenant,
            &f.project,
            &1,
            &Role::Coordinator
        )
        .unwrap_err()
        .unwrap(),
        ContractError::PolicyRetired
    );
    assert_eq!(
        c.try_accept_policy(
            &f.participant,
            &f.tenant,
            &f.project,
            &99,
            &Role::Participant
        )
        .unwrap_err()
        .unwrap(),
        ContractError::PolicyNotFound
    );
}

#[test]
fn breach_records_gate_compliance_and_resolve() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.accept_policy(
        &f.participant,
        &f.tenant,
        &f.project,
        &1,
        &Role::Participant,
    );
    let case_ref = hash(&f.env, 40);
    let breach = c.record_breach(
        &f.admin,
        &f.tenant,
        &f.project,
        &case_ref,
        &f.participant,
        &1,
        &Severity::High,
        &hash(&f.env, 41),
    );
    assert_eq!(breach.state, BreachState::Open);
    assert!(!c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
    c.accept_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &1,
        &Role::Coordinator,
    );
    assert!(c.is_compliant(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &Role::Coordinator
    ));
    assert_eq!(
        c.try_record_breach(
            &f.admin,
            &f.tenant,
            &f.project,
            &case_ref,
            &f.participant,
            &1,
            &Severity::Medium,
            &hash(&f.env, 42)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::BreachExists
    );
    assert_eq!(
        c.try_record_breach(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, 43),
            &f.outsider,
            &1,
            &Severity::Medium,
            &hash(&f.env, 44)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    assert_eq!(
        c.try_record_breach(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, 45),
            &f.participant,
            &99,
            &Severity::Low,
            &hash(&f.env, 46)
        )
        .unwrap_err()
        .unwrap(),
        ContractError::PolicyNotFound
    );
    assert_eq!(
        c.try_record_breach(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, 47),
            &f.participant,
            &1,
            &Severity::Low,
            &BytesN::from_array(&f.env, &[0u8; 32])
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    let resolved = c.resolve_breach(&f.admin, &f.tenant, &f.project, &case_ref);
    assert_eq!(resolved.state, BreachState::Resolved);
    assert_eq!(
        c.get_breach(&f.tenant, &f.project, &case_ref)
            .state,
        BreachState::Resolved
    );
    assert!(c.is_compliant(
        &f.participant,
        &f.tenant,
        &f.project,
        &Role::Participant
    ));
}

#[test]
fn policy_bounds_events_and_version() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let topic = |index: u32| -> Symbol {
        let (_, topics, _) = f.env.events().all().get(index).unwrap();
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap()
    };
    assert_eq!(topic(0), symbol_short!("POLPUB"));
    assert_eq!(c.get_policy(&f.tenant, &f.project, &1).version, 1);
    let last = c.accept_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &1,
        &Role::Coordinator,
    );
    assert_eq!(last.role, Role::Coordinator);
    assert_eq!(topic(0), symbol_short!("POLACC"));
    for version in 2u32..=32u32 {
        c.publish_policy(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, version as u8),
            &BASE,
            &false,
            &Audience::Both,
        );
    }
    assert_eq!(
        c.try_publish_policy(
            &f.admin,
            &f.tenant,
            &f.project,
            &hash(&f.env, 60),
            &BASE,
            &false,
            &Audience::Both
        )
        .unwrap_err()
        .unwrap(),
        ContractError::PolicyLimitReached
    );
    assert_eq!(c.version(), 1);
}
