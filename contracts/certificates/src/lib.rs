#![no_std]

mod errors;
mod storage;
mod types;
mod verify;

#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::Certificate;

use soroban_sdk::{contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, Env};

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    pub fn init(env: Env, admin: Address) -> Result<(), ContractError> {
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }

        // The deployer must sign to prove they control the intended admin address.
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_paused(&env, false);
        Ok(())
    }

    pub fn toggle_pause(env: Env, caller: Address, paused: bool) -> Result<(), ContractError> {
        storage::require_admin(&env, &caller)?;
        storage::set_paused(&env, paused);
        env.events().publish((symbol_short!("paused"),), paused);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    pub fn mint(
        env: Env,
        wallet: Address,
        course_id: u64,
        backend_public_key: Bytes,
        proof: Bytes,
    ) -> Result<(), ContractError> {
        storage::require_not_paused(&env)?;
        wallet.require_auth();

        if storage::has_certificate(&env, &wallet, course_id) {
            return Err(ContractError::CertificateExists);
        }

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

    pub fn get_certificate(env: Env, wallet: Address, course_id: u64) -> Option<Certificate> {
        storage::load_certificate(&env, &wallet, course_id)
    }

    pub fn has_certificate(env: Env, wallet: Address, course_id: u64) -> bool {
        storage::has_certificate(&env, &wallet, course_id)
    }

    pub fn transfer(
        _env: Env,
        _from: Address,
        _to: Address,
        _course_id: u64,
    ) -> Result<(), ContractError> {
        Err(ContractError::SoulboundTransferNotAllowed)
    }
}
