#![no_std]

//! Licensed-content revenue split contract (issue #993).
//!
//! Lets an admin define a bounded, immutable "split manifest" — a list of
//! recipients and their basis-point shares, summing to exactly 100% — and
//! then atomically distribute an incoming amount of a given asset across
//! that manifest with deterministic rounding. Distribution credits an
//! internal, per-recipient, per-asset balance; recipients withdraw their
//! own balance with their own signature. A per-asset ledger tracks total
//! distributed vs. total withdrawn so liabilities always reconcile:
//! `total_distributed - total_withdrawn == sum of all outstanding balances`
//! for that asset, at every point in time.
//!
//! This contract is bookkeeping only — it does not itself hold token
//! custody or move real assets on `distribute`/`withdraw`. Wiring it to
//! an actual token transfer (so a recipient's `withdraw` call also moves
//! real funds) is a follow-up; see
//! `contracts/docs/royalty-splits.md` for why that's out of scope here
//! and what the accounting still guarantees on its own.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Basis points representing 100% — every manifest's shares must sum to
/// exactly this.
const TOTAL_BPS: u32 = 10_000;

/// Bounded storage: a manifest may name at most this many recipients.
const MAX_RECIPIENTS: u32 = 20;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ManifestAlreadyExists = 4,
    ManifestNotFound = 5,
    EmptyManifest = 6,
    TooManyRecipients = 7,
    MismatchedLengths = 8,
    /// A recipient's share is zero, or the manifest's shares don't sum to
    /// exactly `TOTAL_BPS`.
    InvalidBps = 9,
    InvalidAmount = 10,
    NothingToWithdraw = 11,
    Overflow = 12,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Manifest(BytesN<32>),
    /// Withdrawable balance for (recipient, asset).
    Balance(Address, Symbol),
    /// Per-asset distributed/withdrawn totals.
    AssetLedger(Symbol),
}

/// A bounded, immutable revenue split: `recipients[i]` receives
/// `bps[i]` / `TOTAL_BPS` of every amount distributed under this
/// manifest. `recipients.len() == bps.len()`, and `bps` always sums to
/// exactly `TOTAL_BPS` — both enforced at creation, never re-checked
/// later, because the manifest can never change after creation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitManifest {
    pub recipients: Vec<Address>,
    pub bps: Vec<u32>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssetLedger {
    pub total_distributed: i128,
    pub total_withdrawn: i128,
}

#[contract]
pub struct RoyaltySplitsContract;

