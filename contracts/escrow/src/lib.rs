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
    /// Sets or rotates the escrow admin.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), EscrowError> {
        if let Some(current_admin) = storage::get_admin(&env) {
            current_admin.require_auth();
        } else {
            admin.require_auth();
        }

        storage::set_admin(&env, &admin);
        Ok(())
    }

    /// Whitelist a token so it can be used in escrows.
    pub fn whitelist_token(env: Env, token: Address) -> Result<(), EscrowError> {
        storage::require_admin(&env)?;
        storage::whitelist_token(&env, &token);
        Ok(())
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

    /// Returns the total token volume that has been deposited into escrow.
    pub fn get_total_volume(env: Env) -> i128 {
        storage::get_total_volume(&env)
    }

    /// Returns the total protocol fees accumulated for a given token.
    pub fn get_protocol_fee(env: Env, token: Address) -> i128 {
        storage::get_protocol_fee(&env, &token)
    }

    /// Set the contract admin. Can only be called once (if no admin is set).
    pub fn set_admin(env: Env, admin: Address) -> Result<(), EscrowError> {
        if storage::get_admin(&env).is_some() {
            return Err(EscrowError::Unauthorized);
        }
        storage::set_admin(&env, &admin);
        Ok(())
    }

    /// Withdraw accumulated protocol fees for a token to the admin's address.
    /// Only callable by the admin.
    pub fn withdraw_fees(env: Env, token: Address) -> Result<(), EscrowError> {
        let admin = storage::get_admin(&env).ok_or(EscrowError::Unauthorized)?;
        admin.require_auth();

        let fees = storage::get_protocol_fee(&env, &token);
        if fees == 0 {
            return Err(EscrowError::NoFeesAvailable);
        }

        soroban_sdk::token::Client::new(&env, &token).transfer(
            &env.current_contract_address(),
            &admin,
            &fees,
        );

        storage::clear_protocol_fee(&env, &token);
        Ok(())
    }

    /// Returns the contract version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, version::CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation, Ledger},
        token::{Client as TokenClient, StellarAssetClient},
        Env, IntoVal, Symbol,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Registers all contracts, whitelists the token, mints tokens to the
    /// buyer, and sets the ledger timestamp to `now`.
    fn setup(
        now: u64,
    ) -> (
        Env,
        Address,
        Address,
        Address,
        EscrowContractClient<'static>,
    ) {
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

        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        stellar.mint(&buyer, &1000);

        client.set_admin(&admin);
        client.whitelist_token(&token_addr);

        (env, buyer, seller, token_addr, client)
    }

    #[test]
    fn test_whitelist_token_requires_admin_setup() {
        let env = Env::default();
        env.mock_all_auths();

        let escrow_addr = env.register_contract(None, EscrowContract);
        let client = EscrowContractClient::new(&env, &escrow_addr);

        let token_admin = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(token_admin);

        let result = client.try_whitelist_token(&token_addr);
        assert_eq!(result, Err(Ok(EscrowError::Unauthorized)));
    }

    #[test]
    fn test_whitelist_token_requires_admin_auth() {
        let env = Env::default();
        env.mock_all_auths();

        let escrow_addr = env.register_contract(None, EscrowContract);
        let client = EscrowContractClient::new(&env, &escrow_addr);

        let admin = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(token_admin);

        client.set_admin(&admin);
        client.whitelist_token(&token_addr);

        assert_eq!(
            env.auths(),
            std::vec![(
                admin,
                AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        client.address.clone(),
                        Symbol::new(&env, "whitelist_token"),
                        (&token_addr,).into_val(&env),
                    )),
                    sub_invocations: std::vec![],
                },
            )]
        );
    }

    // -----------------------------------------------------------------------
    // get_total_volume
    // -----------------------------------------------------------------------

    #[test]
    fn test_total_volume_increases_correctly() {
        let (_, buyer, seller, token_addr, client) = setup(1000);

        assert_eq!(client.get_total_volume(), 0);

        client.create_escrow(&buyer, &seller, &token_addr, &300, &9000);
        assert_eq!(client.get_total_volume(), 300);

        client.create_escrow(&buyer, &seller, &token_addr, &200, &9000);
        assert_eq!(client.get_total_volume(), 500);
    }

    #[test]
    fn test_create_escrow_rejects_expired_timestamps() {
        let (_env, buyer, seller, token_addr, client) = setup(1000);

        let result = client.try_create_escrow(&buyer, &seller, &token_addr, &300, &1000);
        assert_eq!(result, Err(Ok(EscrowError::InvalidExpiration)));
    }

    #[test]
    fn test_create_escrow_rejects_same_party() {
        let (_env, buyer, _seller, token_addr, client) = setup(1000);

        let result = client.try_create_escrow(&buyer, &buyer, &token_addr, &300, &2000);
        assert_eq!(result, Err(Ok(EscrowError::InvalidParties)));
    }

    // -----------------------------------------------------------------------
    // release_funds
    // -----------------------------------------------------------------------

    #[test]
    fn test_release_funds_success() {
        let (env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &2000);
        client.release_funds(&escrow_id);

        let escrow = client.get_escrow(&escrow_id);
        assert_eq!(escrow.status, EscrowStatus::Completed);

        let seller_balance = TokenClient::new(&env, &token_addr).balance(&seller);
        assert_eq!(seller_balance, 500);
    }

    #[test]
    fn test_release_funds_fails_if_expired() {
        let (env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &2000);

        // Advance past expiration
        env.ledger().with_mut(|li| {
            li.timestamp = 3000;
        });

        let result = client.try_release_funds(&escrow_id);
        assert!(result.is_err(), "release after expiration must fail");
    }

    #[test]
    fn test_release_funds_fails_on_double_release() {
        let (_env, buyer, seller, token_addr, client) = setup(1000);

        let escrow_id = client.create_escrow(&buyer, &seller, &token_addr, &500, &9000);
        client.release_funds(&escrow_id);

        // Second release must be rejected with AlreadyReleased
        let result = client.try_release_funds(&escrow_id);
        assert!(result.is_err(), "double release must be rejected");
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
        assert!(result.is_err(), "refund before expiration must be rejected");
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
