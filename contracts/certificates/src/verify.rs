use ed25519_dalek::{Signature, VerifyingKey};
use soroban_sdk::{Bytes, Env};

use crate::ContractError;

const MAX_PAYLOAD_LEN: usize = 128;

fn to_fixed_bytes<const N: usize>(value: &Bytes) -> Result<[u8; N], ContractError> {
    if value.len() != N as u32 {
        return Err(ContractError::InvalidProof);
    }

    let mut raw = [0u8; N];
    value.copy_into_slice(&mut raw);
    Ok(raw)
}

pub fn verify_backend_proof(
    _env: &Env,
    backend_public_key: &Bytes,
    payload: &Bytes,
    proof: &Bytes,
) -> Result<(), ContractError> {
    let public_key = to_fixed_bytes::<32>(backend_public_key)?;
    let signature = to_fixed_bytes::<64>(proof)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| ContractError::InvalidProof)?;
    let signature = Signature::from_bytes(&signature);
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
