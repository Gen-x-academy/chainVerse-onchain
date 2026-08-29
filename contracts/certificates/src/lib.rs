#![no_std]
mod errors;
mod storage;
mod types;
mod verify;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::Certificate;

use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, Bytes, BytesN, Env,
};
use storage::{MAX_TTL, MIN_TTL, ROTATION_PROPOSAL_TTL};

#[contract]
pub struct CertificateContract;

#[contractimpl]
impl CertificateContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the contract.
    ///
    /// Fix #834: `backend_public_key` must be exactly 32 bytes (Ed25519 key
    /// length); init rejects it with `InvalidPublicKey` before writing any
    /// state.
    pub fn init(
        env: Env,
        admin: Address,
        backend_public_key: Bytes,
        minter: Address,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        // Fix #834: reject keys that are not exactly 32 bytes.
        let pubkey_fixed = storage::validate_pubkey(&env, &backend_public_key)?;
        storage::set_admin(&env, &admin);
        storage::set_backend_pubkey(&env, &pubkey_fixed);
        storage::set_minter(&env, &minter);
        storage::set_paused(&env, false);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Pause
    // -----------------------------------------------------------------------

    pub fn toggle_pause(
        env: Env,
        caller: Address,
        paused: bool,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        storage::set_paused(&env, paused);
        env.events().publish((symbol_short!("paused"),), paused);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        storage::get_paused(&env)
    }

    // -----------------------------------------------------------------------
    // Mint (Ed25519 proof path — Fix #833)
    // -----------------------------------------------------------------------

    /// Mint a certificate after verifying the backend proof.
    ///
    /// Fix #833: `verify_backend_proof` binds the proof to the contract address
    /// and network ID, preventing cross-environment replay attacks.
    pub fn mint(
        env: Env,
        recipient: Address,
        course_id: BytesN<32>,
        nonce: BytesN<32>,
        expires_at: u64,
        proof: Bytes,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        if env.ledger().timestamp() >= expires_at {
            return Err(ContractError::ProofExpired);
        }
        if storage::nonce_consumed(&env, &nonce) {
            return Err(ContractError::NonceAlreadyConsumed);
        }
        let pubkey = storage::get_backend_pubkey(&env).ok_or(ContractError::NotInitialized)?;
        verify::verify_backend_proof(
            &env,
            &pubkey,
            &recipient,
            &course_id,
            &nonce,
            expires_at,
            &proof,
        )?;
        let cert_key = (recipient.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) {
            return Err(ContractError::CertificateExists);
        }
        storage::consume_nonce(&env, &nonce);
        let token_id = storage::next_token_id(&env);
        let cert = Certificate {
            recipient: recipient.clone(),
            course_id: course_id.clone(),
            token_id,
            soul_bound: true,
        };
        storage::save_certificate(&env, cert_key, &cert);
        env.events()
            .publish((symbol_short!("CERT_MNT"),), (recipient, course_id, token_id));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Mint (minter-authorised path)
    // -----------------------------------------------------------------------

    pub fn mint_certificate(
        env: Env,
        student: Address,
        course_id: BytesN<32>,
        metadata_uri: Bytes,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::get_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        let minter: Address =
            storage::get_minter(&env).ok_or(ContractError::NotInitialized)?;
        minter.require_auth();
        let cert_key = (student.clone(), course_id.clone());
        if storage::certificate_exists(&env, &cert_key) {
            return Err(ContractError::CertificateExists);
        }
        let token_id = storage::next_token_id(&env);
        let cert = Certificate {
            recipient: student.clone(),
            course_id: course_id.clone(),
            token_id,
            soul_bound: true,
        };
        storage::save_certificate(&env, cert_key, &cert);
        env.events()
            .publish((symbol_short!("CERT_MNT"),), (student, course_id, token_id, metadata_uri));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Transfer / revoke
    // -----------------------------------------------------------------------

    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        course_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let cert_key = (from.clone(), course_id.clone());
        let cert =
            storage::get_certificate(&env, &cert_key).ok_or(ContractError::CertificateNotFound)?;
        if cert.soul_bound {
            return Err(ContractError::SoulboundTransferNotAllowed);
        }
        from.require_auth();
        storage::remove_certificate(&env, &from, &course_id);
        let new_cert = Certificate { recipient: to.clone(), ..cert };
        storage::save_certificate(&env, (to.clone(), course_id.clone()), &new_cert);
        env.events()
            .publish((symbol_short!("CERT_TRF"),), (from, to, course_id));
        Ok(())
    }

    /// Backward-compatible revoke without a reason.
    pub fn revoke(
        env: Env,
        caller: Address,
        recipient: Address,
        course_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        revoke_impl(&env, &caller, &recipient, &course_id, Bytes::new(&env))
    }

    pub fn revoke_with_reason(
        env: Env,
        caller: Address,
        recipient: Address,
        course_id: BytesN<32>,
        reason: Bytes,
    ) -> Result<(), ContractError> {
        revoke_impl(&env, &caller, &recipient, &course_id, reason)
    }

    pub fn get_certificate(
        env: Env,
        recipient: Address,
        course_id: BytesN<32>,
    ) -> Option<Certificate> {
        storage::get_certificate(&env, &(recipient, course_id))
    }

    // -----------------------------------------------------------------------
    // Upgrade
    // -----------------------------------------------------------------------

    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &caller)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin transfer (Fix #841)
    // -----------------------------------------------------------------------

    pub fn propose_admin_transfer(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if new_admin == caller {
            return Err(ContractError::Unauthorized);
        }
        let expiry = env
            .ledger()
            .timestamp()
            .saturating_add(storage::ADMIN_TRANSFER_TTL);
        storage::set_pending_admin(&env, &new_admin, expiry);
        env.events()
            .publish((symbol_short!("ADM_PROP"),), (caller, new_admin, expiry));
        Ok(())
    }

    pub fn accept_admin_transfer(env: Env) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        let pending: Address =
            storage::get_pending_admin(&env).ok_or(ContractError::NoPendingTransfer)?;
        pending.require_auth();
        let expiry: u64 = storage::get_pending_admin_expiry(&env).unwrap_or(0);
        if env.ledger().timestamp() > expiry {
            storage::clear_pending_admin(&env);
            return Err(ContractError::PendingAdminExpired);
        }
        storage::set_admin(&env, &pending);
        storage::clear_pending_admin(&env);
        env.events().publish((symbol_short!("ADM_ACPT"),), pending);
        Ok(())
    }

    pub fn cancel_admin_transfer(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if storage::get_pending_admin(&env).is_none() {
            return Err(ContractError::NoPendingTransfer);
        }
        storage::clear_pending_admin(&env);
        env.events().publish((symbol_short!("ADM_CNCL"),), caller);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Fix #835: backend signing-key rotation (two-step propose → activate)
    // -----------------------------------------------------------------------

    /// Admin proposes a new backend signing key.
    ///
    /// A proposal is valid for `ROTATION_PROPOSAL_TTL` seconds.  The current
    /// key remains active until `activate_key_rotation` is called.
    pub fn propose_key_rotation(
        env: Env,
        caller: Address,
        new_pubkey: Bytes,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        // Validate the new key before storing anything.
        let pubkey_fixed = storage::validate_pubkey(&env, &new_pubkey)?;
        let expiry = env
            .ledger()
            .timestamp()
            .saturating_add(ROTATION_PROPOSAL_TTL);
        storage::set_pending_backend_pubkey(&env, &pubkey_fixed, expiry);
        env.events()
            .publish((symbol_short!("KEY_PROP"),), (caller, expiry));
        Ok(())
    }

    /// Admin activates a previously proposed backend key rotation.
    ///
    /// The prior key is immediately replaced; all subsequent `mint` calls use
    /// the new key for proof verification.
    pub fn activate_key_rotation(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        let new_key = storage::get_pending_backend_pubkey(&env)
            .ok_or(ContractError::NoPendingKeyRotation)?;
        let expiry = storage::get_pending_backend_pubkey_expiry(&env).unwrap_or(0);
        if env.ledger().timestamp() > expiry {
            storage::clear_pending_backend_pubkey(&env);
            return Err(ContractError::PendingKeyRotationExpired);
        }
        storage::set_backend_pubkey(&env, &new_key);
        storage::clear_pending_backend_pubkey(&env);
        env.events().publish((symbol_short!("KEY_ACTV"),), caller);
        Ok(())
    }

    /// Admin cancels an outstanding key rotation proposal.
    pub fn cancel_key_rotation(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if storage::get_pending_backend_pubkey(&env).is_none() {
            return Err(ContractError::NoPendingKeyRotation);
        }
        storage::clear_pending_backend_pubkey(&env);
        env.events().publish((symbol_short!("KEY_CNCL"),), caller);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Fix #836: minter rotation (two-step propose → activate)
    // -----------------------------------------------------------------------

    /// Admin proposes a new minter address.
    ///
    /// The current minter remains authorised until `activate_minter_rotation`
    /// is called or the proposal is cancelled.
    pub fn propose_minter_rotation(
        env: Env,
        caller: Address,
        new_minter: Address,
    ) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        let expiry = env
            .ledger()
            .timestamp()
            .saturating_add(ROTATION_PROPOSAL_TTL);
        storage::set_pending_minter(&env, &new_minter, expiry);
        env.events()
            .publish((symbol_short!("MNT_PROP"),), (caller, new_minter, expiry));
        Ok(())
    }

    /// Admin activates the pending minter rotation.
    ///
    /// The prior minter immediately loses authorisation on this call.
    pub fn activate_minter_rotation(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        let new_minter = storage::get_pending_minter(&env)
            .ok_or(ContractError::NoPendingMinterRotation)?;
        let expiry = storage::get_pending_minter_expiry(&env).unwrap_or(0);
        if env.ledger().timestamp() > expiry {
            storage::clear_pending_minter(&env);
            return Err(ContractError::PendingMinterRotationExpired);
        }
        storage::set_minter(&env, &new_minter);
        storage::clear_pending_minter(&env);
        env.events()
            .publish((symbol_short!("MNT_ACTV"),), (caller, new_minter));
        Ok(())
    }

    /// Admin cancels an outstanding minter rotation proposal.
    pub fn cancel_minter_rotation(env: Env, caller: Address) -> Result<(), ContractError> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        storage::require_admin(&env, &caller)?;
        if storage::get_pending_minter(&env).is_none() {
            return Err(ContractError::NoPendingMinterRotation);
        }
        storage::clear_pending_minter(&env);
        env.events().publish((symbol_short!("MNT_CNCL"),), caller);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn revoke_impl(
    env: &Env,
    caller: &Address,
    recipient: &Address,
    course_id: &BytesN<32>,
    reason: Bytes,
) -> Result<(), ContractError> {
    env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
    storage::require_admin(env, caller)?;
    let cert_key = (recipient.clone(), course_id.clone());
    let token_id = storage::get_certificate(env, &cert_key)
        .map(|c| c.token_id)
        .unwrap_or(0);
    storage::remove_certificate(env, recipient, course_id);
    env.events().publish(
        (symbol_short!("CERT_RVK"),),
        (
            caller.clone(),
            reason,
            token_id,
            recipient.clone(),
            course_id.clone(),
            env.ledger().timestamp(),
        ),
    );
    Ok(())
}
