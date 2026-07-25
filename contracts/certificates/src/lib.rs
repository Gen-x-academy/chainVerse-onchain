#![no_std]
mod errors; mod storage; mod types; mod verify;
#[cfg(test)] mod test;
pub use errors::ContractError;
pub use types::Certificate;
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Bytes, BytesN, Env};
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

    /// Mints a certificate to the recipient after verifying the backend proof.
    pub fn mint(env: Env, recipient: Address, course_id: BytesN<32>, proof: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) { return Err(ContractError::ContractPaused); }
        let pubkey = storage::get_backend_pubkey(&env).ok_or(ContractError::NotInitialized)?;
        verify::verify_backend_proof(&env, &pubkey, &course_id.clone().into(), &proof)?;
        let cert_key = (recipient.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) { return Err(ContractError::CertificateExists); }
        // Fix #628: token_id from persistent storage (survives contract upgrades)
        let token_id = storage::next_token_id(&env);
        let cert = Certificate { recipient: recipient.clone(), course_id: course_id.clone(), token_id, soul_bound: true };
        storage::save_certificate(&env, cert_key, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (recipient, course_id, token_id));
        Ok(())
    }

    /// Fix #627: Admin-only mint_certificate — only the stored admin can call this.
    pub fn mint_certificate(env: Env, student: Address, course_id: BytesN<32>, metadata_uri: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) { return Err(ContractError::ContractPaused); }
        let admin: Address = storage::get_admin(&env).ok_or(ContractError::NotInitialized)?;
        admin.require_auth();
        let cert_key = (student.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) { return Err(ContractError::CertificateExists); }
        // Fix #628: token_id from persistent storage (survives contract upgrades)
        let token_id = storage::next_token_id(&env);
        let cert = Certificate { recipient: student.clone(), course_id: course_id.clone(), token_id, soul_bound: true };
        storage::save_certificate(&env, cert_key, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (student, course_id, token_id, metadata_uri));
        Ok(())
    }

    /// Fix #629: Transfer is blocked for soul-bound certificates.
    pub fn transfer(env: Env, from: Address, to: Address, course_id: BytesN<32>) -> Result<(), ContractError> {
        let cert_key = (from.clone(), course_id.clone());
        let cert = storage::get_certificate(&env, &cert_key)
            .ok_or(ContractError::CertificateNotFound)?;
        if cert.soul_bound {
            return Err(ContractError::SoulboundTransferNotAllowed);
        }
        from.require_auth();
        storage::remove_certificate(&env, &from, &course_id);
        let new_cert = Certificate { recipient: to.clone(), ..cert };
        storage::save_certificate(&env, (to.clone(), course_id.clone()), &new_cert);
        env.events().publish((symbol_short!("CERT_TRF"),), (from, to, course_id));
        Ok(())
    }

    /// Revokes a certificate. Only callable by admin.
    pub fn revoke(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::remove_certificate(&env, &recipient, &course_id);
        env.events().publish((symbol_short!("CERT_RVK"),), (recipient, course_id));
        Ok(())
    }

    pub fn get_certificate(env: Env, recipient: Address, course_id: u64) -> Option<Certificate> {
        storage::load_certificate(&env, &recipient, course_id)
    }
}
