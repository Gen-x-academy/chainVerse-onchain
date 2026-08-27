#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};
mod error;
mod events;
use error::TokenError;

const DECIMALS: u32 = 7;
/// Fix #630: Hard cap — 1 billion CHV tokens. Enforced in mint(); cannot be changed post-deploy.
const MAX_SUPPLY: i128 = 1_000_000_000 * 10_i128.pow(DECIMALS);
const BALANCE_MIN_TTL: u32 = 100_000;
const BALANCE_MAX_TTL: u32 = 200_000;
const ALLOWANCE_MIN_TTL: u32 = 100_000;
const ALLOWANCE_MAX_TTL: u32 = 200_000;
const CURRENT_STORAGE_VERSION: u32 = 1;

#[contracttype]
pub enum DataKey {
    Admin,
    Balance(Address),
    Initialized,
    TotalMinted,
    CirculatingSupply,
    StorageVersion,
    PendingAdmin,
    Allowance(Address, Address),
    Frozen(Address),
}

#[contract]
pub struct CHVToken;

#[contractimpl]
impl CHVToken {
    pub fn initialize(env: Env, admin: Address, treasury: Address) -> Result<(), TokenError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(TokenError::AlreadyInitialized);
        }
        admin.require_auth();
        let initial_supply: i128 = 100_000_000 * 10_i128.pow(DECIMALS);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&(DataKey::Balance(treasury.clone())), &initial_supply);
        env.storage().instance().set(&DataKey::TotalMinted, &initial_supply);
        env.storage().instance().set(&DataKey::CirculatingSupply, &initial_supply);
        env.storage().instance().set(&DataKey::StorageVersion, &CURRENT_STORAGE_VERSION);
        env.events().publish((symbol_short!("INIT"),), (admin, treasury, initial_supply));
        Ok(())
    }

    /// Fix #630: Mints new CHV tokens to `to`, enforcing the MAX_SUPPLY hard cap.
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        admin.require_auth();
        let total_minted: i128 = env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0);
        if total_minted > MAX_SUPPLY || amount > MAX_SUPPLY - total_minted {
            return Err(TokenError::SupplyCapExceeded);
        }
        let circulating_supply: i128 = env.storage().instance()
            .get(&DataKey::CirculatingSupply).unwrap_or(total_minted);
        let balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(balance + amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.storage().instance().set(&DataKey::TotalMinted, &(total_minted + amount));
        env.storage().instance().set(&DataKey::CirculatingSupply, &(circulating_supply + amount));
        events::emit_mint(&env, &to, amount);
        Ok(())
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if from == to {
            return Err(TokenError::SelfTransfer);
        }
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        if Self::is_frozen(env.clone(), from.clone()) {
            return Err(TokenError::AccountFrozen);
        }
        from.require_auth();
        let from_bal: i128 = env.storage().persistent()
            .get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if from_bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let to_bal: i128 = env.storage().persistent()
            .get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(from.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        events::emit_transfer(&env, &from, &to, amount);
        Ok(())
    }

    /// Approve `spender` to spend up to `amount` of `owner`'s tokens.
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128) -> Result<(), TokenError> {
        if amount < 0 {
            return Err(TokenError::InvalidAmount);
        }
        owner.require_auth();
        let key = DataKey::Allowance(owner, spender);
        env.storage().persistent().set(&key, &amount);
        env.storage().persistent().extend_ttl(&key, ALLOWANCE_MIN_TTL, ALLOWANCE_MAX_TTL);
        Ok(())
    }

    /// Returns remaining allowance for `spender` on `owner`.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        let key = DataKey::Allowance(owner, spender);
        let allowance = env.storage().persistent().get(&key);
        if allowance.is_some() {
            env.storage().persistent().extend_ttl(&key, ALLOWANCE_MIN_TTL, ALLOWANCE_MAX_TTL);
        }
        allowance.unwrap_or(0)
    }

    /// Transfer tokens using allowance. `spender` does not need auth; allowance is checked and decremented.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if from == to {
            return Err(TokenError::SelfTransfer);
        }
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        if Self::is_frozen(env.clone(), from.clone()) {
            return Err(TokenError::AccountFrozen);
        }
        spender.require_auth();
        let mut allow = env.storage().persistent().get(&DataKey::Allowance(from.clone(), spender.clone()))
            .ok_or(TokenError::InsufficientAllowance)?;
        if allow < amount {
            return Err(TokenError::InsufficientAllowance);
        }
        allow -= amount;
        if allow == 0 {
            env.storage().persistent().remove(&DataKey::Allowance(from.clone(), spender.clone()));
        } else {
            let key = DataKey::Allowance(from.clone(), spender.clone());
            env.storage().persistent().set(&key, &allow);
            env.storage().persistent().extend_ttl(&key, ALLOWANCE_MIN_TTL, ALLOWANCE_MAX_TTL);
        }
        let from_bal = env.storage().persistent().get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if from_bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let to_bal = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(from.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        events::emit_transfer(&env, &from, &to, amount);
        Ok(())
    }

    /// Revoke a previously set allowance.
    pub fn revoke_allowance(env: Env, owner: Address, spender: Address) -> Result<(), TokenError> {
        owner.require_auth();
        env.storage().persistent().remove(&DataKey::Allowance(owner, spender));
        Ok(())
    }

    /// Admin-only: freeze an account to prevent outgoing transfers.
    pub fn freeze_account(env: Env, account: Address) -> Result<(), TokenError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Frozen(account.clone()), &true);
        events::emit_freeze(&env, &account);
        Ok(())
    }

    /// Admin-only: unfreeze a previously frozen account.
    pub fn unfreeze_account(env: Env, account: Address) -> Result<(), TokenError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Frozen(account.clone()), &false);
        events::emit_unfreeze(&env, &account);
        Ok(())
    }

    /// Returns true if the account is frozen.
    pub fn is_frozen(env: Env, account: Address) -> bool {
        env.storage().persistent().get(&DataKey::Frozen(account)).unwrap_or(false)
    }

    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        from.require_auth();
        let bal = env.storage().persistent()
            .get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let total_minted: i128 = env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0);
        let circulating_supply: i128 = env.storage().instance()
            .get(&DataKey::CirculatingSupply).unwrap_or(total_minted);
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(bal - amount));
        env.storage().instance().set(&DataKey::CirculatingSupply, &(circulating_supply - amount));
        events::emit_burn(&env, &from, amount);
        Ok(())
    }

    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage().persistent().get(&DataKey::Balance(account)).unwrap_or(0)
    }

    pub fn total_minted(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0)
    }

    /// Returns the current token supply after burns.
    pub fn circulating_supply(env: Env) -> i128 {
        let total_minted: i128 = env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0);
        env.storage().instance().get(&DataKey::CirculatingSupply).unwrap_or(total_minted)
    }

    /// Returns the storage schema version, treating unversioned legacy storage as version zero.
    pub fn storage_version(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::StorageVersion).unwrap_or(0)
    }

    /// Initializes or confirms the storage schema version after a contract upgrade.
    pub fn migrate(
        env: Env,
        admin: Address,
        source_version: u32,
        circulating_supply: i128,
    ) -> Result<(), TokenError> {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        if stored_admin != admin {
            return Err(TokenError::Unauthorized);
        }
        admin.require_auth();

        let current_version = Self::storage_version(env.clone());
        if source_version != current_version {
            return Err(TokenError::UnsupportedStorageVersion);
        }
        if current_version == CURRENT_STORAGE_VERSION {
            if circulating_supply != Self::circulating_supply(env) {
                return Err(TokenError::InvalidMigration);
            }
            return Ok(());
        }
        if circulating_supply < 0 || circulating_supply > Self::total_minted(env.clone()) {
            return Err(TokenError::InvalidMigration);
        }
        env.storage().instance().set(&DataKey::CirculatingSupply, &circulating_supply);
        env.storage().instance().set(&DataKey::StorageVersion, &CURRENT_STORAGE_VERSION);
        Ok(())
    }

    /// #635 — Step 1: current admin proposes a new admin. Does not transfer immediately.
    pub fn propose_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), TokenError> {
        let stored: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::Unauthorized)?;
        if stored != current_admin { return Err(TokenError::Unauthorized); }
        current_admin.require_auth();
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
        env.events().publish((symbol_short!("ADM_PROP"),), (current_admin, new_admin));
        Ok(())
    }

    /// #635 — Step 2: pending admin accepts and becomes the new admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), TokenError> {
        let pending: Address = env.storage().instance().get(&DataKey::PendingAdmin)
            .ok_or(TokenError::NoPendingAdmin)?;
        if pending != new_admin { return Err(TokenError::Unauthorized); }
        new_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events().publish((symbol_short!("ADM_NEW"),), (new_admin,));
        Ok(())
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), TokenError> {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        if stored_admin != admin {
            return Err(TokenError::Unauthorized);
        }
        admin.require_auth();
        let source_version = Self::storage_version(env.clone());
        if source_version != 0 && source_version != CURRENT_STORAGE_VERSION {
            return Err(TokenError::UnsupportedStorageVersion);
        }
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod extended_test;