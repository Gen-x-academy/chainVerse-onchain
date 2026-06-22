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

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Sets the initial escrow admin or rotates the admin after deployment.
    ///
    /// The `admin` parameter is the address that will be stored as the new
    /// administrator. If an admin is already configured, the current admin must
    /// authorize the call; otherwise the proposed admin must authorize the
    /// initial setup. This function does not return contract-defined errors.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), EscrowError> {
        if let Some(current_admin) = storage::get_admin(&env) {
            current_admin.require_auth();
        } else {
            admin.require_auth();
        }

        storage::set_admin(&env, &admin);
        Ok(())
    }

    /// Allows a token address to be used when creating new escrows.
    ///
    /// The `token` parameter identifies the token contract to whitelist.
    /// Security requirement: only the configured admin may call this function.
    /// Returns `EscrowError::Unauthorized` if no admin exists or the admin does
    /// not authorize the call.
    pub fn whitelist_token(env: Env, token: Address) -> Result<(), EscrowError> {
        storage::require_admin(&env)?;
        storage::whitelist_token(&env, &token);
        Ok(())
    }

    /// Upgrades the current escrow contract to `new_wasm_hash`.
    ///
    /// The `admin` parameter must match the stored admin and authorize through
    /// `storage::require_admin`; `new_wasm_hash` is the target WASM hash.
    /// Returns `EscrowError::Unauthorized` if the stored admin is missing, the
    /// authorization fails, or the supplied `admin` is not the stored admin.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), EscrowError> {
        let stored_admin = storage::require_admin(&env)?;
        if stored_admin != admin {
            return Err(EscrowError::Unauthorized);
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    /// Creates a new pending escrow and transfers the buyer's tokens into it.
    ///
    /// The `buyer` funds the escrow, `seller` receives funds when released,
    /// `token` must be whitelisted, `amount` is the deposited token quantity,
    /// and `expiration` is the ledger timestamp after which refunds may occur.
    /// Security requirement: the buyer must authorize the token transfer.
    /// Returns `InvalidAmount`, `InvalidParties`, `InvalidExpiration`, or
    /// `TokenNotAllowed` when validation fails.
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

    /// Releases escrowed funds to the seller for an active escrow.
    ///
    /// The `escrow_id` parameter identifies the escrow to release. Security
    /// requirement: the escrow buyer must authorize the call. Returns `NotFound`
    /// if the escrow does not exist, `AlreadyReleased` for completed escrows,
    /// `NotPending` for disputed or otherwise non-pending escrows, and `Expired`
    /// if the escrow has already expired.
    pub fn release_funds(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        release::release_funds(&env, escrow_id)
    }

    /// Refunds escrowed funds to the buyer after expiration.
    ///
    /// The `escrow_id` parameter identifies the escrow to refund. Security
    /// requirement: the escrow buyer must authorize the call. Returns `NotFound`
    /// if the escrow does not exist, `NotPending` when it is not refundable, and
    /// `NotExpired` if the current ledger timestamp is still before expiration.
    pub fn refund_buyer(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        refund::refund_buyer(&env, escrow_id)
    }

    /// Loads the escrow record for `escrow_id`.
    ///
    /// This read-only helper has no authorization requirement. It returns
    /// `EscrowError::NotFound` when no escrow exists for the supplied ID.
    pub fn get_escrow(env: Env, escrow_id: u64) -> Result<Escrow, EscrowError> {
        storage::load_escrow(&env, escrow_id).ok_or(EscrowError::NotFound)
    }

    /// Returns the total token amount ever deposited through escrow creation.
    ///
    /// This read-only helper takes no contract parameters beyond `env`, has no
    /// authorization requirement, and does not return contract-defined errors.
    pub fn get_total_volume(env: Env) -> i128 {
        storage::get_total_volume(&env)
    }

    /// Returns the accumulated protocol fees for `token`.
    ///
    /// The `token` parameter identifies the token whose fee balance should be
    /// read. This read-only helper has no authorization requirement and returns
    /// zero when no fees have been recorded.
    pub fn get_protocol_fee(env: Env, token: Address) -> i128 {
        storage::get_protocol_fee(&env, &token)
    }

    /// Returns the total number of escrow IDs that have been created.
    ///
    /// This read-only helper takes no contract parameters beyond `env`, has no
    /// authorization requirement, and returns zero before the first escrow is
    /// created.
    pub fn get_escrow_count(env: Env) -> u64 {
        storage::get_escrow_count(&env)
    }

    /// Marks a pending escrow as disputed to block normal release or refund flow.
    ///
    /// The `escrow_id` parameter identifies the escrow to dispute. Security
    /// requirement: the current implementation requires buyer authorization.
    /// Returns `NotFound` when the escrow is missing, `AlreadyDisputed` when it
    /// is already disputed, and `NotPending` when it is not pending.
    pub fn flag_dispute(env: Env, escrow_id: u64) -> Result<(), EscrowError> {
        let mut escrow = storage::load_escrow(&env, escrow_id).ok_or(EscrowError::NotFound)?;

        if escrow.status == EscrowStatus::Disputed {
            return Err(EscrowError::AlreadyDisputed);
        }
        if escrow.status != EscrowStatus::Pending {
            return Err(EscrowError::NotPending);
        }

        escrow.buyer.require_auth();

        escrow.status = EscrowStatus::Disputed;
        storage::save_escrow(&env, escrow_id, &escrow);
        Ok(())
    }

    /// Placeholder entry point for resolving a disputed escrow.
    ///
    /// The `_escrow_id` parameter identifies the escrow and `_release_to_seller`
    /// will eventually choose the ruling direction. Security requirements are
    /// not yet implemented because arbitration is still a placeholder. This
    /// function always returns `EscrowError::DisputeResolutionNotImplemented`.
    pub fn resolve_dispute(
        _env: Env,
        _escrow_id: u64,
        _release_to_seller: bool,
    ) -> Result<(), EscrowError> {
        Err(EscrowError::DisputeResolutionNotImplemented)
    }

    /// Withdraws accumulated protocol fees for `token` to the admin address.
    ///
    /// The `token` parameter identifies which token's fee balance to withdraw.
    /// Security requirement: the configured admin must authorize the call.
    /// Returns `Unauthorized` if no admin is configured or authorization fails,
    /// and `NoFeesAvailable` when the token has no accumulated fees.
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

    /// Returns the static contract version string.
    ///
    /// This read-only helper takes no contract parameters beyond `env`, has no
    /// authorization requirement, and does not return contract-defined errors.
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
