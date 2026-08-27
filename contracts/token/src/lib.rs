#![no_std]

pub mod royalty;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};
pub use royalty::RoyaltyConfig;

// TTL constants: ~1 year at 6-second ledgers (issue #735)
const BALANCE_MIN_TTL: u32 = 3_110_400;
const BALANCE_MAX_TTL: u32 = 6_220_800;

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
    InvalidAmount         = 7,
    Unauthorized          = 8,
    RoyaltyBpsTooHigh     = 9,
    ArithmeticOverflow    = 10,
    ContractPaused        = 11,
    NoPendingAdmin        = 12,
}

#[contracttype]
enum DataKey {
    Balance(Address),
    TotalSupply,
    Initialized,
    Admin,
    PendingAdmin,
    Paused,
    Royalty,
    Allowance(Address, Address),
}

#[contracttype]
#[derive(Clone)]
pub struct Allowance {
    pub amount: i128,
    pub expires_at: Option<u64>,
}

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
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
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
        if Self::is_paused(env.clone()) {
            return Err(TokenError::ContractPaused);
        }
        from.require_auth();
        Self::apply_transfer(&env, from, to, amount)
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), TokenError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((symbol_short!("PAUSED"),), (admin,));
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), TokenError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((symbol_short!("UNPAUSED"),), (admin,));
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    pub fn propose_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), TokenError> {
        Self::require_admin(&env, &current_admin)?;
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
        env.events().publish((symbol_short!("ADM_PROP"),), (current_admin, new_admin));
        Ok(())
    }

    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), TokenError> {
        let pending: Address = env.storage().instance().get(&DataKey::PendingAdmin)
            .ok_or(TokenError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(TokenError::Unauthorized);
        }
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events().publish((symbol_short!("ADM_NEW"),), (new_admin,));
        Ok(())
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), TokenError> {
        Self::require_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("UPGRADED"),), (new_wasm_hash,));
        Ok(())
    }

    pub fn set_royalty(
        env: Env,
        admin: Address,
        recipient: Address,
        bps: u32,
    ) -> Result<(), TokenError> {
        royalty::set_royalty(&env, admin, recipient, bps)
    }

    pub fn royalty(env: Env) -> Option<RoyaltyConfig> {
        royalty::get_royalty(&env)
    }

    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128, expires_at: Option<u64>) {
        owner.require_auth();
        let allowance = Allowance { amount, expires_at };
        let key = DataKey::Allowance(owner.clone(), spender.clone());
        env.storage().persistent().set(&key, &allowance);
        env.storage().persistent().extend_ttl(&key, BALANCE_MIN_TTL, BALANCE_MAX_TTL);
    }

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

    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if Self::is_paused(env.clone()) {
            return Err(TokenError::ContractPaused);
        }
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
        Self::apply_transfer(&env, from.clone(), to, amount)?;

        allow.amount -= amount;
        if allow.amount == 0 {
            env.storage().persistent().remove(&DataKey::Allowance(from, spender));
        } else {
            let key = DataKey::Allowance(from, spender);
            env.storage().persistent().set(&key, &allow);
            env.storage().persistent().extend_ttl(&key, BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        }
        Ok(())
    }

    fn apply_transfer(env: &Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let royalty = royalty::get_royalty(env);
        let royalty_amount = royalty.as_ref()
            .map(|config| amount.checked_mul(config.bps as i128).ok_or(TokenError::ArithmeticOverflow))
            .transpose()?
            .map(|value| value / 10_000)
            .unwrap_or(0);
        let proceeds = amount.checked_sub(royalty_amount).ok_or(TokenError::ArithmeticOverflow)?;
        let recipient = royalty.map(|config| config.recipient);

        if from == to {
            if let Some(recipient) = recipient {
                if recipient != from && royalty_amount > 0 {
                    let from_after = from_balance.checked_sub(royalty_amount)
                        .ok_or(TokenError::ArithmeticOverflow)?;
                    let recipient_balance = Self::balance(env.clone(), recipient.clone());
                    let recipient_after = recipient_balance.checked_add(royalty_amount)
                        .ok_or(TokenError::ArithmeticOverflow)?;
                    env.storage().instance().set(&DataKey::Balance(from), &from_after);
                    env.storage().instance().set(&DataKey::Balance(recipient), &recipient_after);
                }
            }
            return Ok(());
        }

        let from_after = from_balance.checked_sub(amount).ok_or(TokenError::ArithmeticOverflow)?;
        let to_balance = Self::balance(env.clone(), to.clone());
        let to_credit = if recipient.as_ref() == Some(&to) { amount } else { proceeds };
        let to_after = to_balance.checked_add(to_credit).ok_or(TokenError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::Balance(from.clone()), &from_after);
        env.storage().instance().set(&DataKey::Balance(to.clone()), &to_after);
        if let Some(recipient) = recipient {
            if royalty_amount > 0 && recipient != to {
                let recipient_balance = Self::balance(env.clone(), recipient.clone());
                let recipient_after = recipient_balance.checked_add(royalty_amount)
                    .ok_or(TokenError::ArithmeticOverflow)?;
                env.storage().instance().set(&DataKey::Balance(recipient), &recipient_after);
            }
        }
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), TokenError> {
        let configured_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        if configured_admin != *admin {
            return Err(TokenError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    pub fn revoke_allowance(env: Env, owner: Address, spender: Address) {
        owner.require_auth();
        env.storage().persistent().remove(&DataKey::Allowance(owner.clone(), spender.clone()));
    }
}
