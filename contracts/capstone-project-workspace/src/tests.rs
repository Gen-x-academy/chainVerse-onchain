extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneProjectWorkspace, CapstoneProjectWorkspaceClient, ContractError, DataKey, Member,
    MemberRole, RequestRecord, Role, RoleGrant, Section, SectionRef, Workspace, WorkspaceState,
};

const NOW: u64 = 3_000_000;
const DAY: u64 = 86_400;

struct Fixture {
    env: Env,
    client: CapstoneProjectWorkspaceClient<'static>,
    admin: Address,
    manager: Address,
    lead: Address,
    contributor: Address,
    observer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn id(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneProjectWorkspace, ());
    let client = CapstoneProjectWorkspaceClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let lead = Address::generate(&env);
    let contributor = Address::generate(&env);
    let observer = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    client.set_role(
        &admin,
        &manager,
        &tenant,
        &project,
        &Role::Manager,
        &true,
        &0,
    );
    client.create_workspace(&manager, &tenant, &project, &id(&env, 10), &id(&env, 100));
    client.add_member(
        &manager,
        &tenant,
        &project,
        &id(&env, 11),
        &lead,
        &MemberRole::Lead,
        &0,
    );
    client.add_member(
        &manager,
        &tenant,
        &project,
        &id(&env, 12),
        &contributor,
        &MemberRole::Contributor,
        &0,
    );
    client.add_member(
        &manager,
        &tenant,
        &project,
        &id(&env, 13),
        &observer,
        &MemberRole::Observer,
        &0,
    );
    Fixture {
        env,
        client,
        admin,
        manager,
        lead,
        contributor,
        observer,
        tenant,
        project,
    }
}

fn home(f: &Fixture) -> Workspace {
    f.client.get_home(&f.manager, &f.tenant, &f.project)
}

fn update(
    f: &Fixture,
    who: &Address,
    request: u8,
    section: Section,
    expected: u32,
) -> Result<u32, ContractError> {
    match f.client.try_update_section(
        who,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &section,
        &expected,
        &id(&f.env, request.wrapping_add(100)),
    ) {
        Ok(Ok(version)) => Ok(version),
        Err(Ok(err)) => Err(err),
        other => panic!("unexpected result: {other:?}"),
    }
}

