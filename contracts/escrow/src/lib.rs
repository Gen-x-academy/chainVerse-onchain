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

    /// Refund escrowed funds to the buyer after the escrow has expired.
    /// Must be called by the buyer.
    pub fn refund_buyer(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        refund::refund_buyer(&env, escrow_id)
    }

    /// Returns the escrow record for the given ID.
    pub fn get_escrow(env: Env, escrow_id: u64) -> Result<Escrow, EscrowError> {
        storage::load_escrow(&env, escrow_id).ok_or(EscrowError::NotFound)
    }

    /// Returns the contract version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, version::CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token::{Client as TokenClient, StellarAssetClient},
        Env,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Registers all contracts, whitelists the token, mints tokens to the
    /// buyer, and sets the ledger timestamp to `now`.
    fn setup(now: u64) -> (Env, Address, Address, Address, EscrowContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();

        env.ledger().with_mut(|li| {
            li.timestamp = now;
        });

        let escrow_addr = env.register_contract(None, EscrowContract);
        let client = EscrowContractClient::new(&env, &escrow_addr);

        let token_admin = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(token_admin.clone());
        let stellar = StellarAssetClient::new(&env, &token_addr);

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        stellar.mint(&buyer, &1000);

        client.whitelist_token(&token_addr);

        (env, buyer, seller, token_addr, client)
    }

    // -----------------------------------------------------------------------
    // refund_buyer
    // -----------------------------------------------------------------------

    #[test]
    fn test_refund_buyer_success() {
        // Escrow created at t=1000, expires at t=2000. Refund at t=3000.
        let (env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &2000);

        // Advance time past expiration
        env.ledger().with_mut(|li| {
            li.timestamp = 3000;
        });

        client.refund_buyer(&escrow_id);

        // Buyer should have their 500 tokens back
        let balance = TokenClient::new(&env, &token_addr).balance(&buyer);
        assert_eq!(balance, 1000); // 500 remaining after create + 500 refunded

        // Escrow status should be Cancelled
        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Cancelled);
    }

    #[test]
    fn test_refund_buyer_fails_before_expiration() {
        // Escrow expires at t=5000; refund attempt at t=1000 should fail.
        let (env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &5000);

        let result = client.try_refund_buyer(&escrow_id);
        assert!(
            result.is_err(),
            "refund before expiration must be rejected"
        );
        let _ = env; // keep env alive
    }

    #[test]
    fn test_refund_buyer_fails_if_not_found() {
        let (_env, _buyer, _seller, _token, client) = setup(1000);
        let result = client.try_refund_buyer(&999);
        assert!(result.is_err(), "refund of non-existent escrow must fail");
    }

    #[test]
    fn test_refund_buyer_fails_if_already_released() {
        // Create escrow and release it, then try to refund — must fail.
        let (env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &9999);

        client.release_funds(&escrow_id);

        // Advance past expiration
        env.ledger().with_mut(|li| {
            li.timestamp = 10001;
        });

        let result = client.try_refund_buyer(&escrow_id);
        assert!(
            result.is_err(),
            "refund of a completed escrow must be rejected"
        );
    }
}