#[contractimpl]
impl RoyaltySplitsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    /// Admin-only: create a new, immutable split manifest. Validates that
    /// `recipients` and `bps` are the same, non-zero, bounded length, that
    /// every share is > 0, and that the shares sum to *exactly*
    /// `TOTAL_BPS` (10,000 = 100.00%) — a manifest that under- or
    /// over-allocates can never be persisted.
    pub fn create_manifest(
        env: Env,
        admin: Address,
        manifest_id: BytesN<32>,
        recipients: Vec<Address>,
        bps: Vec<u32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Manifest(manifest_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ManifestAlreadyExists);
        }
        if recipients.is_empty() {
            return Err(ContractError::EmptyManifest);
        }
        if recipients.len() > MAX_RECIPIENTS {
            return Err(ContractError::TooManyRecipients);
        }
        if recipients.len() != bps.len() {
            return Err(ContractError::MismatchedLengths);
        }

        let mut total: u32 = 0;
        for share in bps.iter() {
            if share == 0 {
                return Err(ContractError::InvalidBps);
            }
            total = total.checked_add(share).ok_or(ContractError::Overflow)?;
        }
        if total != TOTAL_BPS {
            return Err(ContractError::InvalidBps);
        }

        let manifest = SplitManifest {
            recipients,
            bps,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &manifest);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("SPLITNEW"),), (manifest_id,));
        Ok(())
    }

    pub fn get_manifest(
        env: Env,
        manifest_id: BytesN<32>,
    ) -> Result<SplitManifest, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Manifest(manifest_id))
            .ok_or(ContractError::ManifestNotFound)
    }

    /// Admin-only: atomically distribute `amount` of `asset` across
    /// `manifest_id`'s recipients. Deterministic rounding: every
    /// recipient but the last gets `amount * bps[i] / TOTAL_BPS`
    /// (integer division, checked), and the *last* recipient gets
    /// whatever remains — `amount` minus every prior share — so the sum
    /// of credited shares always equals `amount` exactly, with no dust
    /// left unaccounted for and no dependency on floating point.
    pub fn distribute(
        env: Env,
        admin: Address,
        manifest_id: BytesN<32>,
        asset: Symbol,
        amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        let manifest = Self::get_manifest(env.clone(), manifest_id.clone())?;
        let n = manifest.recipients.len();

        let mut distributed_so_far: i128 = 0;
        for i in 0..n {
            let recipient = manifest.recipients.get(i).ok_or(ContractError::ManifestNotFound)?;
            let share = if i == n - 1 {
                // Last recipient absorbs the rounding remainder.
                amount
                    .checked_sub(distributed_so_far)
                    .ok_or(ContractError::Overflow)?
            } else {
                let share_bps = manifest.bps.get(i).ok_or(ContractError::ManifestNotFound)?;
                let raw = amount
                    .checked_mul(share_bps as i128)
                    .ok_or(ContractError::Overflow)?;
                raw / (TOTAL_BPS as i128)
            };

            distributed_so_far = distributed_so_far
                .checked_add(share)
                .ok_or(ContractError::Overflow)?;

            let balance_key = DataKey::Balance(recipient.clone(), asset.clone());
            let current: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
            let new_balance = current.checked_add(share).ok_or(ContractError::Overflow)?;
            env.storage().persistent().set(&balance_key, &new_balance);
            env.storage()
                .persistent()
                .extend_ttl(&balance_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }

        let ledger_key = DataKey::AssetLedger(asset.clone());
        let mut ledger: AssetLedger = env
            .storage()
            .persistent()
            .get(&ledger_key)
            .unwrap_or(AssetLedger {
                total_distributed: 0,
                total_withdrawn: 0,
            });
        ledger.total_distributed = ledger
            .total_distributed
            .checked_add(amount)
            .ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&ledger_key, &ledger);
        env.storage()
            .persistent()
            .extend_ttl(&ledger_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("SPLITDST"),),
            (manifest_id, asset, amount),
        );
        Ok(())
    }

    /// Withdraw the caller's entire withdrawable balance for `asset`.
    /// Requires the recipient's own signature — no one else, not even the
    /// admin, can withdraw on their behalf. Returns the withdrawn amount.
    pub fn withdraw(env: Env, recipient: Address, asset: Symbol) -> Result<i128, ContractError> {
        recipient.require_auth();

        let balance_key = DataKey::Balance(recipient.clone(), asset.clone());
        let balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if balance <= 0 {
            return Err(ContractError::NothingToWithdraw);
        }

        env.storage().persistent().set(&balance_key, &0i128);

        let ledger_key = DataKey::AssetLedger(asset.clone());
        let mut ledger: AssetLedger = env
            .storage()
            .persistent()
            .get(&ledger_key)
            .ok_or(ContractError::NothingToWithdraw)?;
        ledger.total_withdrawn = ledger
            .total_withdrawn
            .checked_add(balance)
            .ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&ledger_key, &ledger);

        env.events().publish(
            (soroban_sdk::symbol_short!("SPLITWD"),),
            (recipient, asset, balance),
        );
        Ok(balance)
    }

    pub fn get_balance(env: Env, recipient: Address, asset: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(recipient, asset))
            .unwrap_or(0)
    }

    pub fn get_asset_ledger(env: Env, asset: Symbol) -> AssetLedger {
        env.storage()
            .persistent()
            .get(&DataKey::AssetLedger(asset))
            .unwrap_or(AssetLedger {
                total_distributed: 0,
                total_withdrawn: 0,
            })
    }

    /// `total_distributed - total_withdrawn` for `asset` — the sum of
    /// every recipient's outstanding balance in that asset, without
    /// having to enumerate recipients. This is the reconciliation
    /// invariant the issue asks for, exposed directly.
    pub fn get_outstanding_liability(env: Env, asset: Symbol) -> i128 {
        let ledger = Self::get_asset_ledger(env, asset);
        ledger.total_distributed - ledger.total_withdrawn
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
