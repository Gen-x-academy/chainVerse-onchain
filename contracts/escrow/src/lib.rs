#![no_std]

mod create;
mod dispute;
mod errors;
mod escrow_state;
mod events;
mod fund;
mod refund;
mod release;
mod storage;
mod types;
mod version;

pub use errors::EscrowError;
pub use types::{Escrow, EscrowStatus, FeeRecord};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

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
        env.events()
            .publish((soroban_sdk::symbol_short!("ADM_CHNG"),), (old_admin, admin));
        Ok(())
    }

    /// Whitelists a token for use in new escrows. Only callable by admin.
    pub fn whitelist_token(env: Env, admin: Address, token: Address) -> Result<(), EscrowError> {
        storage::require_admin_addr(&env, &admin)?;
        storage::whitelist_token(&env, &token);
        Ok(())
    }

    /// Creates an unfunded escrow. Buyer must later call `fund_escrow`.
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

    /// Deposits the escrow amount. Only the buyer may fund.
    pub fn fund_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        fund::fund_escrow(&env, caller, escrow_id)
    }

    /// Releases remaining funds to the seller. Buyer or admin.
    pub fn release_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        release::release_escrow(&env, caller, escrow_id)
    }

    /// Releases a portion of locked funds to the seller.
    pub fn partial_release(
        env: Env,
        caller: Address,
        escrow_id: u64,
        amount: i128,
    ) -> Result<(), EscrowError> {
        release::partial_release(&env, caller, escrow_id, amount)
    }

    /// Opens a dispute on a funded escrow (buyer or seller).
    pub fn dispute_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        dispute::dispute_escrow(&env, caller, escrow_id)
    }

    /// Refunds the buyer after the expiration timestamp (#709).
    pub fn refund_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        refund::refund_escrow(&env, caller, escrow_id)
    }

    /// Opens a dispute on a still-funded escrow. Only the buyer may dispute, and
    /// only while the escrow is `Pending` — a released or cancelled escrow can
    /// no longer be disputed, preventing post-settlement state corruption.
    pub fn dispute_escrow(env: Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
        dispute::dispute(&env, caller, escrow_id)
    }

    /// Returns the escrow record for the given ID, if it exists.
    pub fn get_escrow(env: Env, escrow_id: u64) -> Option<Escrow> {
        storage::get_escrow(&env, escrow_id)
    }

    /// Returns escrow IDs indexed by buyer.
    pub fn get_by_buyer_index(env: Env, buyer: Address) -> Vec<u64> {
        storage::get_buyer_index(&env, &buyer)
    }

    /// Sets the protocol fee in basis points. Hard-capped at 5000 bps (50%).
    pub fn set_protocol_fee_bps(env: Env, admin: Address, bps: u32) -> Result<(), EscrowError> {
        const MAX_FEE_BPS: u32 = 5_000;
        storage::require_admin_addr(&env, &admin)?;
        if bps > MAX_FEE_BPS {
            return Err(EscrowError::Unauthorized);
        }
        storage::set_protocol_fee_bps(&env, bps);
        Ok(())
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), EscrowError> {
        storage::require_admin(&env)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((soroban_sdk::symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }
}
