#![no_std]
mod errors; mod storage; mod types; mod verify;
#[cfg(test)] mod test;
pub use errors::ContractError;
pub use types::Certificate;
use soroban_sdk::{contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    /// Initializes the contract with the admin address and backend public key.
    pub fn init(env: Env, admin: Address, backend_public_key: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_admin(&env).is_some() { return Err(ContractError::AlreadyInitialized); }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_backend_pubkey(&env, &backend_public_key);
        storage::set_paused(&env, false);
        Ok(())
    }
    /// Pauses or unpauses certificate minting. Only callable by admin.
    pub fn toggle_pause(env: Env, caller: Address, paused: bool) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::set_paused(&env, paused);
        env.events().publish((symbol_short!("paused"),), paused);
        Ok(())
    }
    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool { storage::get_paused(&env) }
    /// Mints a certificate to the recipient after verifying the backend proof.
    pub fn mint(env: Env, recipient: Address, course_id: BytesN<32>, proof: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) { return Err(ContractError::ContractPaused); }
        let pubkey = storage::get_backend_pubkey(&env).ok_or(ContractError::NotInitialized)?;
        verify::verify_proof(&env, &recipient, &course_id, &proof, &pubkey)?;
        let cert_key = (recipient.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) { return Err(ContractError::AlreadyMinted); }
        let cert = Certificate { recipient: recipient.clone(), course_id: course_id.clone() };
        storage::save_certificate(&env, cert_key, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (recipient, course_id));
        Ok(())
    }
    /// Revokes a certificate. Only callable by admin.
    pub fn revoke(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::remove_certificate(&env, &(recipient.clone(), course_id.clone()));
        env.events().publish((symbol_short!("CERT_RVK"),), (recipient, course_id));
        Ok(())
    }
    /// Returns a certificate for the given recipient and course, if it exists.
    pub fn get_certificate(env: Env, recipient: Address, course_id: BytesN<32>) -> Option<Certificate> {
        storage::get_certificate(&env, &(recipient, course_id))
    }
}
