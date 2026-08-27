#![no_std]
mod errors;
mod storage;
mod types;
mod verify;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::Certificate;

use soroban_sdk::{contract, contractclient, contractimpl, symbol_short, Address, Bytes, BytesN, Env};
use storage::{MAX_TTL, MIN_TTL};

#[contractclient(name = "CertificateContractClient")]
pub trait CertificateInterface {
    fn init(env: Env, admin: Address, backend_public_key: Bytes, minter: Address) -> Result<(), ContractError>;
    fn mint(env: Env, recipient: Address, course_id: BytesN<32>, proof: Bytes) -> Result<(), ContractError>;
    fn mint_certificate(env: Env, student: Address, course_id: BytesN<32>, metadata_uri: Bytes) -> Result<(), ContractError>;
    fn transfer(env: Env, from: Address, to: Address, course_id: BytesN<32>) -> Result<(), ContractError>;
    fn revoke(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>) -> Result<(), ContractError>;
    fn revoke_with_reason(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>, reason: Bytes) -> Result<(), ContractError>;
    fn get_certificate(env: Env, recipient: Address, course_id: BytesN<32>) -> Option<Certificate>;
    fn toggle_pause(env: Env, caller: Address, paused: bool) -> Result<(), ContractError>;
    fn is_paused(env: Env) -> bool;
    fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), ContractError>;
    fn propose_admin_transfer(env: Env, caller: Address, new_admin: Address) -> Result<(), ContractError>;
    fn accept_admin_transfer(env: Env) -> Result<(), ContractError>;
    fn cancel_admin_transfer(env: Env, caller: Address) -> Result<(), ContractError>;
}

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    pub fn init(env: Env, admin: Address, backend_public_key: Bytes, minter: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_admin(&env).is_some() { return Err(ContractError::AlreadyInitialized); }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_backend_pubkey(&env, &backend_public_key);
        storage::set_minter(&env, &minter);
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

    pub fn mint(env: Env, recipient: Address, course_id: BytesN<32>, proof: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) { return Err(ContractError::ContractPaused); }
        let pubkey = storage::get_backend_pubkey(&env).ok_or(ContractError::NotInitialized)?;
        verify::verify_backend_proof(&env, &pubkey, &course_id.clone().into(), &proof)?;
        let cert_key = (recipient.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) { return Err(ContractError::CertificateExists); }
        let token_id = storage::next_token_id(&env);
        let cert = Certificate { recipient: recipient.clone(), course_id: course_id.clone(), token_id, soul_bound: true };
        storage::save_certificate(&env, cert_key, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (recipient, course_id, token_id));
        Ok(())
    }

    pub fn mint_certificate(env: Env, student: Address, course_id: BytesN<32>, metadata_uri: Bytes) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) { return Err(ContractError::ContractPaused); }
        let minter: Address = storage::get_minter(&env).ok_or(ContractError::NotInitialized)?;
        minter.require_auth();
        let cert_key = (student.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) { return Err(ContractError::CertificateExists); }
        let token_id = storage::next_token_id(&env);
        let cert = Certificate { recipient: student.clone(), course_id: course_id.clone(), token_id, soul_bound: true };
        storage::save_certificate(&env, cert_key, &cert);
        env.events().publish((symbol_short!("CERT_MNT"),), (student, course_id, token_id, metadata_uri));
        Ok(())
    }

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

    pub fn revoke(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>) -> Result<(), ContractError> {
        // Fix #842: backward-compatible entrypoint — revokes without an explicit reason.
        self::revoke_impl(&env, &caller, &recipient, &course_id, Bytes::new(&env))
    }

    pub fn revoke_with_reason(env: Env, caller: Address, recipient: Address, course_id: BytesN<32>, reason: Bytes) -> Result<(), ContractError> {
        self::revoke_impl(&env, &caller, &recipient, &course_id, reason)
    }

    pub fn get_certificate(env: Env, recipient: Address, course_id: BytesN<32>) -> Option<Certificate> {
        storage::get_certificate(&env, &(recipient, course_id))
    }

    pub fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        storage::require_admin(&env, &caller)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }

    /// Fix #841: current admin nominates `new_admin` as pending admin for a
    /// bounded window during which the nominee may accept the role.
    pub fn propose_admin_transfer(env: Env, caller: Address, new_admin: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if new_admin == caller {
            return Err(ContractError::Unauthorized);
        }
        let expiry = env.ledger().timestamp().saturating_add(storage::ADMIN_TRANSFER_TTL);
        storage::set_pending_admin(&env, &new_admin, expiry);
        env.events().publish((symbol_short!("ADMIN_PROP"),), (caller, new_admin, expiry));
        Ok(())
    }

    /// Fix #841: only the nominated pending admin may accept, and only before expiry.
    pub fn accept_admin_transfer(env: Env) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        let pending: Address = storage::get_pending_admin(&env).ok_or(ContractError::NoPendingTransfer)?;
        pending.require_auth();
        let expiry: u64 = storage::get_pending_admin_expiry(&env).unwrap_or(0);
        if env.ledger().timestamp() > expiry {
            storage::clear_pending_admin(&env);
            return Err(ContractError::PendingAdminExpired);
        }
        storage::set_admin(&env, &pending);
        storage::clear_pending_admin(&env);
        env.events().publish((symbol_short!("ADMIN_ACPT"),), pending);
        Ok(())
    }

    /// Fix #841: current admin may cancel a pending proposal.
    pub fn cancel_admin_transfer(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if storage::get_pending_admin(&env).is_none() {
            return Err(ContractError::NoPendingTransfer);
        }
        storage::clear_pending_admin(&env);
        env.events().publish((symbol_short!("ADMIN_CANCEL"),), caller);
        Ok(())
    }
}

fn revoke_impl(env: &Env, caller: &Address, recipient: &Address, course_id: &BytesN<32>, reason: Bytes) -> Result<(), ContractError> {
    env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
    storage::require_admin(env, caller)?;
    let cert_key = (recipient.clone(), course_id.clone());
    let token_id = storage::get_certificate(env, &cert_key).map(|c| c.token_id).unwrap_or(0);
    storage::remove_certificate(env, recipient, course_id);
    // Fix #842: stable revocation event carrying actor, reason, token id,
    // recipient, course and timestamp.
    env.events().publish(
        (symbol_short!("CERT_RVK"),),
        (caller.clone(), reason, token_id, recipient.clone(), course_id.clone(), env.ledger().timestamp()),
    );
    Ok(())
}
