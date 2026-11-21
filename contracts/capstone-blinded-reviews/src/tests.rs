extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    BlindMode, BlindState, CapstoneBlindedReviewsContract, CapstoneBlindedReviewsContractClient,
    ContractError, Role, Visibility,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneBlindedReviewsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(3_000);
    let contract_id = env.register(CapstoneBlindedReviewsContract, ());
    let client = CapstoneBlindedReviewsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin);
    client.grant_role(
        &admin,
        &tenant,
        &project,
        &coordinator,
        &Role::BlindCoordinator,
    );
    (env, admin, coordinator, tenant, project, client)
}

fn configure(client: &CapstoneBlindedReviewsContractClient<'_>, coordinator: &Address, env: &Env) {
    client.configure(
        coordinator,
        &hash(env, 1),
        &hash(env, 2),
        &hash(env, 3),
        &hash(env, 4),
        &0,
        &BlindMode::DoubleBlind,
        &hash(env, 5),
        &hash(env, 6),
    );
}

#[test]
fn initialization_is_one_time_and_unauthorized_cannot_configure() {
    let (env, admin, coordinator, tenant, project, client) = setup();
    let outsider = Address::generate(&env);

    assert!(client.has_role(&coordinator, &tenant, &project, &Role::BlindCoordinator));
    assert!(!client.has_role(&outsider, &tenant, &project, &Role::BlindCoordinator));
    assert_eq!(
        client.try_initialize(&admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        client
            .try_configure(
                &outsider,
                &tenant,
                &project,
                &hash(&env, 3),
                &hash(&env, 4),
                &0,
                &BlindMode::DoubleBlind,
                &hash(&env, 5),
                &hash(&env, 6),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(client.projection(&tenant, &project), Visibility::Hidden);
}

#[test]
fn configure_is_idempotent_and_updates_are_versioned() {
    let (env, _, coordinator, tenant, project, client) = setup();
    let first = client.configure(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 10),
        &hash(&env, 11),
        &0,
        &BlindMode::SingleBlind,
        &hash(&env, 12),
        &hash(&env, 13),
    );
    let replay = client.configure(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 10),
        &hash(&env, 11),
        &0,
        &BlindMode::SingleBlind,
        &hash(&env, 12),
        &hash(&env, 13),
    );
    assert_eq!(first, 1);
    assert_eq!(replay, 1);
    assert_eq!(
        client
            .try_configure(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 10),
                &hash(&env, 99),
                &0,
                &BlindMode::SingleBlind,
                &hash(&env, 12),
                &hash(&env, 13),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(
        client
            .try_configure(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 14),
                &hash(&env, 15),
                &2,
                &BlindMode::DoubleBlind,
                &hash(&env, 12),
                &hash(&env, 13),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    let second = client.configure(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 16),
        &hash(&env, 17),
        &1,
        &BlindMode::DoubleBlind,
        &hash(&env, 12),
        &hash(&env, 13),
    );
    assert_eq!(second, 2);
    assert_eq!(
        client.get_config(&tenant, &project).unwrap().version,
        2
    );
}

#[test]
fn seal_and_reveal_follow_lifecycle_and_change_projection() {
    let (env, admin, coordinator, tenant, project, client) = setup();
    configure(&client, &coordinator, &env);
    assert_eq!(client.projection(&tenant, &project), Visibility::Hidden);

    assert_eq!(
        client
            .try_seal(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 20),
                &hash(&env, 21),
                &1,
                &hash(&env, 22),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::MissingPrerequisite
    );

    client.register_assignment(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 23),
        &hash(&env, 24),
        &1,
        &hash(&env, 25),
        &hash(&env, 26),
        &hash(&env, 27),
    );
    let replay = client.register_assignment(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 23),
        &hash(&env, 24),
        &1,
        &hash(&env, 25),
        &hash(&env, 26),
        &hash(&env, 27),
    );
    assert_eq!(replay, 2);
    assert_eq!(
        client
            .try_register_assignment(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 28),
                &hash(&env, 29),
                &2,
                &hash(&env, 25),
                &hash(&env, 26),
                &hash(&env, 27),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateAssignment
    );
    client.register_assignment(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 30),
        &hash(&env, 31),
        &2,
        &hash(&env, 32),
        &hash(&env, 33),
        &hash(&env, 34),
    );
    assert_eq!(client.get_assignments(&tenant, &project).len(), 2);
    let assignment = client
        .get_assignment(&tenant, &project, &hash(&env, 25))
        .unwrap();
    assert_eq!(assignment.blinded_ref, hash(&env, 25));

    env.ledger().set_timestamp(4_500);
    let sealed = client.seal(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 35),
        &hash(&env, 36),
        &3,
        &hash(&env, 37),
    );
    assert_eq!(sealed, 4);
    let config = client.get_config(&tenant, &project).unwrap();
    assert_eq!(config.state, BlindState::Sealed);
    assert_eq!(config.sealed_at, 4_500);
    assert_eq!(client.projection(&tenant, &project), Visibility::Hidden);

    assert_eq!(
        client
            .try_register_assignment(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 38),
                &hash(&env, 39),
                &4,
                &hash(&env, 40),
                &hash(&env, 41),
                &hash(&env, 42),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
    assert_eq!(
        client
            .try_reveal(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 43),
                &hash(&env, 44),
                &4,
                &hash(&env, 45),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );

    env.ledger().set_timestamp(5_000);
    let revealed = client.reveal(
        &admin,
        &tenant,
        &project,
        &hash(&env, 46),
        &hash(&env, 47),
        &4,
        &hash(&env, 48),
    );
    assert_eq!(revealed, 5);
    let config = client.get_config(&tenant, &project).unwrap();
    assert_eq!(config.state, BlindState::Revealed);
    assert_eq!(config.revealed_at, 5_000);
    assert_eq!(
        client.projection(&tenant, &project),
        Visibility::Pseudonymous
    );
}

#[test]
fn assignments_are_bounded_and_zero_commitments_are_rejected() {
    let (env, _, coordinator, tenant, project, client) = setup();
    client.configure(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 3),
        &hash(&env, 4),
        &0,
        &BlindMode::Open,
        &hash(&env, 5),
        &hash(&env, 6),
    );
    assert_eq!(client.projection(&tenant, &project), Visibility::Visible);

    assert_eq!(
        client
            .try_register_assignment(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 50),
                &hash(&env, 51),
                &1,
                &BytesN::from_array(&env, &[0u8; 32]),
                &hash(&env, 52),
                &hash(&env, 53),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidCommitment
    );

    let mut version = 1u32;
    let mut i = 1u8;
    while i <= 64 {
        let next = client.register_assignment(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 60 + i),
            &hash(&env, 130 + i),
            &version,
            &hash(&env, 100 + i),
            &hash(&env, 5),
            &hash(&env, 6),
        );
        version = next;
        i += 1;
    }
    assert_eq!(client.get_assignments(&tenant, &project).len(), 64);
    assert_eq!(
        client
            .try_register_assignment(
                &coordinator,
                &tenant,
                &project,
                &hash(&env, 240),
                &hash(&env, 241),
                &version,
                &hash(&env, 242),
                &hash(&env, 5),
                &hash(&env, 6),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}
