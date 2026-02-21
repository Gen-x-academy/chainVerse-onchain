use soroban_sdk::{Env, Bytes, BytesN};
use crate::storage::DataKey;
use crate::errors::Error;

pub fn verify_signature(
    env: &Env,
    payload: Bytes,
    signature: BytesN<64>,
) -> Result<(), Error> {
    let pubkey: BytesN<32> = env
        .storage()
        .instance()
        .get(&DataKey::BackendPubKey)
        .ok_or(Error::Unauthorized)?;

    env.crypto()
        .ed25519_verify(&pubkey, &payload, &signature)
        .map_err(|_| Error::InvalidSignature)?;

    Ok(())
}