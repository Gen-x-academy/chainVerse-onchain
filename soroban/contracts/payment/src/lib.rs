//! ChainVerse Academy – Soroban payment contract.
//!
//! Implements the administrative configuration boundary defined in issue
//! #914 (single-use initialisation, administrator/treasury management,
//! supported-asset CRUD, course payment-configuration CRUD), the authorized
//! purchase execution defined in issue #915 (SAC token transfer,
//! business-level payment idempotency, enrollment, revenue split records),
//! and the per-asset revenue accounting and pull-based withdrawals defined
//! in issue #916 (isolated instructor/platform balances per asset,
//! instructor_withdraw, platform_withdraw, balance queries, withdrawal events).
#![no_std]

mod errors;
mod events;
mod purchase;
mod storage;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_purchase;
#[cfg(test)]
mod test_withdrawal;

pub use errors::ContractError;

use chainverse_types::{
    AssetConfig, CourseConfig, PaymentRecord, WithdrawalRecord, CONTRACT_VERSION,
    MAX_FEE_BASIS_POINTS,
};
use soroban_sdk::{contract, contractimpl, token, Address, Env, String as SorobanString, Symbol};

use storage::{
    is_asset_enabled, is_initialized, read_admin, read_asset_config, read_course_config, read_fee,
    read_instructor_balance_asset, read_platform_balance_asset, read_treasury, write_admin,
    write_asset_config, write_course_config, write_fee, write_refund_window, write_treasury,
};

// ─── Contract ────────────────────────────────────────────────────────────────

#[contract]
pub struct PaymentContract;

#[contractimpl]
impl PaymentContract {
    // ── Initialisation ────────────────────────────────────────────────────

