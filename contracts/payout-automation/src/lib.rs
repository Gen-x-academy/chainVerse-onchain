#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    token::Client as TokenClient,
    symbol_short, Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayoutError {
    Unauthorized  = 1,
    NotInitialized = 2,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single entry in a payout batch.
#[contracttype]
#[derive(Clone)]
pub struct PayoutEntry {
    pub recipient: Address,
    pub amount:    i128,
}

#[contracttype]
pub enum DataKey {
    Authorised(Address),
    Admin,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct PayoutAutomation;

#[contractimpl]
impl PayoutAutomation {
    /// One-time initialisation — sets the admin and first authorised caller.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Authorised(admin), &true);
    }

    /// Admin-only: add an address to the authorised set.
    pub fn add_authorised(env: Env, admin: Address, caller: Address) -> Result<(), PayoutError> {
        admin.require_auth();
        Self::only_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Authorised(caller), &true);
        Ok(())
    }

    /// Admin-only: remove an address from the authorised set.
    pub fn remove_authorised(env: Env, admin: Address, caller: Address) -> Result<(), PayoutError> {
        admin.require_auth();
        Self::only_admin(&env, &admin)?;
        env.storage().instance().remove(&DataKey::Authorised(caller));
        Ok(())
    }

    /// Execute a batch payout. The caller must be in the authorised set.
    /// Transfers `token` from the contract to each recipient in `payouts`.
    /// An empty batch is accepted gracefully (no-op).
    pub fn execute(
        env: Env,
        caller: Address,
        token: Address,
        payouts: Vec<PayoutEntry>,
    ) -> Result<(), PayoutError> {
        caller.require_auth();

        if !Self::is_authorised(&env, &caller) {
            return Err(PayoutError::Unauthorized);
        }

        let token_client = TokenClient::new(&env, &token);
        let mut total_amount: i128 = 0;
        let mut recipient_count: u32 = 0;
        for entry in payouts.iter() {
            if entry.amount <= 0 {
                continue;
            }
            token_client.transfer(
                &env.current_contract_address(),
                &entry.recipient,
                &entry.amount,
            );
            total_amount += entry.amount;
            recipient_count += 1;
        }

        env.events().publish(
            (symbol_short!("payout"), symbol_short!("executed")),
            (caller, token, total_amount, recipient_count),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn is_authorised(env: &Env, addr: &Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Authorised(addr.clone()))
            .unwrap_or(false)
    }

    fn only_admin(env: &Env, caller: &Address) -> Result<(), PayoutError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(PayoutError::NotInitialized)?;
        if &admin != caller {
            return Err(PayoutError::Unauthorized);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::Address as _,
        token::{Client as TokenClient, StellarAssetClient},
        vec, Env,
    };

    /// Register the contract, initialise it, mint `amount` tokens to the
    /// contract address, and return useful handles.
    fn setup(
        amount: i128,
    ) -> (
        Env,
        Address,                          // admin / authorised caller
        Address,                          // token address
        PayoutAutomationClient<'static>,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, PayoutAutomation);
        let client = PayoutAutomationClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Mint tokens directly into the payout contract so it can distribute them
        let token_admin = Address::generate(&env);
        let token_addr = env.register_stellar_asset_contract(token_admin.clone());
        StellarAssetClient::new(&env, &token_addr).mint(&contract_id, &amount);

        (env, admin, token_addr, client)
    }

    // Issue #100 — execute with valid authorisation and well-formed batch
    #[test]
    fn test_execute_with_valid_authorisation_pays_all_recipients() {
        let (env, admin, token_addr, client) = setup(1000);

        let r1 = Address::generate(&env);
        let r2 = Address::generate(&env);

        let payouts = vec![
            &env,
            PayoutEntry { recipient: r1.clone(), amount: 300 },
            PayoutEntry { recipient: r2.clone(), amount: 700 },
        ];

        client.execute(&admin, &token_addr, &payouts);

        let tc = TokenClient::new(&env, &token_addr);
        assert_eq!(tc.balance(&r1), 300);
        assert_eq!(tc.balance(&r2), 700);
    }

    // Issue #101 — execute from unauthorised caller must be rejected
    #[test]
    fn test_execute_from_unauthorised_caller_is_rejected() {
        let (env, _admin, token_addr, client) = setup(1000);

        let outsider  = Address::generate(&env);
        let recipient = Address::generate(&env);

        let payouts = vec![
            &env,
            PayoutEntry { recipient: recipient.clone(), amount: 100 },
        ];

        let result = client.try_execute(&outsider, &token_addr, &payouts);
        assert!(result.is_err(), "execute from non-authorised caller must be rejected");

        // No funds must have moved
        let tc = TokenClient::new(&env, &token_addr);
        assert_eq!(tc.balance(&recipient), 0);
    }

    // Issue #102 — execute with empty batch must be handled gracefully
    #[test]
    fn test_execute_with_empty_batch_is_graceful() {
        let (env, admin, token_addr, client) = setup(500);

        let empty: Vec<PayoutEntry> = vec![&env];

        // Must not panic or revert
        client.execute(&admin, &token_addr, &empty);

        // Contract balance unchanged
        let contract_id = client.address.clone();
        let tc = TokenClient::new(&env, &token_addr);
        assert_eq!(tc.balance(&contract_id), 500);
    }
}
