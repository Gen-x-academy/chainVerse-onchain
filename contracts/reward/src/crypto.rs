use soroban_sdk::{Bytes, BytesN, Env};
use crate::errors::Error;
use crate::storage::get_backend_pubkey;

pub fn verify_signature(
    env: &Env,
    payload: Bytes,
    signature: BytesN<64>,
) -> Result<(), Error> {
    let pubkey = get_backend_pubkey(env).ok_or(Error::Unauthorized)?;

    env.crypto()
        .ed25519_verify(&pubkey, &payload, &signature)
        .map_err(|_| Error::InvalidSignature)?;

    Ok(())
}
