extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, MentorProfilesContract, MentorProfilesContractClient, ProfileStatus,
    ProfileVisibility,
};

fn setup() -> (Env, Address, MentorProfilesContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorProfilesContract, ());
    let client = MentorProfilesContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

fn commitment(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn owner_creates_and_updates_versioned_profile_commitments() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    assert_eq!(
        client.create_profile(
            &owner,
            &commitment(&env, 1),
            &1,
            &ProfileVisibility::Public,
        ),
        1
    );
    assert_eq!(
        client.update_profile(
            &owner,
            &commitment(&env, 2),
            &2,
            &ProfileVisibility::Matched,
        ),
        2
    );

    let current = client.get_profile(&owner);
    assert_eq!(current.version, 2);
    assert_eq!(current.profile_commitment, commitment(&env, 2));
    assert_eq!(current.schema_version, 2);
    assert_eq!(current.visibility, ProfileVisibility::Matched);
    assert_eq!(current.status, ProfileStatus::Active);
    assert_eq!(
        client.get_profile_revision(&owner, &1).profile_commitment,
        commitment(&env, 1)
    );
}

#[test]
fn only_the_profile_owner_can_mutate_it() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.create_profile(
        &owner,
        &commitment(&env, 1),
        &1,
        &ProfileVisibility::Private,
    );
    env.mock_auths(&[]);

    assert!(
        client
            .try_update_profile(
            &stranger,
            &commitment(&env, 2),
            &1,
            &ProfileVisibility::Public,
            )
            .is_err(),
        "an unsigned profile update must be rejected"
    );
    assert!(
        client
            .try_set_visibility(&stranger, &ProfileVisibility::Public)
            .is_err(),
        "an unsigned visibility change must be rejected"
    );
    assert_eq!(client.get_profile(&owner).version, 1);
}

#[test]
fn visibility_and_status_changes_create_auditable_revisions() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    client.create_profile(
        &owner,
        &commitment(&env, 3),
        &1,
        &ProfileVisibility::Private,
    );
    assert_eq!(
        client.set_visibility(&owner, &ProfileVisibility::Public),
        2
    );
    assert_eq!(client.set_status(&owner, &ProfileStatus::Deactivated), 3);
    let current = client.get_profile(&owner);
    assert_eq!(current.visibility, ProfileVisibility::Public);
    assert_eq!(current.status, ProfileStatus::Deactivated);
    assert_eq!(
        client.get_profile_revision(&owner, &2).visibility,
        ProfileVisibility::Public
    );
    assert_eq!(
        client.get_profile_revision(&owner, &3).status,
        ProfileStatus::Deactivated
    );
    assert_eq!(client.set_status(&owner, &ProfileStatus::Deactivated), 3);
}

#[test]
fn invalid_commitments_and_duplicate_creation_are_rejected() {
    let (env, _, client) = setup();
    let owner = Address::generate(&env);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    assert_eq!(
        client.try_create_profile(
            &owner,
            &zero,
            &1,
            &ProfileVisibility::Private,
        ),
        Err(Ok(ContractError::InvalidCommitment))
    );
    assert_eq!(
        client.try_create_profile(
            &owner,
            &commitment(&env, 4),
            &0,
            &ProfileVisibility::Private,
        ),
        Err(Ok(ContractError::InvalidSchemaVersion))
    );
    client.create_profile(
        &owner,
        &commitment(&env, 4),
        &1,
        &ProfileVisibility::Private,
    );
    assert_eq!(
        client.try_create_profile(
            &owner,
            &commitment(&env, 5),
            &1,
            &ProfileVisibility::Public,
        ),
        Err(Ok(ContractError::AlreadyExists))
    );
}