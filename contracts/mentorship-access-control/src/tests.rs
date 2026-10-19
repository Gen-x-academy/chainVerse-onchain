extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, MentorshipAccessControlContract, MentorshipAccessControlContractClient,
    Permission, Role,
};

fn setup() -> (Env, MentorshipAccessControlContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(MentorshipAccessControlContract, ());
    let client = MentorshipAccessControlContractClient::new(&env, &id);
    (env, client)
}

fn scope(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn initialize(env: &Env, client: &MentorshipAccessControlContractClient<'_>) -> Address {
    let admin = Address::generate(env);
    client.initialize(&admin);
    admin
}

#[test]
fn initialize_seeds_admin_and_least_privilege_matrix() {
    let (env, client) = setup();
    let admin = initialize(&env, &client);
    let global = client.global_scope();

    assert!(client.has_role(&admin, &global, &Role::Administrator));
    assert!(client.can(&Role::Student, &Permission::CreateEngagement));
    assert!(client.can(&Role::Guardian, &Permission::ManageGuardianConsent));
    assert!(client.can(&Role::Moderator, &Permission::ReviewSafetyCase));
    assert!(client.can(&Role::FinanceStaff, &Permission::InitiateSettlement));
    assert!(!client.can(&Role::FinanceStaff, &Permission::ApproveSettlement));
    assert!(client.can(&Role::Administrator, &Permission::ApproveSettlement));
    assert!(!client.can(&Role::Administrator, &Permission::ViewEngagement));
    assert!(!client.can(&Role::Student, &Permission::ApproveSettlement));
    assert!(!client.can(&Role::Moderator, &Permission::ViewPaymentStatus));
    assert!(!client.can(&Role::OrganizationStaff, &Permission::ReviewSafetyCase));
}

#[test]
fn authorization_requires_role_permission_and_exact_scope() {
    let (env, client) = setup();
    let admin = initialize(&env, &client);
    let student = Address::generate(&env);
    let engagement = scope(&env, 1);
    let another_engagement = scope(&env, 2);
    client.grant_role(&admin, &student, &engagement, &Role::Student);

    client.authorize(
        &student,
        &engagement,
        &Role::Student,
        &Permission::CreateEngagement,
    );
    let wrong_scope = client
        .try_authorize(
            &student,
            &another_engagement,
            &Role::Student,
            &Permission::CreateEngagement,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(wrong_scope, ContractError::Unauthorized);

    let denied_action = client
        .try_authorize(
            &student,
            &engagement,
            &Role::Student,
            &Permission::ApproveSettlement,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(denied_action, ContractError::PermissionDenied);
}

#[test]
fn only_administrator_can_be_global_and_revocation_is_immediate() {
    let (env, client) = setup();
    let admin = initialize(&env, &client);
    let student = Address::generate(&env);
    let global = client.global_scope();
    let engagement = scope(&env, 1);

    let invalid_global = client
        .try_grant_role(&admin, &student, &global, &Role::Student)
        .unwrap_err()
        .unwrap();
    assert_eq!(invalid_global, ContractError::InvalidScope);

    client.grant_role(&admin, &student, &engagement, &Role::Student);
    client.revoke_role(&admin, &student, &engagement, &Role::Student);
    assert!(!client.has_role(&student, &engagement, &Role::Student));
}

#[test]
fn only_admin_can_change_the_matrix() {
    let (env, client) = setup();
    let admin = initialize(&env, &client);
    let outsider = Address::generate(&env);
    let rejected = client
        .try_set_permission(
            &outsider,
            &Role::Student,
            &Permission::ApproveSettlement,
            &true,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(rejected, ContractError::NotAdmin);

    client.set_permission(
        &admin,
        &Role::Student,
        &Permission::ApproveSettlement,
        &true,
    );
    assert!(client.can(&Role::Student, &Permission::ApproveSettlement));
}