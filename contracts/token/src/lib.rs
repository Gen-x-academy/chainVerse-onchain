#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env,
};

// ---------------------------------------------------------------------------
// Error types  (#737 — replace panic!/unwrap with typed errors)
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TokenError {
    AlreadyInitialized   = 1,
    NotInitialized       = 2,
    InsufficientBalance  = 3,
    InsufficientAllowance = 4,
    AllowanceNotFound    = 5,
    AllowanceExpired     = 6,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
enum DataKey {
    Balance(Address),
    TotalSupply,
    Initialized,
    Allowance(Address, Address),
}

// ---------------------------------------------------------------------------
// Allowance record
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct Allowance {
    pub amount: i128,
    pub expires_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct TokenContract;

#[contractimpl]
impl TokenContract {

    pub fn initialize(env: Env, admin: Address, total_supply: i128) -> Result<(), TokenError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(TokenError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::TotalSupply, &total_supply);
        env.storage().instance().set(&DataKey::Balance(admin.clone()), &total_supply);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    pub fn total_supply(env: Env) -> Result<i128, TokenError> {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .ok_or(TokenError::NotInitialized)
    }

    pub fn balance(env: Env, user: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Balance(user))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        from.require_auth();
        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let to_balance = Self::balance(env.clone(), to.clone());
        env.storage().instance().set(&DataKey::Balance(from), &(from_balance - amount));
        env.storage().instance().set(&DataKey::Balance(to), &(to_balance + amount));
        Ok(())
    }

    /// Approve `spender` to spend `amount` of `owner`'s tokens until `expires_at` (ledger timestamp).
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128, expires_at: Option<u64>) {
        owner.require_auth();
        let allowance = Allowance { amount, expires_at };
        env.storage().persistent().set(&DataKey::Allowance(owner.clone(), spender.clone()), &allowance);
    }

    /// Returns remaining allowance for `spender` on `owner`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        if let Some(allow) = env.storage().persistent().get::<DataKey, Allowance>(&DataKey::Allowance(owner.clone(), spender.clone())) {
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

    /// Transfer tokens using allowance.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        let mut allow = env.storage()
            .persistent()
            .get::<DataKey, Allowance>(&DataKey::Allowance(from.clone(), spender.clone()))
            .ok_or(TokenError::AllowanceNotFound)?;

        if let Some(exp) = allow.expires_at {
            if env.ledger().timestamp() > exp {
                return Err(TokenError::AllowanceExpired);
            }
        }
        if allow.amount < amount {
            return Err(TokenError::InsufficientAllowance);
        }

        allow.amount -= amount;
        if allow.amount == 0 {
            env.storage().persistent().remove(&DataKey::Allowance(from.clone(), spender.clone()));
        } else {
            env.storage().persistent().set(&DataKey::Allowance(from.clone(), spender.clone()), &allow);
        }

        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let to_bal = Self::balance(env.clone(), to.clone());
        env.storage().instance().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage().instance().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        Ok(())
    }

    /// Revoke allowance.
    pub fn revoke_allowance(env: Env, owner: Address, spender: Address) {
        owner.require_auth();
        env.storage().persistent().remove(&DataKey::Allowance(owner.clone(), spender.clone()));
    }
}
