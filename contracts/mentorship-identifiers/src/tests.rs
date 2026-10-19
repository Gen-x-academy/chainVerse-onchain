extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, MentorshipIdentifiersContract, MentorshipIdentifiersContractClient,
};

fn setup() -> (Env, Address, Address, MentorshipIdentifiersContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipIdentifiersContract, ());
    let client = MentorshipIdentifiersContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    client.initialize(&issuer);
    (env, contract_id, issuer, client)
}

fn reference_hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

#[test]
fn registration_returns_stable_nonsequential_public_id() {
    let (env, _, issuer, client) = setup();
    let first_ref = reference_hash(&env, 1);
    let second_ref = reference_hash(&env, 2);

    let first = client.register(&issuer, &first_ref);
    let repeated = client.register(&issuer, &first_ref);
    let second = client.register(&issuer, &second_ref);

    assert_eq!(first, repeated);
    assert_ne!(first.public_id, second.public_id);
    let mut sequential_first = [0u8; 32];
    sequential_first[24..].copy_from_slice(&1u64.to_be_bytes());
    let mut sequential_second = [0u8; 32];
    sequential_second[24..].copy_from_slice(&2u64.to_be_bytes());
    assert_ne!(first.public_id.to_array(), sequential_first);
    assert_ne!(second.public_id.to_array(), sequential_second);
    let mut next_after_first = first.public_id.to_array();
    for byte in next_after_first.iter_mut().rev() {
        let (value, overflow) = byte.overflowing_add(1);
        *byte = value;
        if !overflow {
            break;
        }
    }
    assert_ne!(second.public_id.to_array(), next_after_first);
    assert_ne!(first.correlation_id, second.correlation_id);
}

#[test]
fn public_lookup_does_not_need_or_return_internal_reference() {
    let (env, _, issuer, client) = setup();
    let reference = reference_hash(&env, 3);
    let identity = client.register(&issuer, &reference);

    assert_eq!(client.get_by_public_id(&identity.public_id), identity);
    assert_eq!(client.get_by_reference(&reference), identity);
}

#[test]
fn only_issuer_can_create_identifiers() {
    let (env, _, _, client) = setup();
    let other = Address::generate(&env);
    let reference = reference_hash(&env, 4);

    let error = client
        .try_register(&other, &reference)
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotAuthorized);
}

#[test]
fn unknown_identifiers_return_not_found() {
    let (env, _, _, client) = setup();
    let unknown = reference_hash(&env, 5);

    assert_eq!(
        client.try_get_by_reference(&unknown).unwrap_err().unwrap(),
        ContractError::NotFound
    );
    assert_eq!(
        client
            .try_get_by_public_id(&unknown)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
}

#[test]
fn initialization_is_one_time() {
    let (_, _, issuer, client) = setup();

    assert_eq!(
        client.try_initialize(&issuer).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}
