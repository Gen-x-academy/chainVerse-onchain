#![no_std]
mod errors; mod storage; mod types; mod verify;
#[cfg(test)] mod test;
pub use errors::ContractError;
pub use types::Certificate;
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Bytes, BytesN, Env, String};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    pub fn init(env: Env, admin: Address, backend_public_key: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_admin(&env).is_some() { return Err(ContractError::AlreadyInitialized); }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_backend_pubkey(&env, &backend_public_key);
        storage::set_paused(&env, false);
        Ok(())
    }

    pub fn toggle_pause(env: Env, caller: Address, paused: bool) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::set_paused(&env, paused);
        env.events().publish((symbol_short!("paused"),), paused);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool { storage::get_paused(&env) }

    /// Mints a soul-bound certificate. Only callable by admin. Fixes #627 (auth) and #628 (persistent token_id).
    pub fn mint_certificate(env: Env, caller: Address, student: Address, course_id: u64, _metadata_uri: String) -> Result<u64, ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if storage::has_certificate(&env, &student, course_id) {
            return Err(ContractError::CertificateExists);
        }
        let token_id = storage::next_token_id(&env);
        let cert = Certificate {
            wallet: student.clone(),
            course_id,
            issued_at: env.ledger().timestamp(),
            soul_bound: true,
        };
        storage::save_certificate(&env, &student, course_id, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (student, course_id, token_id));
        Ok(token_id)
    }

    /// Transfer is blocked for soul-bound certificates. Fixes #629.
    pub fn transfer(env: Env, from: Address, _to: Address, course_id: u64) -> Result<(), ContractError> {
        from.require_auth();
        let cert = storage::load_certificate(&env, &from, course_id)
            .ok_or(ContractError::CertificateNotFound)?;
        if cert.soul_bound {
            return Err(ContractError::SoulboundTransferNotAllowed);
        }
        Ok(())
    }

    pub fn revoke(env: Env, caller: Address, recipient: Address, course_id: u64) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::remove_certificate(&env, &recipient, course_id);
        env.events().publish((symbol_short!("CERT_RVK"),), (recipient, course_id));
        Ok(())
    }

    pub fn get_certificate(env: Env, recipient: Address, course_id: u64) -> Option<Certificate> {
        storage::load_certificate(&env, &recipient, course_id)
    }
}
