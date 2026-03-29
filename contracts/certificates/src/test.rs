use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, Env};

use crate::{CertificateContract, CertificateContractClient, ContractError};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn public_key_bytes(env: &Env, signing_key: &SigningKey) -> Bytes {
    Bytes::from_slice(env, &signing_key.verifying_key().to_bytes())
}

fn proof_bytes(env: &Env, signing_key: &SigningKey, wallet: &Address, course_id: u64) -> Bytes {
    let payload = (wallet.clone(), course_id).to_xdr(env);
    let signature = signing_key.sign(&payload.to_alloc_vec());
    Bytes::from_slice(env, &signature.to_bytes())
}

#[test]
fn test_transfer_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let signer = signing_key();

    client.init(&admin);
    client.mint(
        &user1,
        &1,
        &public_key_bytes(&env, &signer),
        &proof_bytes(&env, &signer, &user1, 1),
    );

    let result = client.try_transfer(&user1, &user2, &1);
    assert_eq!(result, Err(Ok(ContractError::SoulboundTransferNotAllowed)));
}

#[test]
fn duplicate_mint_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();

    client.init(&admin);
    client.mint(
        &wallet,
        &7,
        &public_key_bytes(&env, &signer),
        &proof_bytes(&env, &signer, &wallet, 7),
    );

    let second_attempt = client.try_mint(
        &wallet,
        &7,
        &public_key_bytes(&env, &signer),
        &proof_bytes(&env, &signer, &wallet, 7),
    );

    assert_eq!(second_attempt, Err(Ok(ContractError::CertificateExists)));
    assert!(client.has_certificate(&wallet, &7));
}
