#![no_std]

mod errors;
mod storage;
mod types;
mod verify;

#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::Certificate;

use soroban_sdk::{contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    /// Initializes the certificates contract with the admin and backend proof key.
    pub fn init(env: Env, admin: Address, backend_public_key: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }

        // The deployer must sign to prove they control the intended admin address.
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_backend_pubkey(&env, &backend_public_key);
        storage::set_paused(&env, false);
        Ok(())
    }

    /// Updates the paused state for certificate minting and admin actions.
    pub fn toggle_pause(env: Env, caller: Address, paused: bool) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::set_paused(&env, paused);
        env.events().publish((symbol_short!("paused"),), paused);
        Ok(())
    }

    /// Returns whether the certificates contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::is_paused(&env)
    }

    /// Mints a soulbound certificate for a wallet after validating the backend proof.
    pub fn mint(
        env: Env,
        wallet: Address,
        course_id: u64,
        proof: Bytes,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_not_paused(&env)?;
        wallet.require_auth();

        if storage::has_certificate(&env, &wallet, course_id) {
            return Err(ContractError::CertificateExists);
        }

        let backend_public_key =
            storage::get_backend_pubkey(&env).ok_or(ContractError::NotInitialized)?;

        let payload = (wallet.clone(), course_id).to_xdr(&env);
        verify::verify_backend_proof(&env, &backend_public_key, &payload, &proof)?;

        let certificate = Certificate {
            wallet: wallet.clone(),
            course_id,
            issued_at: env.ledger().timestamp(),
        };

        storage::save_certificate(&env, &wallet, course_id, &certificate);
        env.events()
            .publish((symbol_short!("cert_mint"), wallet), course_id);

        Ok(())
    }

    /// Revokes an existing certificate for a wallet and course.
    pub fn revoke_certificate(
        env: Env,
        caller: Address,
        wallet: Address,
        course_id: u64,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;

        if !storage::has_certificate(&env, &wallet, course_id) {
            return Err(ContractError::CertificateNotFound);
        }

        storage::remove_certificate(&env, &wallet, course_id);
        env.events()
            .publish((symbol_short!("cert_rvk"), wallet), course_id);

        Ok(())
    }

    /// Loads the certificate issued to a wallet for a course, if one exists.
    pub fn get_certificate(env: Env, wallet: Address, course_id: u64) -> Option<Certificate> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::load_certificate(&env, &wallet, course_id)
    }

    /// Returns whether a wallet already has a certificate for a course.
    pub fn has_certificate(env: Env, wallet: Address, course_id: u64) -> bool {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::has_certificate(&env, &wallet, course_id)
    }

    /// Rejects certificate transfers because certificates are soulbound.
    pub fn transfer(
        env: Env,
        _from: Address,
        _to: Address,
        _course_id: u64,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        Err(ContractError::SoulboundTransferNotAllowed)
    }

    /// Upgrades the current contract to the provided WASM hash after admin authorization.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Returns the contract version for post-deploy verification.
    pub fn version(_env: Env) -> u32 {
        1
    }
}