fn set_state(
    f: &Fixture,
    request: u8,
    expected: u32,
    to: WorkspaceState,
) -> Result<u32, ContractError> {
    match f.client.try_set_state(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &expected,
        &to,
    ) {
        Ok(Ok(version)) => Ok(version),
        Err(Ok(err)) => Err(err),
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn initialization_is_one_time_and_version_is_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}

#[test]
fn create_provisions_one_scoped_home_with_overview() {
    let f = setup();
    let workspace = home(&f);
    assert_eq!(workspace.state, WorkspaceState::Active);
    assert_eq!(workspace.member_count, 3);
    assert_eq!(workspace.created_at, NOW);
    // Creation (1) plus three member additions.
    assert_eq!(workspace.version, 4);
    let overview = f
        .client
        .get_section(&f.manager, &f.tenant, &f.project, &Section::Overview);
    assert_eq!(overview.ref_hash, id(&f.env, 100));
    assert_eq!(overview.version, 1);

    let err = f
        .client
        .try_create_workspace(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 20),
            &id(&f.env, 101),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyExists);
    let err = f
        .client
        .try_create_workspace(
            &f.manager,
            &id(&f.env, 5),
            &f.project,
            &id(&f.env, 21),
            &id(&f.env, 101),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let zero = BytesN::from_array(&f.env, &[0u8; 32]);
    f.client.set_role(
        &f.admin,
        &f.manager,
        &id(&f.env, 5),
        &f.project,
        &Role::Manager,
        &true,
        &0,
    );
    let err = f
        .client
        .try_create_workspace(
            &f.manager,
            &id(&f.env, 5),
            &f.project,
            &id(&f.env, 22),
            &zero,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidInput);
}

#[test]
fn unprovisioned_projects_report_not_found() {
    let f = setup();
    let other_project = id(&f.env, 9);
    f.client.set_role(
        &f.admin,
        &f.manager,
        &f.tenant,
        &other_project,
        &Role::Manager,
        &true,
        &0,
    );
    assert_eq!(
        f.client
            .try_get_home(&f.manager, &f.tenant, &other_project)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    assert_eq!(
        f.client
            .try_add_member(
                &f.manager,
                &f.tenant,
                &other_project,
                &id(&f.env, 30),
                &f.lead,
                &MemberRole::Lead,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
}

#[test]
fn section_authorization_tree_is_deny_by_default() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    let all = [
        Section::Overview,
        Section::Members,
        Section::Objectives,
        Section::Milestones,
        Section::Resources,
        Section::Activity,
        Section::NextActions,
    ];
    let mut request = 40u8;
    let mut allowed = |who: &Address, allow: &[Section], f: &Fixture| {
        for section in all.iter() {
            request += 1;
            let result = update(f, who, request, *section, 0);
            let overview_taken = *section == Section::Overview;
            if allow.contains(section) {
                // Overview already exists at version 1, so version 0 is stale.
                if overview_taken {
                    assert_eq!(result, Err(ContractError::StaleVersion));
                } else {
                    assert!(result.is_ok(), "expected {section:?} to be allowed");
                }
            } else {
                assert_eq!(result, Err(ContractError::Unauthorized), "{section:?}");
            }
        }
    };
    allowed(&f.observer, &[], &f);
    allowed(&stranger, &[], &f);
    allowed(
        &f.contributor,
        &[Section::Resources, Section::Activity, Section::NextActions],
        &f,
    );
    // Leads edit everything except the member roster commitment.
    let lead_sections = [
        Section::Overview,
        Section::Objectives,
        Section::Milestones,
        Section::Resources,
        Section::Activity,
        Section::NextActions,
    ];
    // Resources, Activity and NextActions were taken by the contributor above,
    // so they are stale at version 0 for the lead.
    for section in all.iter() {
        request += 1;
        let result = update(&f, &f.lead, request, *section, 0);
        match section {
            Section::Members => assert_eq!(result, Err(ContractError::Unauthorized)),
            Section::Overview | Section::Resources | Section::Activity | Section::NextActions => {
                assert_eq!(result, Err(ContractError::StaleVersion), "{section:?}")
            }
            _ => assert!(result.is_ok(), "{section:?}"),
        }
        assert!(lead_sections.contains(section) || *section == Section::Members);
    }
    // Managers edit every section, including the roster.
    assert_eq!(update(&f, &f.manager, 90, Section::Members, 0), Ok(1));
}

#[test]
fn reads_require_manager_or_unexpired_membership() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    for who in [&f.manager, &f.lead, &f.contributor, &f.observer] {
        assert!(f.client.try_get_home(who, &f.tenant, &f.project).is_ok());
        assert!(f
            .client
            .try_get_section(who, &f.tenant, &f.project, &Section::Overview)
            .is_ok());
        assert!(f
            .client
            .try_list_members(who, &f.tenant, &f.project, &0, &8)
            .is_ok());
    }
    assert_eq!(
        f.client
            .try_get_home(&stranger, &f.tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_get_member(&stranger, &f.tenant, &f.project, &f.lead)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_list_members(&stranger, &f.tenant, &f.project, &0, &8)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    // Membership management stays with managers and the administrator owns roles.
    for who in [&f.lead, &f.contributor, &f.observer, &stranger] {
        assert_eq!(
            f.client
                .try_add_member(
                    who,
                    &f.tenant,
                    &f.project,
                    &id(&f.env, 50),
                    &stranger,
                    &MemberRole::Observer,
                    &0,
                )
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
        assert_eq!(
            f.client
                .try_remove_member(who, &f.tenant, &f.project, &id(&f.env, 51), &f.observer)
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
        assert_eq!(
            f.client
                .try_set_state(
                    who,
                    &f.tenant,
                    &f.project,
                    &id(&f.env, 52),
                    &4,
                    &WorkspaceState::Paused,
                )
                .unwrap_err()
                .unwrap(),
            ContractError::Unauthorized
        );
    }
    assert_eq!(
        f.client
            .try_set_role(
                &f.manager,
                &stranger,
                &f.tenant,
                &f.project,
                &Role::Manager,
                &true,
                &0
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn tenant_and_project_boundaries_are_enforced() {
    let f = setup();
    let other_tenant = id(&f.env, 70);
    let other_project = id(&f.env, 71);
    // Membership in one workspace grants nothing in another tenant or project.
    assert_eq!(
        f.client
            .try_get_home(&f.lead, &other_tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_get_home(&f.lead, &f.tenant, &other_project)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(update(&f, &f.lead, 60, Section::Objectives, 0), Ok(1));
    f.client.set_role(
        &f.admin,
        &f.manager,
        &other_tenant,
        &f.project,
        &Role::Manager,
        &true,
        &0,
    );
    f.client.create_workspace(
        &f.manager,
        &other_tenant,
        &f.project,
        &id(&f.env, 10),
        &id(&f.env, 100),
    );
    assert_eq!(
        f.client
            .try_get_section(&f.manager, &other_tenant, &f.project, &Section::Objectives)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
}

#[test]
fn membership_cannot_be_duplicated_and_is_bounded() {
    let f = setup();
    let err = f
        .client
        .try_add_member(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 60),
            &f.lead,
            &MemberRole::Observer,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyMember);
    assert_eq!(home(&f).member_count, 3);
    for n in 0u32..61 {
        let account = Address::generate(&f.env);
        let mut request = [0u8; 32];
        request[0] = 7;
        request[31] = n as u8;
        f.client.add_member(
            &f.manager,
            &f.tenant,
            &f.project,
            &BytesN::from_array(&f.env, &request),
            &account,
            &MemberRole::Observer,
            &0,
        );
    }
    assert_eq!(home(&f).member_count, 64);
    let overflow = Address::generate(&f.env);
    let err = f
        .client
        .try_add_member(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 61),
            &overflow,
            &MemberRole::Observer,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::LimitReached);
    let first = f
        .client
        .list_members(&f.observer, &f.tenant, &f.project, &0, &32);
    assert_eq!(first.members.len(), 32);
    assert!(first.has_more);
    let last = f
        .client
        .list_members(&f.observer, &f.tenant, &f.project, &32, &32);
    assert_eq!(last.members.len(), 32);
    assert_eq!(last.next, 64);
    assert!(!last.has_more);
    for (from, limit) in [(0u32, 0u32), (0, 33), (65, 1)] {
        assert_eq!(
            f.client
                .try_list_members(&f.observer, &f.tenant, &f.project, &from, &limit)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidPage
        );
    }
}

#[test]
fn replays_return_recorded_results_without_side_effects() {
    let f = setup();
    let before = home(&f).version;
    let v = f.client.add_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &f.contributor,
        &MemberRole::Contributor,
        &0,
    );
    // Request 12 was the original contributor addition, so this is a replay.
    assert_eq!(v, 3);
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(home(&f).version, before);
    assert_eq!(home(&f).member_count, 3);

    assert_eq!(update(&f, &f.lead, 60, Section::Objectives, 0), Ok(1));
    assert_eq!(update(&f, &f.lead, 60, Section::Objectives, 0), Ok(1));
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(update(&f, &f.lead, 61, Section::Objectives, 1), Ok(2));
    // A replay of the first update still answers with its recorded version.
    assert_eq!(update(&f, &f.lead, 60, Section::Objectives, 0), Ok(1));
    assert_eq!(
        f.client
            .get_section(&f.lead, &f.tenant, &f.project, &Section::Objectives)
            .version,
        2
    );
}

#[test]
fn request_reuse_with_other_payload_or_actor_conflicts() {
    let f = setup();
    assert_eq!(update(&f, &f.lead, 60, Section::Objectives, 0), Ok(1));
    // Same request id, different section.
    assert_eq!(
        update(&f, &f.lead, 60, Section::Milestones, 0),
        Err(ContractError::RequestConflict)
    );
    // Same request id and payload, different editor.
    assert_eq!(
        f.client
            .try_update_section(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 60),
                &Section::Objectives,
                &0,
                &id(&f.env, 160),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    // Reusing an add_member request id for another account conflicts.
    let other = Address::generate(&f.env);
    assert_eq!(
        f.client
            .try_add_member(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &other,
                &MemberRole::Contributor,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
}

#[test]
fn stale_section_versions_are_rejected() {
    let f = setup();
    assert_eq!(update(&f, &f.lead, 60, Section::Milestones, 0), Ok(1));
    // A concurrent writer that still holds version 0 loses.
    assert_eq!(
        update(&f, &f.manager, 61, Section::Milestones, 0),
        Err(ContractError::StaleVersion)
    );
    assert_eq!(update(&f, &f.manager, 61, Section::Milestones, 1), Ok(2));
    let section = f
        .client
        .get_section(&f.lead, &f.tenant, &f.project, &Section::Milestones);
    assert_eq!(section.version, 2);
    assert_eq!(section.ref_hash, id(&f.env, 161));
    assert_eq!(
        f.client
            .try_get_section(&f.lead, &f.tenant, &f.project, &Section::Resources)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    let zero = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(
        f.client
            .try_update_section(
                &f.lead,
                &f.tenant,
                &f.project,
                &id(&f.env, 62),
                &Section::Milestones,
                &2,
                &zero,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
}

#[test]
fn lifecycle_follows_active_paused_archived() {
    let f = setup();
    let v = home(&f).version;
    assert_eq!(set_state(&f, 70, v, WorkspaceState::Paused), Ok(v + 1));
    assert_eq!(home(&f).state, WorkspaceState::Paused);

    // Paused workspaces keep their data readable but reject edits and additions.
    assert_eq!(
        update(&f, &f.lead, 71, Section::Objectives, 0),
        Err(ContractError::InvalidState)
    );
    let newcomer = Address::generate(&f.env);
    assert_eq!(
        f.client
            .try_add_member(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 72),
                &newcomer,
                &MemberRole::Observer,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    // Same-state and stale requests are rejected.
    assert_eq!(
        set_state(&f, 73, v + 1, WorkspaceState::Paused),
        Err(ContractError::InvalidTransition)
    );
    assert_eq!(
        set_state(&f, 74, v, WorkspaceState::Active),
        Err(ContractError::StaleVersion)
    );
    assert_eq!(set_state(&f, 75, v + 1, WorkspaceState::Active), Ok(v + 2));
    assert_eq!(update(&f, &f.lead, 76, Section::Objectives, 0), Ok(1));
    let now = home(&f).version;
    assert_eq!(
        set_state(&f, 77, now, WorkspaceState::Archived),
        Ok(now + 1)
    );

    // Archived is terminal and fully frozen.
    for to in [
        WorkspaceState::Active,
        WorkspaceState::Paused,
        WorkspaceState::Archived,
    ] {
        assert_eq!(
            set_state(&f, 78, now + 1, to),
            Err(ContractError::InvalidTransition)
        );
    }
    assert_eq!(
        update(&f, &f.manager, 79, Section::Overview, 1),
        Err(ContractError::InvalidState)
    );
    assert_eq!(
        f.client
            .try_remove_member(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 80),
                &f.observer
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    assert!(f
        .client
        .try_get_home(&f.observer, &f.tenant, &f.project)
        .is_ok());
}

#[test]
fn removed_members_lose_access_and_can_be_re_added() {
    let f = setup();
    let v = f.client.remove_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 60),
        &f.contributor,
    );
    assert_eq!(v, home(&f).version);
    assert_eq!(home(&f).member_count, 2);
    assert_eq!(
        f.client
            .try_get_home(&f.contributor, &f.tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        update(&f, &f.contributor, 61, Section::Resources, 0),
        Err(ContractError::Unauthorized)
    );
    let page = f
        .client
        .list_members(&f.lead, &f.tenant, &f.project, &0, &32);
    assert_eq!(page.members.len(), 2);
    assert_eq!(
        f.client
            .try_get_member(&f.lead, &f.tenant, &f.project, &f.contributor)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    // Removing again is NotFound, not a silent success.
    assert_eq!(
        f.client
            .try_remove_member(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 62),
                &f.contributor
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    f.client.add_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 63),
        &f.contributor,
        &MemberRole::Lead,
        &0,
    );
    assert_eq!(
        f.client
            .get_member(&f.lead, &f.tenant, &f.project, &f.contributor)
            .role,
        MemberRole::Lead
    );
}

#[test]
fn memberships_and_manager_roles_expire() {
    let f = setup();
    let temp = Address::generate(&f.env);
    f.client.add_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 60),
        &temp,
        &MemberRole::Lead,
        &(NOW + 100),
    );
    assert_eq!(update(&f, &temp, 61, Section::Objectives, 0), Ok(1));
    f.env.ledger().set_timestamp(NOW + 100);
    assert_eq!(
        update(&f, &temp, 62, Section::Objectives, 1),
        Err(ContractError::RoleExpired)
    );
    assert_eq!(
        f.client
            .try_get_home(&temp, &f.tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::RoleExpired
    );
    // Invalid membership terms are rejected.
    let other = Address::generate(&f.env);
    for (request, at) in [(63u8, NOW + 100), (64, NOW + 6 * 365 * DAY)] {
        assert_eq!(
            f.client
                .try_add_member(
                    &f.manager,
                    &f.tenant,
                    &f.project,
                    &id(&f.env, request),
                    &other,
                    &MemberRole::Observer,
                    &at,
                )
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidInput
        );
    }

    // Manager role expiry is reported distinctly and blocks management.
    let temp_manager = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &temp_manager,
        &f.tenant,
        &f.project,
        &Role::Manager,
        &true,
        &(NOW + 500),
    );
    f.env.ledger().set_timestamp(NOW + 500);
    assert_eq!(
        f.client
            .try_set_state(
                &temp_manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 65),
                &1,
                &WorkspaceState::Paused,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RoleExpired
    );
    assert_eq!(
        f.client
            .try_set_role(
                &f.admin,
                &temp_manager,
                &f.tenant,
                &f.project,
                &Role::Manager,
                &true,
                &(NOW + 5)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
}

#[test]
fn events_follow_the_documented_schema_without_private_payloads() {
    let f = setup();
    let topic_of = |f: &Fixture| -> Symbol {
        let events = f.env.events().all();
        assert_eq!(events.len(), 1);
        let (_, topics, _) = events.get(0).unwrap();
        assert_eq!(topics.len(), 1);
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap()
    };
    let newcomer = Address::generate(&f.env);
    f.client.add_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 60),
        &newcomer,
        &MemberRole::Contributor,
        &(NOW + 9),
    );
    assert_eq!(topic_of(&f), symbol_short!("WSP_MEM"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (tenant, project, member, role, expires, version): (
        BytesN<32>,
        BytesN<32>,
        Address,
        u32,
        u64,
        u32,
    ) = data.try_into_val(&f.env).unwrap();
    assert_eq!(tenant, f.tenant);
    assert_eq!(project, f.project);
    assert_eq!(
        (member, role, expires, version),
        (newcomer.clone(), 2, NOW + 9, 5)
    );

    update(&f, &f.lead, 61, Section::Objectives, 0).unwrap();
    assert_eq!(topic_of(&f), symbol_short!("WSP_SEC"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, section, section_version, version): (BytesN<32>, BytesN<32>, u32, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((section, section_version, version), (3, 1, 6));

    set_state(&f, 62, 6, WorkspaceState::Paused).unwrap();
    assert_eq!(topic_of(&f), symbol_short!("WSP_STS"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, from, to, version): (BytesN<32>, BytesN<32>, u32, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((from, to, version), (1, 2, 7));

    f.client.remove_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 63),
        &newcomer,
    );
    assert_eq!(topic_of(&f), symbol_short!("WSP_RMV"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, removed, version): (BytesN<32>, BytesN<32>, Address, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((removed, version), (newcomer, 8));

    let other = id(&f.env, 80);
    f.client.set_role(
        &f.admin,
        &f.manager,
        &f.tenant,
        &other,
        &Role::Manager,
        &true,
        &0,
    );
    f.client.create_workspace(
        &f.manager,
        &f.tenant,
        &other,
        &id(&f.env, 64),
        &id(&f.env, 102),
    );
    assert_eq!(topic_of(&f), symbol_short!("WSP_NEW"));
}

#[test]
fn failed_calls_emit_no_events_and_change_nothing() {
    let f = setup();
    let before = home(&f);
    assert_eq!(
        update(&f, &f.observer, 60, Section::Resources, 0),
        Err(ContractError::Unauthorized)
    );
    assert_eq!(
        set_state(&f, 61, before.version, WorkspaceState::Active),
        Err(ContractError::InvalidTransition)
    );
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(home(&f), before);
}

#[test]
fn error_codes_and_discriminants_are_stable() {
    assert_eq!(ContractError::AlreadyInitialized as u32, 1);
    assert_eq!(ContractError::NotInitialized as u32, 2);
    assert_eq!(ContractError::Unauthorized as u32, 3);
    assert_eq!(ContractError::RequestConflict as u32, 4);
    assert_eq!(ContractError::StaleVersion as u32, 5);
    assert_eq!(ContractError::NotFound as u32, 6);
    assert_eq!(ContractError::AlreadyExists as u32, 7);
    assert_eq!(ContractError::InvalidInput as u32, 8);
    assert_eq!(ContractError::InvalidTransition as u32, 9);
    assert_eq!(ContractError::LimitReached as u32, 10);
    assert_eq!(ContractError::InvalidPage as u32, 11);
    assert_eq!(ContractError::ArithmeticError as u32, 12);
    assert_eq!(ContractError::RoleExpired as u32, 13);
    assert_eq!(ContractError::InvalidState as u32, 14);
    assert_eq!(ContractError::AlreadyMember as u32, 15);
    assert_eq!(Role::Manager as u32, 1);
    assert_eq!(
        [
            MemberRole::Lead as u32,
            MemberRole::Contributor as u32,
            MemberRole::Observer as u32
        ],
        [1, 2, 3]
    );
    assert_eq!(
        [
            WorkspaceState::Active as u32,
            WorkspaceState::Paused as u32,
            WorkspaceState::Archived as u32
        ],
        [1, 2, 3]
    );
    assert_eq!(
        [
            Section::Overview as u32,
            Section::Members as u32,
            Section::Objectives as u32,
            Section::Milestones as u32,
            Section::Resources as u32,
            Section::Activity as u32,
            Section::NextActions as u32
        ],
        [1, 2, 3, 4, 5, 6, 7]
    );
}

#[test]
fn records_written_by_an_earlier_deployment_remain_readable() {
    let f = setup();
    let tenant = id(&f.env, 55);
    let project = id(&f.env, 56);
    let legacy_member = Address::generate(&f.env);
    let legacy_home = Workspace {
        tenant_id: tenant.clone(),
        project_id: project.clone(),
        state: WorkspaceState::Active,
        member_count: 1,
        created_at: 4,
        updated_at: 5,
        version: 9,
    };
    let legacy_section = SectionRef {
        section: Section::Milestones,
        ref_hash: id(&f.env, 57),
        version: 3,
        updated_at: 6,
    };
    f.env.as_contract(&f.client.address, || {
        let storage = f.env.storage().persistent();
        storage.set(
            &DataKey::Workspace(tenant.clone(), project.clone()),
            &legacy_home,
        );
        storage.set(
            &DataKey::Section(tenant.clone(), project.clone(), Section::Milestones),
            &legacy_section,
        );
        storage.set(
            &DataKey::Member(tenant.clone(), project.clone(), legacy_member.clone()),
            &Member {
                account: legacy_member.clone(),
                role: MemberRole::Lead,
                added_at: 4,
                expires_at: 0,
            },
        );
        storage.set(
            &DataKey::Role(
                f.manager.clone(),
                tenant.clone(),
                project.clone(),
                Role::Manager,
            ),
            &RoleGrant {
                active: true,
                expires_at: 0,
            },
        );
        storage.set(
            &DataKey::Request(tenant.clone(), project.clone(), id(&f.env, 90)),
            &RequestRecord {
                input_hash: id(&f.env, 91),
                result: 9,
            },
        );
    });
    assert_eq!(
        f.client.get_home(&f.manager, &tenant, &project),
        legacy_home
    );
    assert_eq!(
        f.client
            .get_section(&f.manager, &tenant, &project, &Section::Milestones),
        legacy_section
    );
    assert_eq!(
        f.client
            .get_member(&f.manager, &tenant, &project, &legacy_member)
            .role,
        MemberRole::Lead
    );
    // The legacy member can edit and the section continues at version 3.
    let v = f.client.update_section(
        &legacy_member,
        &tenant,
        &project,
        &id(&f.env, 92),
        &Section::Milestones,
        &3,
        &id(&f.env, 93),
    );
    assert_eq!(v, 4);
    assert_eq!(f.client.get_home(&f.manager, &tenant, &project).version, 10);
}

#[test]
fn resource_budgets_stay_within_transaction_limits() {
    let f = setup();
    f.env.cost_estimate().budget().reset_default();
    update(&f, &f.lead, 60, Section::Objectives, 0).unwrap();
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    let newcomer = Address::generate(&f.env);
    f.env.cost_estimate().budget().reset_default();
    f.client.add_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 61),
        &newcomer,
        &MemberRole::Observer,
        &0,
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    f.env.cost_estimate().budget().reset_default();
    f.client.remove_member(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 62),
        &newcomer,
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);
}
