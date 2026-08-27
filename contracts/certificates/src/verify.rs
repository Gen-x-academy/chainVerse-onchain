use ed25519_dalek::{Signature, VerifyingKey};
use soroban_sdk::{xdr::ToXdr, Address, Bytes, BytesN, Env};

use crate::ContractError;

const MAX_PAYLOAD_LEN: usize = 256;

fn to_fixed_bytes<const N: usize>(value: &Bytes) -> Result<[u8; N], ContractError> {
    if value.len() != N as u32 {
        return Err(ContractError::InvalidProof);
    }

    let mut raw = [0u8; N];
    value.copy_into_slice(&mut raw);
    Ok(raw)
}

pub fn verify_backend_proof(
    env: &Env,
    backend_public_key: &Bytes,
    recipient: &Address,
    course_id: &BytesN<32>,
    expires_at: u64,
    nonce: &BytesN<32>,
    proof: &Bytes,
) -> Result<(), ContractError> {
    let public_key = to_fixed_bytes::<32>(backend_public_key)?;
    let signature = to_fixed_bytes::<64>(proof)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| ContractError::InvalidProof)?;
    let signature = Signature::from_bytes(&signature);
    let payload = (recipient.clone(), course_id.clone(), expires_at, nonce.clone()).to_xdr(env);
    let payload_len = payload.len() as usize;
    if payload_len > MAX_PAYLOAD_LEN {
        return Err(ContractError::InvalidProof);
    }

    let mut message = [0u8; MAX_PAYLOAD_LEN];
    payload.copy_into_slice(&mut message[..payload_len]);

    verifying_key
        .verify_strict(&message[..payload_len], &signature)
        .map_err(|_| ContractError::InvalidProof)?;

    Ok(())
}
