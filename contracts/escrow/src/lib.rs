#![no_std]

mod create;
mod errors;
mod events;
mod refund;
mod release;
mod storage;
mod types;
mod version;

pub use errors::EscrowError;
pub use types::{Escrow, EscrowStatus, FeeRecord};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Sets or rotates the escrow admin. If no admin is set, the new admin must
    /// authorize. If an admin exists, the current admin must authorize.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), EscrowError> {
        let old_admin = storage::get_admin(&env);
        if let Some(current_admin) = old_admin.clone() {
            current_admin.require_auth();
        } else {
            admin.require_auth();
        }
        storage::set_admin(&env, &admin);
        env.events().publish((soroban_sdk::symbol_short!("ADM_CHNG"),), (old_admin, admin));
        Ok(())
    }

    /// Whitelists a token for use in new escrows. Only callable by admin.
    pub fn whitelist_token(env: Env, admin: Address, token: Address) -> Result<(), EscrowError> {
        storage::require_admin(&env, &admin)?;
        storage::whitelist_token(&env, &token);
        Ok(())
    }

    /// Creates a new escrow. Buyer funds are held by the contract until release or refund.
    /// - `buyer`: the address funding the escrow (must authorize)
    /// - `seller`: the address to receive funds on release
    /// - `token`: whitelisted token address
    /// - `amount`: must be greater than zero
    /// - `expiration`: must be in the future
    pub fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        token: Address,
        amount: i128,
        expiration: u64,
    ) -> Result<u64, EscrowError> {
        create::create_escrow(&env, buyer, seller, token, amount, expiration)
    }

    /// Releases funds to the seller. Only callable by the buyer or admin.
    pub fn release_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        release::release_escrow(&env, caller, escrow_id)
    }

    /// Refunds the buyer after expiry. Only callable after the expiration timestamp.
    pub fn refund_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        refund::refund_escrow(&env, caller, escrow_id)
    }

    /// Returns the escrow record for the given ID, if it exists.
    pub fn get_escrow(env: Env, escrow_id: u64) -> Option<Escrow> {
        storage::get_escrow(&env, escrow_id)
    }

    /// #638 — Sets the protocol fee in basis points. Hard-capped at 5000 bps (50%)
    /// to prevent admin from setting a fee that drains all payments.
    pub fn set_protocol_fee_bps(env: Env, admin: Address, bps: u32) -> Result<(), EscrowError> {
        const MAX_FEE_BPS: u32 = 5_000; // 50% hard cap
        storage::require_admin(&env, &admin)?;
        if bps > MAX_FEE_BPS {
            return Err(EscrowError::Unauthorized);
        }
        storage::set_protocol_fee_bps(&env, bps);
        Ok(())
    }
}
