use soroban_sdk::{Bytes, BytesN, Env};

use crate::ContractError;

fn to_fixed_bytes<const N: usize>(env: &Env, value: &Bytes) -> Result<BytesN<N>, ContractError> {
    if value.len() != N as u32 {
        return Err(ContractError::InvalidProof);
    }

    let mut raw = [0u8; N];
    value.copy_into_slice(&mut raw);
    Ok(BytesN::from_array(env, &raw))
}

pub fn verify_backend_proof(
    env: &Env,
    backend_public_key: &Bytes,
    payload: &Bytes,
    proof: &Bytes,
) -> Result<(), ContractError> {
    let public_key = to_fixed_bytes::<32>(env, backend_public_key)?;
    let signature = to_fixed_bytes::<64>(env, proof)?;

    // Host verification traps on a tampered signature, which rejects the call
    // and rolls back state changes.
    env.crypto()
        .ed25519_verify(&public_key, payload, &signature);
    Ok(())
}