    /// Initialise the contract exactly once.
    ///
    /// Stores `admin`, `treasury`, `fee_bps` (platform fee in basis points),
    /// and `refund_window_seconds` in instance storage and emits the
    /// corresponding configuration events.
    ///
    /// # Errors
    /// - [`ContractError::AlreadyInitialized`] – called a second time.
    /// - [`ContractError::InvalidAddress`] – `admin` or `treasury` is a
    ///   zero-equivalent value (Soroban disallows the zero address at the
    ///   type level, so the check is structural; both must be valid
    ///   `Address` values).
    /// - [`ContractError::InvalidFee`] – `fee_bps` exceeds
    ///   `MAX_FEE_BASIS_POINTS` (2 000).
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        fee_bps: u32,
        refund_window_seconds: u64,
    ) -> Result<(), ContractError> {
        if is_initialized(&env) {
            return Err(ContractError::AlreadyInitialized);
        }
        if fee_bps > MAX_FEE_BASIS_POINTS {
            return Err(ContractError::InvalidFee);
        }

        write_admin(&env, &admin);
        write_treasury(&env, &treasury);
        write_fee(&env, fee_bps);
        write_refund_window(&env, refund_window_seconds);

        events::admin_set(&env, &admin);
        events::treasury_set(&env, &treasury);
        events::fee_set(&env, fee_bps);

        Ok(())
    }

    // ── Admin / treasury setters ──────────────────────────────────────────

    /// Replace the contract administrator.
    ///
    /// Requires `current_admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`] – contract not yet initialised.
    /// - [`ContractError::NotAdmin`] – `caller` does not match the stored
    ///   administrator address.
    pub fn set_admin(env: Env, caller: Address, new_admin: Address) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        write_admin(&env, &new_admin);
        events::admin_set(&env, &new_admin);
        Ok(())
    }

    /// Replace the platform treasury address.
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    pub fn set_treasury(
        env: Env,
        caller: Address,
        new_treasury: Address,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        write_treasury(&env, &new_treasury);
        events::treasury_set(&env, &new_treasury);
        Ok(())
    }

    /// Update the global platform fee (in basis points).
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::InvalidFee`] – `fee_bps` > 2 000.
    pub fn set_fee(env: Env, caller: Address, fee_bps: u32) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        if fee_bps > MAX_FEE_BASIS_POINTS {
            return Err(ContractError::InvalidFee);
        }
        write_fee(&env, fee_bps);
        events::fee_set(&env, fee_bps);
        Ok(())
    }

    // ── Asset CRUD ────────────────────────────────────────────────────────

    /// Add a new supported asset or overwrite an existing one.
    ///
    /// Idempotent – calling with the same address again updates the entry.
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    pub fn add_asset(
        env: Env,
        caller: Address,
        asset: Address,
        enabled: bool,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        let config = AssetConfig {
            asset: asset.clone(),
            enabled,
        };
        write_asset_config(&env, &asset, &config);
        events::asset_configured(&env, &asset, enabled);
        Ok(())
    }

    /// Enable a previously added asset so it can be used for payment.
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::AssetNotFound`] – the asset has never been added.
    pub fn enable_asset(env: Env, caller: Address, asset: Address) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        let mut config = read_asset_config(&env, &asset).ok_or(ContractError::AssetNotFound)?;
        config.enabled = true;
        write_asset_config(&env, &asset, &config);
        events::asset_configured(&env, &asset, true);
        Ok(())
    }

    /// Disable an asset so it can no longer be used for payment.
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::AssetNotFound`]
    pub fn disable_asset(env: Env, caller: Address, asset: Address) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        let mut config = read_asset_config(&env, &asset).ok_or(ContractError::AssetNotFound)?;
        config.enabled = false;
        write_asset_config(&env, &asset, &config);
        events::asset_configured(&env, &asset, false);
        Ok(())
    }

    // ── Course CRUD ───────────────────────────────────────────────────────

    /// Add a new course payment configuration or overwrite an existing one.
    ///
    /// Stores `price`, `asset`, `instructor`, `fee_bps`, and `active` flag
    /// for the given `course_id`.  The asset must have been registered via
    /// `add_asset` and must currently be enabled.
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::InvalidAmount`] – `price` is zero or negative.
    /// - [`ContractError::AssetNotEnabled`] – asset not registered or disabled.
    /// - [`ContractError::InvalidFee`] – `fee_bps` > 2 000.
    #[allow(clippy::too_many_arguments)]
    pub fn add_course(
        env: Env,
        caller: Address,
        course_id: Symbol,
        price: i128,
        asset: Address,
        instructor: Address,
        fee_bps: u32,
        active: bool,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        if price <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if !is_asset_enabled(&env, &asset) {
            return Err(ContractError::AssetNotEnabled);
        }
        if fee_bps > MAX_FEE_BASIS_POINTS {
            return Err(ContractError::InvalidFee);
        }

        let config = CourseConfig {
            course_id: course_id.clone(),
            price,
            asset: asset.clone(),
            instructor: instructor.clone(),
            fee_bps,
            active,
        };
        write_course_config(&env, &course_id, &config);
        events::course_configured(
            &env,
            &course_id,
            price,
            &asset,
            &instructor,
            fee_bps,
            active,
        );
        Ok(())
    }

    /// Update the price, asset, instructor, fee, or active state of a course.
    ///
    /// The course must already exist.  Use `add_course` to create a new entry.
    /// The new asset (if changed) must be enabled.
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::CourseNotFound`]
    /// - [`ContractError::InvalidAmount`]
    /// - [`ContractError::AssetNotEnabled`]
    /// - [`ContractError::InvalidFee`]
    #[allow(clippy::too_many_arguments)]
    pub fn update_course(
        env: Env,
        caller: Address,
        course_id: Symbol,
        price: i128,
        asset: Address,
        instructor: Address,
        fee_bps: u32,
        active: bool,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        // Course must already exist.
        read_course_config(&env, &course_id).ok_or(ContractError::CourseNotFound)?;

        if price <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if !is_asset_enabled(&env, &asset) {
            return Err(ContractError::AssetNotEnabled);
        }
        if fee_bps > MAX_FEE_BASIS_POINTS {
            return Err(ContractError::InvalidFee);
        }

        let config = CourseConfig {
            course_id: course_id.clone(),
            price,
            asset: asset.clone(),
            instructor: instructor.clone(),
            fee_bps,
            active,
        };
        write_course_config(&env, &course_id, &config);
        events::course_configured(
            &env,
            &course_id,
            price,
            &asset,
            &instructor,
            fee_bps,
            active,
        );
        Ok(())
    }

    /// Set a course to active (open for purchase).
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::CourseNotFound`]
    pub fn activate_course(
        env: Env,
        caller: Address,
        course_id: Symbol,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        let mut config =
            read_course_config(&env, &course_id).ok_or(ContractError::CourseNotFound)?;
        config.active = true;
        let asset = config.asset.clone();
        let instructor = config.instructor.clone();
        let price = config.price;
        let fee_bps = config.fee_bps;
        write_course_config(&env, &course_id, &config);
        events::course_configured(&env, &course_id, price, &asset, &instructor, fee_bps, true);
        Ok(())
    }

    /// Set a course to inactive (closed for purchase).
    ///
    /// Requires `admin.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`]
    /// - [`ContractError::CourseNotFound`]
    pub fn deactivate_course(
        env: Env,
        caller: Address,
        course_id: Symbol,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        let mut config =
            read_course_config(&env, &course_id).ok_or(ContractError::CourseNotFound)?;
        config.active = false;
        let asset = config.asset.clone();
        let instructor = config.instructor.clone();
        let price = config.price;
        let fee_bps = config.fee_bps;
        write_course_config(&env, &course_id, &config);
        events::course_configured(&env, &course_id, price, &asset, &instructor, fee_bps, false);
        Ok(())
    }

    // ── Query methods ─────────────────────────────────────────────────────

    /// Return the current administrator address.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        read_admin(&env)
    }

    /// Return the current treasury address.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    pub fn get_treasury(env: Env) -> Result<Address, ContractError> {
        read_treasury(&env)
    }

    /// Return the current platform fee in basis points.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    pub fn get_fee(env: Env) -> Result<u32, ContractError> {
        read_fee(&env)
    }

    /// Return the asset configuration for `asset`, or `None` if not added.
    pub fn get_asset_config(env: Env, asset: Address) -> Option<AssetConfig> {
        read_asset_config(&env, &asset)
    }

    /// Return `true` if `asset` is registered and currently enabled.
    pub fn is_asset_enabled(env: Env, asset: Address) -> bool {
        is_asset_enabled(&env, &asset)
    }

    /// Return the course payment configuration, or `None` if absent.
    pub fn get_course_config(env: Env, course_id: Symbol) -> Option<CourseConfig> {
        read_course_config(&env, &course_id)
    }

    /// Return `true` if the course exists and its `active` flag is set.
    pub fn is_course_active(env: Env, course_id: Symbol) -> bool {
        read_course_config(&env, &course_id)
            .map(|c| c.active)
            .unwrap_or(false)
    }

    // ── Purchase execution (issue #915) ───────────────────────────────────

    /// Pay for a course and enroll the student, atomically.
    ///
    /// The exact configured price is transferred from `student` to this
    /// contract through the Stellar Asset Contract configured for the
    /// course; the gross payment plus its fee/instructor split are stored,
    /// the enrollment is created, and the frozen `PYMT_RCD` event is emitted.
    ///
    /// `payment_id` is a caller-supplied business idempotency key of up to
    /// 32 bytes (clients typically derive it from `(student, course_id)`).
    /// It is reserved globally: a second purchase reusing the ID fails with
    /// [`ContractError::DuplicatePaymentId`] regardless of arguments, and an
    /// empty ID fails with [`ContractError::InvalidPaymentId`].
    ///
    /// Requires `student.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::InvalidPaymentId`] – empty payment ID.
    /// - [`ContractError::CourseNotFound`] – course has no configuration.
    /// - [`ContractError::CourseInactive`] – course is not open for purchase.
    /// - [`ContractError::AssetNotEnabled`] – asset unregistered or disabled.
    /// - [`ContractError::AlreadyEnrolled`] – student already owns the course.
    /// - [`ContractError::DuplicatePaymentId`] – payment ID already reserved.
    /// - [`ContractError::PaymentFailed`] – SAC transfer failed (e.g.
    ///   insufficient balance) or the token contract panicked.
    pub fn pay_for_course(
        env: Env,
        student: Address,
        course_id: Symbol,
        payment_id: Symbol,
    ) -> Result<(), ContractError> {
        purchase::execute_purchase(&env, &student, &course_id, &payment_id)
    }

    // ── Payment / enrollment queries ─────────────────────────────────────

    /// Return `true` if `student` is enrolled in `course_id`.
    pub fn is_enrolled(env: Env, student: Address, course_id: Symbol) -> bool {
        storage::has_enrollment(&env, &student, &course_id)
    }

    /// Return the full payment receipt for `(student, course_id)`, or `None`.
    ///
    /// The receipt contains the gross amount and the persisted split
    /// allocation (`fee_amount + instructor_amount == amount`).
    pub fn get_payment_record(
        env: Env,
        student: Address,
        course_id: Symbol,
    ) -> Option<PaymentRecord> {
        storage::read_payment_record(&env, &student, &course_id)
    }

    /// Return the payment receipt identified by its globally unique
    /// business `payment_id`, or `None`.
    pub fn get_payment_by_id(env: Env, payment_id: Symbol) -> Option<PaymentRecord> {
        storage::read_payment_record_by_id(&env, &payment_id)
    }

    /// Return the instructor's claimable balance for a specific asset (zero when never credited).
    ///
    /// Balances are isolated by Stellar Asset Contract address.  Two calls
    /// with different `asset` values may return different amounts for the same
    /// instructor.
    pub fn get_instructor_balance(env: Env, instructor: Address, asset: Address) -> i128 {
        storage::read_instructor_balance_asset(&env, &instructor, &asset)
    }

    /// Return the platform's claimable balance for a specific asset (zero when
    /// never credited).
    ///
    /// Accumulates the platform fee portion of every purchase denominated in
    /// `asset`.
    pub fn get_platform_balance(env: Env, asset: Address) -> i128 {
        storage::read_platform_balance_asset(&env, &asset)
    }

    // ── Withdrawal entrypoints (issue #916) ───────────────────────────────

    /// Withdraw all or part of the instructor's claimable balance for `asset`.
    ///
    /// Applies checks-effects-interactions:
    /// 1. Verify the instructor's authorization.
    /// 2. Load and validate the current balance.
    /// 3. Reduce the stored balance **before** the token transfer.
    /// 4. Execute the SAC transfer; on failure, the whole invocation is
    ///    rolled back by the Soroban host, restoring the original balance.
    ///
    /// Requires `instructor.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::InvalidAmount`] – `amount` ≤ 0.
    /// - [`ContractError::InsufficientBalance`] – balance < `amount`.
    /// - [`ContractError::TransferFailed`] – SAC transfer failed.
    pub fn instructor_withdraw(
        env: Env,
        instructor: Address,
        asset: Address,
        amount: i128,
    ) -> Result<WithdrawalRecord, ContractError> {
        // ── 1. Guard: contract must be initialized ──────────────────────
        // read_admin is a lightweight existence check; we don't need the value.
        let _ = storage::read_admin(&env)?;

        // ── 2. Amount validation ────────────────────────────────────────
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // ── 3. Authorization ────────────────────────────────────────────
        instructor.require_auth();

        // ── 4. Balance check ────────────────────────────────────────────
        let balance = read_instructor_balance_asset(&env, &instructor, &asset);
        if balance < amount {
            return Err(ContractError::InsufficientBalance);
        }

        // ── 5. Effects: reduce balance before the transfer ──────────────
        storage::write_instructor_balance_asset(&env, &instructor, &asset, balance - amount);

        // ── 6. Interactions: SAC transfer ────────────────────────────────
        let token_client = token::Client::new(&env, &asset);
        let escrow = env.current_contract_address();
        token_client
            .try_transfer(&escrow, &instructor, &amount)
            .map_err(|_| ContractError::TransferFailed)?
            .map_err(|_| ContractError::TransferFailed)?;

        // ── 7. Event & return record ─────────────────────────────────────
        let withdrawn_at = env.ledger().timestamp();
        events::withdrawal_processed(&env, &instructor, &asset, amount, withdrawn_at);

        Ok(WithdrawalRecord {
            recipient: instructor,
            asset,
            amount,
            withdrawn_at,
        })
    }

    /// Withdraw all or part of the platform's claimable balance for `asset`.
    ///
    /// Only the configured treasury address may call this method.
    ///
    /// Applies checks-effects-interactions in the same order as
    /// [`Self::instructor_withdraw`].
    ///
    /// Requires `treasury.require_auth()`.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialized`]
    /// - [`ContractError::NotAdmin`] – caller is not the treasury address.
    /// - [`ContractError::InvalidAmount`] – `amount` ≤ 0.
    /// - [`ContractError::InsufficientBalance`] – balance < `amount`.
    /// - [`ContractError::TransferFailed`] – SAC transfer failed.
    pub fn platform_withdraw(
        env: Env,
        caller: Address,
        asset: Address,
        amount: i128,
    ) -> Result<WithdrawalRecord, ContractError> {
        // ── 1. Guard: contract must be initialized ──────────────────────
        let treasury = storage::read_treasury(&env)?;

        // ── 2. Treasury authorization ────────────────────────────────────
        if caller != treasury {
            return Err(ContractError::NotAdmin);
        }

        // ── 3. Amount validation ────────────────────────────────────────
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // ── 4. Authorization ────────────────────────────────────────────
        caller.require_auth();

        // ── 5. Balance check ────────────────────────────────────────────
        let balance = read_platform_balance_asset(&env, &asset);
        if balance < amount {
            return Err(ContractError::InsufficientBalance);
        }

        // ── 6. Effects: reduce balance before the transfer ──────────────
        storage::write_platform_balance_asset(&env, &asset, balance - amount);

        // ── 7. Interactions: SAC transfer ────────────────────────────────
        let token_client = token::Client::new(&env, &asset);
        let escrow = env.current_contract_address();
        token_client
            .try_transfer(&escrow, &treasury, &amount)
            .map_err(|_| ContractError::TransferFailed)?
            .map_err(|_| ContractError::TransferFailed)?;

        // ── 8. Event & return record ─────────────────────────────────────
        let withdrawn_at = env.ledger().timestamp();
        events::withdrawal_processed(&env, &treasury, &asset, amount, withdrawn_at);

        Ok(WithdrawalRecord {
            recipient: treasury,
            asset,
            amount,
            withdrawn_at,
        })
    }

    /// Return the deployed contract version string.
    pub fn version(env: Env) -> SorobanString {
        SorobanString::from_str(&env, CONTRACT_VERSION)
    }
}
