#![no_std]
//! #977 — Add refundable physical-loan deposits.
//!
//! Integrates per-loan token deposits with three explicit resolution outcomes:
//! full refund to the patron (`release_deposit`), partial charge to the
//! treasury (`partial_charge`), and full charge to the treasury (`full_charge`).
//!
//! The contract holds actual tokens during the loan. Solvency is maintained by
//! design: every charge or release transfers exactly `charge_amount` or
//! `remaining_amount` respectively, and `remaining_amount` is always
//! decremented atomically. A partial charge that would exceed `remaining_amount`
//! returns `InsufficientBalance` before touching any tokens.
//!
//! Only the admin may initiate charges; the patron (or admin) may release.
//!
//! ## ABI
//! `initialize`, `lock_deposit`, `release_deposit`, `partial_charge`,
//! `full_charge`, `get_deposit`.
//!
//! ## Storage
//! Instance: `Admin`, `Treasury`, `DepositCount`.
//! Persistent (TTL-tiered): `Deposit(id)`.
//!
//! ## Events
//! `DEP_LOCK`, `DEP_REL`, `DEP_CHG`.
//!
//! ## Migration
//! New independent contract; no prior on-chain state.

use soroban_sdk::xdr::ToXdr;

const DEP_MIN_TTL: u32 = 100_000;
const DEP_MAX_TTL: u32 = 500_000;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Bytes,
    BytesN, Env,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DepositError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    InvalidAmount = 5,
    AlreadyClosed = 6,
    InsufficientBalance = 7,
    Overflow = 8,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositStatus {
    /// Tokens are locked; eligible for release or charge.
    Locked,
    /// Remaining tokens were fully refunded to the patron.
    Released,
    /// Remaining tokens were fully charged to the treasury.
    Charged,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    pub loan_id: BytesN<32>,
    pub patron: Address,
    pub token: Address,
    pub original_amount: i128,
    pub remaining_amount: i128,
    pub status: DepositStatus,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Treasury,
    DepositCount,
    Deposit(BytesN<32>),
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), DepositError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(DepositError::NotInitialized)?;
    if *caller != admin {
        return Err(DepositError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn load_deposit(env: &Env, id: &BytesN<32>) -> Result<Deposit, DepositError> {
    env.storage()
        .persistent()
        .get(&DataKey::Deposit(id.clone()))
        .ok_or(DepositError::NotFound)
}

fn save_deposit(env: &Env, id: &BytesN<32>, deposit: &Deposit) {
    env.storage()
        .persistent()
        .set(&DataKey::Deposit(id.clone()), deposit);
    env.storage().persistent().extend_ttl(
        &DataKey::Deposit(id.clone()),
        DEP_MIN_TTL,
        DEP_MAX_TTL,
    );
}

/// Collision-resistant id derivation (ADR-0001 I3).
fn next_deposit_id(
    env: &Env,
    loan_id: &BytesN<32>,
    patron: &Address,
) -> Result<BytesN<32>, DepositError> {
    let n: u64 = env
        .storage()
        .instance()
        .get(&DataKey::DepositCount)
        .unwrap_or(0u64);
    let next = n.checked_add(1).ok_or(DepositError::Overflow)?;
    env.storage().instance().set(&DataKey::DepositCount, &next);
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    input.append(&Bytes::from_slice(
        env,
        &env.ledger().timestamp().to_be_bytes(),
    ));
    input.append(&Bytes::from_slice(env, &loan_id.to_array()));
    input.append(&patron.to_xdr(env));
    Ok(env.crypto().sha256(&input).into())
}

fn get_treasury(env: &Env) -> Result<Address, DepositError> {
    env.storage()
        .instance()
        .get(&DataKey::Treasury)
        .ok_or(DepositError::NotInitialized)
}

#[contract]
pub struct LibraryDeposits;

#[contractimpl]
impl LibraryDeposits {
    /// One-time bootstrap: sets the admin and treasury address.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
    ) -> Result<(), DepositError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(DepositError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        Ok(())
    }

    /// Patron locks `amount` of `token_addr` as a deposit bound to `loan_id`.
    /// Tokens are transferred from `patron` into the contract. Returns the
    /// deposit id.
    pub fn lock_deposit(
        env: Env,
        patron: Address,
        loan_id: BytesN<32>,
        token_addr: Address,
        amount: i128,
    ) -> Result<BytesN<32>, DepositError> {
        let _admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(DepositError::NotInitialized)?;
        if amount <= 0 {
            return Err(DepositError::InvalidAmount);
        }
        patron.require_auth();
        token::Client::new(&env, &token_addr).transfer(
            &patron,
            &env.current_contract_address(),
            &amount,
        );
        let id = next_deposit_id(&env, &loan_id, &patron)?;
        let deposit = Deposit {
            loan_id: loan_id.clone(),
            patron: patron.clone(),
            token: token_addr,
            original_amount: amount,
            remaining_amount: amount,
            status: DepositStatus::Locked,
        };
        save_deposit(&env, &id, &deposit);
        env.events().publish(
            (symbol_short!("DEP_LOCK"),),
            (id.clone(), loan_id, patron, amount),
        );
        Ok(id)
    }

    /// Admin or patron releases the full remaining deposit back to the patron.
    pub fn release_deposit(
        env: Env,
        caller: Address,
        deposit_id: BytesN<32>,
    ) -> Result<(), DepositError> {
        let mut deposit = load_deposit(&env, &deposit_id)?;
        if deposit.status != DepositStatus::Locked {
            return Err(DepositError::AlreadyClosed);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(DepositError::NotInitialized)?;
        if caller != admin && caller != deposit.patron {
            return Err(DepositError::Unauthorized);
        }
        caller.require_auth();
        let refund = deposit.remaining_amount;
        token::Client::new(&env, &deposit.token).transfer(
            &env.current_contract_address(),
            &deposit.patron,
            &refund,
        );
        deposit.remaining_amount = 0;
        deposit.status = DepositStatus::Released;
        save_deposit(&env, &deposit_id, &deposit);
        env.events()
            .publish((symbol_short!("DEP_REL"),), (deposit_id, refund));
        Ok(())
    }

    /// Admin charges `charge_amount` of the deposit to the treasury. The
    /// remainder stays locked. Returns `InsufficientBalance` if
    /// `charge_amount > remaining_amount`.
    pub fn partial_charge(
        env: Env,
        caller: Address,
        deposit_id: BytesN<32>,
        charge_amount: i128,
    ) -> Result<(), DepositError> {
        require_admin(&env, &caller)?;
        if charge_amount <= 0 {
            return Err(DepositError::InvalidAmount);
        }
        let mut deposit = load_deposit(&env, &deposit_id)?;
        if deposit.status != DepositStatus::Locked {
            return Err(DepositError::AlreadyClosed);
        }
        if charge_amount > deposit.remaining_amount {
            return Err(DepositError::InsufficientBalance);
        }
        let treasury = get_treasury(&env)?;
        token::Client::new(&env, &deposit.token).transfer(
            &env.current_contract_address(),
            &treasury,
            &charge_amount,
        );
        deposit.remaining_amount = deposit
            .remaining_amount
            .checked_sub(charge_amount)
            .ok_or(DepositError::Overflow)?;
        save_deposit(&env, &deposit_id, &deposit);
        env.events()
            .publish((symbol_short!("DEP_CHG"),), (deposit_id, charge_amount));
        Ok(())
    }

    /// Admin charges the full remaining deposit to the treasury and closes the
    /// deposit (`status → Charged`).
    pub fn full_charge(
        env: Env,
        caller: Address,
        deposit_id: BytesN<32>,
    ) -> Result<(), DepositError> {
        require_admin(&env, &caller)?;
        let mut deposit = load_deposit(&env, &deposit_id)?;
        if deposit.status != DepositStatus::Locked {
            return Err(DepositError::AlreadyClosed);
        }
        let remaining = deposit.remaining_amount;
        if remaining <= 0 {
            return Err(DepositError::InvalidAmount);
        }
        let treasury = get_treasury(&env)?;
        token::Client::new(&env, &deposit.token).transfer(
            &env.current_contract_address(),
            &treasury,
            &remaining,
        );
        deposit.remaining_amount = 0;
        deposit.status = DepositStatus::Charged;
        save_deposit(&env, &deposit_id, &deposit);
        env.events()
            .publish((symbol_short!("DEP_CHG"),), (deposit_id, remaining));
        Ok(())
    }

    pub fn get_deposit(env: Env, deposit_id: BytesN<32>) -> Result<Deposit, DepositError> {
        load_deposit(&env, &deposit_id)
    }
}

#[cfg(test)]
mod tests;
