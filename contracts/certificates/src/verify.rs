use soroban_sdk::{Env, Bytes, Address};
use crate::shared::errors::ContractError;

pub fn verify_backend_signature(
    env: &Env,
    backend_public_key: &Bytes,
    payload: &Bytes,
    signature: &Bytes,
) -> Result<(), ContractError> {
    if env.crypto().ed25519_verify(
        backend_public_key,
        payload,
        signature,
    ) {
        Ok(())
    } else {
        Err(ContractError::InvalidSignature)
    }
}