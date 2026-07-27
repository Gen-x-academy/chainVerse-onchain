#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Symbol
};

// TTL constants: ~1 year at 6-second ledgers (issue #735)
const BALANCE_MIN_TTL: u32 = 3_110_400;
const BALANCE_MAX_TTL: u32 = 6_220_800;

#[contract]
pub struct TokenContract;

#[contracttype]
enum DataKey {
    Balance(Address),
    TotalSupply,
    Initialized,
    Allowance(Address, Address),
}

/// Allowance record stored in persistent storage.
/// Requires `#[contracttype]` for stable XDR serialization (issue #738).
#[contracttype]
#[derive(Clone)]
pub struct Allowance {
    pub amount: i128,
    pub expires_at: Option<u64>,
}

#[contractimpl]
impl TokenContract {

    pub fn initialize(env: Env, admin: Address, total_supply: i128) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::TotalSupply, &total_supply);
        env.storage().instance().set(&DataKey::Balance(admin.clone()), &total_supply);
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    pub fn total_supply(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap()
    }

    pub fn balance(env: Env, user: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Balance(user))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("insufficient balance");
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .instance()
            .set(&DataKey::Balance(from), &(from_balance - amount));

        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(to_balance + amount));
        // no persistent storage used here — instance storage has no TTL
    }

    /// Approve `spender` to spend `amount` of `owner`'s tokens until `expires_at` (ledger timestamp).
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128, expires_at: Option<u64>) {
        owner.require_auth();
        let allowance = Allowance { amount, expires_at };
        let key = DataKey::Allowance(owner.clone(), spender.clone());
        env.storage().persistent().set(&key, &allowance);
        // Extend TTL so the allowance entry survives on testnet (issue #735)
        env.storage().persistent().extend_ttl(&key, BALANCE_MIN_TTL, BALANCE_MAX_TTL);
    }

    /// Returns remaining allowance for `spender` on `owner`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        if let Some(allow) = env.storage().persistent().get(&DataKey::Allowance(owner.clone(), spender.clone())) {
            if let Some(exp) = allow.expires_at {
                if env.ledger().timestamp() > exp {
                    return 0;
                }
            }
            allow.amount
        } else {
            0
        }
    }

    /// Transfer tokens using allowance. `spender` does not need auth; allowance is checked and decremented.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        // Verify allowance
        let mut allow = env.storage().persistent().get(&DataKey::Allowance(from.clone(), spender.clone()))
            .expect("allowance not found");
        if let Some(exp) = allow.expires_at {
            if env.ledger().timestamp() > exp {
                panic!("allowance expired");
            }
        }
        if allow.amount < amount {
            panic!("insufficient allowance");
        }
        // Decrement allowance
        allow.amount -= amount;
        if allow.amount == 0 {
            env.storage().persistent().remove(&DataKey::Allowance(from.clone(), spender.clone()));
        } else {
            let key = DataKey::Allowance(from.clone(), spender.clone());
            env.storage().persistent().set(&key, &allow);
            env.storage().persistent().extend_ttl(&key, BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        }
        // Perform balance transfer
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            panic!("insufficient balance");
        }
        let to_bal = Self::balance(env.clone(), to.clone());
        env.storage().instance().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage().instance().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
    }

    /// Revoke allowance.
    pub fn revoke_allowance(env: Env, owner: Address, spender: Address) {
        owner.require_auth();
        env.storage().persistent().remove(&DataKey::Allowance(owner.clone(), spender.clone()));
    }
}