#![no_std]

mod create;
mod errors;
mod events;
mod release;
mod storage;
mod types;
mod version;

pub use errors::EscrowError;
pub use types::{Escrow, EscrowStatus};

use soroban_sdk::{contract, contractimpl, Address, Env, String};

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Whitelist a token so it can be used in escrows. Admin-only in production;
    /// kept simple here as a direct call for composability.
    pub fn whitelist_token(env: Env, token: Address) {
        storage::whitelist_token(&env, &token);
    }

    /// Create a new escrow. Transfers `amount` of `token` from `buyer` into
    /// the contract and returns the new escrow ID.
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

    /// Release escrowed funds to the seller.
    /// Must be called by the buyer.
    pub fn release_funds(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        release::release_funds(&env, escrow_id)
    }

    /// Returns the contract version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, version::CONTRACT_VERSION)
    }
}
