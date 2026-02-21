use soroban_sdk::{Env, Address, Bytes};
use crate::shared::errors::ContractError;
use crate::shared::events::emit_certificate_minted;
use crate::shared::types::Certificate;
use crate::certificate::storage::{get_certificate, store_certificate};
use crate::certificate::verify::verify_backend_signature;

pub fn mint_certificate(
    env: Env,
    wallet: Address,
    course_id: u64,
    backend_public_key: Bytes,
    signature: Bytes,
) -> Result<(), ContractError> {

    wallet.require_auth();

    // Prevent duplicate mint
    if get_certificate(&env, &wallet, course_id).is_some() {
        return Err(ContractError::CertificateExists);
    }

    // Construct payload: wallet + course_id
    let mut payload = Bytes::new(&env);
    payload.append(&wallet.serialize(&env));
    payload.append(&course_id.to_be_bytes());

    // Verify backend signature
    verify_backend_signature(
        &env,
        &backend_public_key,
        &payload,
        &signature,
    )?;

    // Store certificate
    let cert = Certificate {
        wallet: wallet.clone(),
        course_id,
        issued_at: env.ledger().timestamp(),
    };

    store_certificate(&env, &wallet, course_id, &cert);

    // Emit event
    emit_certificate_minted(
        &env,
        wallet,
        course_id,
        env.ledger().timestamp(),
    );

    Ok(())
}