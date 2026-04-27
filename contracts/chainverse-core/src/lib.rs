#![no_std]

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

pub mod admin;
pub mod analytics;
pub mod escrow;
pub mod utils;

mod errors;
mod events;
mod storage;

#[cfg(test)]
mod test;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use errors::ContractError;
pub use escrow::{paginate, EscrowRecord, EscrowStatus};
pub use storage::Config;

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

use analytics::{
    EVT_ADMIN_CHANGED, EVT_CONFIG_UPDATED, EVT_ESCROW_CANCELLED, EVT_ESCROW_CREATED,
    EVT_ESCROW_RELEASED,
};
use storage::DataKey;

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

#[contract]
pub struct ChainverseCore;

#[contractimpl]
impl ChainverseCore {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// One-time initialization. Sets admin, protocol fee, and supported tokens.
    pub fn initialize(
        env: Env,
        admin: Address,
        protocol_fee: u32,
        supported_tokens: Vec<Address>,
    ) -> Result<(), ContractError> {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Config) {
            return Err(ContractError::AlreadyInitialized);
        }

        let config = storage::Config {
            admin,
            protocol_fee,
            supported_tokens,
        };

        env.storage().persistent().set(&DataKey::Config, &config);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin module — all privileged functions go through only_admin()
    // -----------------------------------------------------------------------

    /// Returns `true` when the contract is paused.
    pub fn is_paused(env: Env) -> bool {
        admin::is_paused(&env)
    }

    /// Admin-only: pause the contract.
    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        admin::pause(&env, &caller)
    }

    /// Admin-only: unpause the contract.
    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        admin::unpause(&env, &caller)
    }

    // -----------------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------------

    /// Returns the global configuration.
    pub fn get_config(env: Env) -> Result<Config, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Config)
            .ok_or(ContractError::NotInitialized)
    }

    /// Admin-only: update protocol fee and/or supported token list.
    pub fn update_config(
        env: Env,
        caller: Address,
        new_protocol_fee: Option<u32>,
        new_supported_tokens: Option<Vec<Address>>,
    ) -> Result<(), ContractError> {
        admin::only_admin(&env, &caller)?;

        let mut config: storage::Config = env
            .storage()
            .persistent()
            .get(&DataKey::Config)
            .ok_or(ContractError::NotInitialized)?;

        if let Some(fee) = new_protocol_fee {
            config.protocol_fee = fee;
        }
        if let Some(tokens) = new_supported_tokens {
            config.supported_tokens = tokens;
        }

        env.storage().persistent().set(&DataKey::Config, &config);
        analytics::record(&env, EVT_CONFIG_UPDATED);
        Ok(())
    }

    /// Admin-only: transfer admin rights to a new address.
    pub fn transfer_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        admin::only_admin(&env, &caller)?;

        let mut config: storage::Config = env
            .storage()
            .persistent()
            .get(&DataKey::Config)
            .ok_or(ContractError::NotInitialized)?;

        let old_admin = config.admin.clone();
        config.admin = new_admin.clone();

        env.storage().persistent().set(&DataKey::Config, &config);
        analytics::record(&env, EVT_ADMIN_CHANGED);
        env.events().publish((symbol_short!("ADM_CHNG"),), (old_admin, new_admin));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Escrow module
    // -----------------------------------------------------------------------

    /// Creates a new escrow and returns its id.
    /// `expires_at` is a Unix timestamp after which the escrow can be refunded; pass 0 for no expiry.
    pub fn create_escrow(
        env: Env,
        depositor: Address,
        recipient: Address,
        token: Address,
        amount: i128,
        expires_at: u64,
    ) -> Result<u64, ContractError> {
        admin::assert_not_paused(&env)?;
        let id = escrow::create(&env, depositor, recipient, token, amount, expires_at)?;
        analytics::record(&env, EVT_ESCROW_CREATED);
        Ok(id)
    }

    /// Releases a pending escrow to the recipient.
    pub fn release_escrow(env: Env, caller: Address, id: u64) -> Result<(), ContractError> {
        admin::assert_not_paused(&env)?;
        escrow::release(&env, caller, id)?;
        analytics::record(&env, EVT_ESCROW_RELEASED);
        Ok(())
    }

    /// Cancels a pending escrow, returning funds to the depositor.
    pub fn cancel_escrow(env: Env, caller: Address, id: u64) -> Result<(), ContractError> {
        admin::assert_not_paused(&env)?;
        escrow::cancel(&env, caller, id)?;
        analytics::record(&env, EVT_ESCROW_CANCELLED);
        Ok(())
    }

    /// Buyer cancels a Pending escrow before the seller has interacted.
    pub fn buyer_cancel_escrow(env: Env, buyer: Address, id: u64) -> Result<(), ContractError> {
        admin::assert_not_paused(&env)?;
        escrow::buyer_cancel(&env, buyer, id)?;
        analytics::record(&env, EVT_ESCROW_CANCELLED);
        Ok(())
    }

    /// Refunds an escrow that has passed its expiry timestamp.
    pub fn refund_expired_escrow(env: Env, id: u64) -> Result<(), ContractError> {
        admin::assert_not_paused(&env)?;
        escrow::refund_expired(&env, id)?;
        analytics::record(&env, EVT_ESCROW_CANCELLED);
        Ok(())
    }

    /// Returns the escrow record for `id`.
    pub fn get_escrow(env: Env, id: u64) -> Result<EscrowRecord, ContractError> {
        escrow::get(&env, id)
    }

    /// Returns all escrows where `buyer` is the depositor.
    pub fn get_escrows_by_buyer(env: Env, buyer: Address) -> Vec<EscrowRecord> {
        escrow::get_by_buyer(&env, &buyer)
    }

    /// Returns all escrows where `seller` is the recipient.
    pub fn get_escrows_by_seller(env: Env, seller: Address) -> Vec<EscrowRecord> {
        escrow::get_by_seller(&env, &seller)
    }

    // -----------------------------------------------------------------------
    // Analytics module
    // -----------------------------------------------------------------------

    /// Returns the total number of times `event` has been recorded.
    pub fn event_count(env: Env, event: soroban_sdk::Symbol) -> u64 {
        analytics::count(&env, event)
    }

    /// Returns high-level statistics about the contract's escrows.
    pub fn get_escrow_stats(env: Env) -> analytics::Stats {
        analytics::get_stats(&env)
    }

    /// Search escrows by token and/or status.
    pub fn search_escrows(
        env: Env,
        token: Option<Address>,
        status: Option<EscrowStatus>,
    ) -> Vec<EscrowRecord> {
        escrow::search(&env, token, status)
    }

    /// Returns all escrows currently in the Pending (active) state.
    pub fn get_active_escrows(env: Env) -> Vec<EscrowRecord> {
        escrow::get_active_escrows(&env)
    }

    // -----------------------------------------------------------------------
    // Utils module
    // -----------------------------------------------------------------------

    /// Returns `true` when `token` is in the supported-token list.
    pub fn is_token_supported(env: Env, token: Address) -> bool {
        utils::is_token_supported(&env, &token)
    }

    /// Calculates the protocol fee for a given amount.
    pub fn calculate_fee(env: Env, amount: i128) -> Result<i128, ContractError> {
        utils::calculate_fee(&env, amount)
    }
}
