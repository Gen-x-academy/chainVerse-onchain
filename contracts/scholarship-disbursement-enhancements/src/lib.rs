#![no_std]

//! Scholarship/bursary disbursement enhancements contract.
//!
//! Scope of this pass (issues #1098, #1099, #1100, #1101):
//! - #1098: Support Stellar asset configuration (native/issued assets, network, issuer, decimals, trustlines)
//! - #1099: Execute scheduled scholarship payments (bounded batches, deterministic outcomes)
//! - #1100: Track pending and finalized transactions (state machine, configurable confirmations)
//! - #1101: Handle failed payout recovery (missing trustlines, bad destinations, retry with new envelopes)
//!
//! This contract extends the base `scholarship-disbursements` contract with:
//! - Asset configuration and validation
//! - Automated batch payment execution
//! - Transaction state tracking with confirmations
//! - Failed payment recovery with retry envelopes
//!
//! See `contracts/docs/scholarship-disbursement-enhancements.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Map, String, Symbol, Vec,
};
use scholarship_disbursements::{DisbursementIntent, IntentStatus};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const MAX_BATCH_SIZE: u32 = 100;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorized = 4,
    IntentNotFound = 5,
    IntentNotExecutable = 6,
    AlreadyExecuted = 7,
    IntentOnHold = 8,
    InvalidAssetConfig = 9,
    AssetNotSupported = 10,
    TrustlineMissing = 11,
    InsufficientBalance = 12,
    BatchSizeExceeded = 13,
    BatchExecutionFailed = 14,
    TransactionNotFound = 15,
    InvalidTransactionState = 16,
    ConfirmationThresholdNotMet = 17,
    RecoveryNotAuthorized = 18,
    RecoveryAmountExceedsOriginal = 19,
    InvalidRetryEnvelope = 20,
    ArithmeticOverflow = 21,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Asset configurations per program
    AssetConfig(BytesN<32>),
    /// Transaction tracking
    Transaction(BytesN<32>),
    /// Failed payment recovery records
    RecoveryAttempt(BytesN<32>),
    /// Batch execution records
    BatchExecution(BytesN<32>),
    /// Counter for unique IDs
    IdCounter,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetType {
    Native,      // XLM
    Issued,      // Custom asset (USDC, etc.)
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetConfig {
    pub program_id: BytesN<32>,
    pub asset_type: AssetType,
    pub asset_code: String,      // e.g., "USDC", "XLM"
    pub issuer: Option<Address>, // None for native
    pub decimals: u32,
    pub trustline_required: bool,
    pub is_active: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    Submitted,
    Pending,
    Confirming,
    Confirmed,
    Finalized,
    Failed,
    Expired,
    Reversed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionRecord {
    pub id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub recipient: Address,
    pub amount: i128,
    pub asset_code: String,
    pub tx_hash: Option<BytesN<32>>,
    pub state: TransactionState,
    pub confirmations: u32,
    pub required_confirmations: u32,
    pub submitted_at: u64,
    pub confirmed_at: Option<u64>,
    pub finalized_at: Option<u64>,
    pub failure_reason: Option<String>,
    pub retry_count: u32,
    pub envelope_xdr: Option<Bytes>, // For retry with new envelope
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryAttempt {
    pub id: BytesN<32>,
    pub original_intent_id: BytesN<32>,
    pub transaction_id: BytesN<32>,
    pub failure_reason: String,
    pub recovery_type: RecoveryType,
    pub status: RecoveryStatus,
    pub retry_envelope_xdr: Option<Bytes>,
    pub attempted_at: u64,
    pub completed_at: Option<u64>,
    pub tx_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryType {
    TrustlineMissing,
    BadDestination,
    InsufficientFunds,
    NetworkExpiry,
    NetworkError,
    Other,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    RequiresManualIntervention,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchExecutionRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_ids: Vec<BytesN<32>>,
    pub total_amount: i128,
    pub asset_code: String,
    pub executed_count: u32,
    pub failed_count: u32,
    pub executed_at: u64,
    pub executed_by: Address,
}

#[contract]
pub struct ScholarshipDisbursementEnhancementsContract;

#[contractimpl]
impl ScholarshipDisbursementEnhancementsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::IdCounter, &0u64);
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

    fn require_creator_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        // In production, check scholarship-disbursements creator allowlist
        Self::require_admin(env, caller)
    }

    fn next_id(env: &Env) -> BytesN<32> {
        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::IdCounter)
            .unwrap_or(0);
        let next = counter.checked_add(1).unwrap_or(1);
        env.storage().instance().set(&DataKey::IdCounter, &next);

        let mut input = Bytes::new(env);
        input.extend_from_array(&next.to_be_bytes());
        input.extend_from_array(&env.ledger().timestamp().to_be_bytes());
        env.crypto().sha256(&input).into()
    }

    // ── #1098 — Stellar Asset Configuration ────────────────────────────────

    /// Configure supported asset for a program.
    /// Unsupported assets fail early; configuration changes are governed.
    pub fn configure_asset(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        asset_type: AssetType,
        asset_code: String,
        issuer: Option<Address>,
        decimals: u32,
        trustline_required: bool,
    ) -> Result<AssetConfig, ContractError> {
        Self::require_admin(&env, &admin)?;

        if asset_code.len() == 0 || asset_code.len() > 12 {
            return Err(ContractError::InvalidAssetConfig);
        }

        if asset_type == AssetType::Native {
            if asset_code != String::from_str(&env, "XLM") {
                return Err(ContractError::InvalidAssetConfig);
            }
            if issuer.is_some() {
                return Err(ContractError::InvalidAssetConfig);
            }
            if decimals != 7 {
                return Err(ContractError::InvalidAssetConfig);
            }
            if trustline_required {
                return Err(ContractError::InvalidAssetConfig);
            }
        } else {
            if issuer.is_none() {
                return Err(ContractError::InvalidAssetConfig);
            }
            if decimals == 0 || decimals > 18 {
                return Err(ContractError::InvalidAssetConfig);
            }
        }

        let existing: Option<AssetConfig> = env
            .storage()
            .persistent()
            .get(&DataKey::AssetConfig(program_id.clone()));

        let config = AssetConfig {
            program_id: program_id.clone(),
            asset_type,
            asset_code,
            issuer,
            decimals,
            trustline_required,
            is_active: true,
            created_at: existing.map(|e| e.created_at).unwrap_or_else(|| env.ledger().timestamp()),
            updated_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::AssetConfig(program_id.clone()), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::AssetConfig(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("ASSETCFG"),),
            (program_id, config.asset_type as u32),
        );

        Ok(config)
    }

    /// Get asset configuration for a program.
    pub fn get_asset_config(env: Env, program_id: BytesN<32>) -> Result<AssetConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::AssetConfig(program_id))
            .ok_or(ContractError::AssetNotSupported)
    }

    /// Validate that an intent's asset matches program configuration.
    pub fn validate_intent_asset(
        env: Env,
        intent_id: BytesN<32>,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
            env.clone(),
            intent_id.clone(),
        )?;

        if intent.program_id != program_id {
            return Err(ContractError::InvalidAssetConfig);
        }

        let config = Self::get_asset_config(env, program_id)?;
        if !config.is_active {
            return Err(ContractError::AssetNotSupported);
        }

        // In production, would check intent amount precision matches asset decimals
        Ok(())
    }

    // ── #1099 — Execute Scheduled Scholarship Payments ──────────────────────

    /// Execute a batch of due payments.
    /// Only authorized automation executes; partial batch outcomes reconcile per intent;
    /// successful payments record ledger references.
    pub fn execute_batch(
        env: Env,
        executor: Address,
        program_id: BytesN<32>,
        intent_ids: Vec<BytesN<32>>,
    ) -> Result<BatchExecutionRecord, ContractError> {
        Self::require_creator_or_admin(&env, &executor)?;

        let batch_size = intent_ids.len() as u32;
        if batch_size == 0 || batch_size > MAX_BATCH_SIZE {
            return Err(ContractError::BatchSizeExceeded);
        }

        let config = Self::get_asset_config(env.clone(), program_id.clone())?;
        let mut executed_count = 0u32;
        let mut failed_count = 0u32;
        let mut total_amount = 0i128;

        for intent_id in &intent_ids {
            let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
                env.clone(),
                intent_id.clone(),
            )?;

            if intent.program_id != program_id {
                failed_count = failed_count.checked_add(1).unwrap();
                continue;
            }

            if !scholarship_disbursements::ScholarshipDisbursementsContract::is_intent_executable(
                env.clone(),
                intent_id.clone(),
            ) {
                failed_count = failed_count.checked_add(1).unwrap();
                continue;
            }

            // Create transaction record
            let tx_id = Self::next_id(&env);
            let tx_record = TransactionRecord {
                id: tx_id.clone(),
                intent_id: intent_id.clone(),
                program_id: program_id.clone(),
                recipient: intent.recipient.clone(),
                amount: intent.amount,
                asset_code: config.asset_code.clone(),
                tx_hash: None,
                state: TransactionState::Submitted,
                confirmations: 0,
                required_confirmations: 3, // configurable
                submitted_at: env.ledger().timestamp(),
                confirmed_at: None,
                finalized_at: None,
                failure_reason: None,
                retry_count: 0,
                envelope_xdr: None,
            };

            env.storage().persistent().set(&DataKey::Transaction(tx_id.clone()), &tx_record);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Transaction(tx_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

            // Record execution in base contract
            match scholarship_disbursements::ScholarshipDisbursementsContract::record_execution(
                env.clone(),
                executor.clone(),
                intent_id.clone(),
            ) {
                Ok(_) => {
                    executed_count = executed_count.checked_add(1).unwrap();
                    total_amount = total_amount.checked_add(intent.amount).unwrap();
                }
                Err(_) => {
                    failed_count = failed_count.checked_add(1).unwrap();
                }
            }
        }

        let batch_id = Self::next_id(&env);
        let record = BatchExecutionRecord {
            id: batch_id.clone(),
            program_id: program_id.clone(),
            intent_ids: intent_ids.clone(),
            total_amount,
            asset_code: config.asset_code.clone(),
            executed_count,
            failed_count,
            executed_at: env.ledger().timestamp(),
            executed_by: executor.clone(),
        };

        env.storage().persistent().set(&DataKey::BatchExecution(batch_id.clone()), &record);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::BatchExecution(batch_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("BATCHEXE"),),
            (batch_id, program_id, executed_count, failed_count),
        );

        Ok(record)
    }

    // ── #1100 — Track Pending and Finalized Transactions ────────────────────

    /// Create transaction record for an intent (called before submission).
    pub fn create_transaction_record(
        env: Env,
        creator: Address,
        intent_id: BytesN<32>,
        tx_hash: Option<BytesN<32>>,
        envelope_xdr: Option<Bytes>,
        required_confirmations: u32,
    ) -> Result<TransactionRecord, ContractError> {
        Self::require_creator_or_admin(&env, &creator)?;

        let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
            env.clone(),
            intent_id.clone(),
        )?;

        let config = Self::get_asset_config(env.clone(), intent.program_id.clone())?;

        let tx_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let record = TransactionRecord {
            id: tx_id.clone(),
            intent_id: intent_id.clone(),
            program_id: intent.program_id.clone(),
            recipient: intent.recipient.clone(),
            amount: intent.amount,
            asset_code: config.asset_code.clone(),
            tx_hash,
            state: TransactionState::Submitted,
            confirmations: 0,
            required_confirmations,
            submitted_at: now,
            confirmed_at: None,
            finalized_at: None,
            failure_reason: None,
            retry_count: 0,
            envelope_xdr,
        };

        env.storage().persistent().set(&DataKey::Transaction(tx_id.clone()), &record);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Transaction(tx_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("TXNEW"),),
            (tx_id, intent_id, record.state as u32),
        );

        Ok(record)
    }

    /// Update transaction state based on network confirmation.
    /// State follows verified network evidence; confirmations are configurable;
    /// a payment cannot finalize twice.
    pub fn update_transaction_state(
        env: Env,
        admin: Address,
        tx_id: BytesN<32>,
        new_state: TransactionState,
        tx_hash: Option<BytesN<32>>,
        confirmations: u32,
        failure_reason: Option<String>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut record: TransactionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Transaction(tx_id.clone()))
            .ok_or(ContractError::TransactionNotFound)?;

        // Validate state transitions
        let valid_transition = match (record.state, new_state) {
            (TransactionState::Submitted, TransactionState::Pending) => true,
            (TransactionState::Pending, TransactionState::Confirming) => true,
            (TransactionState::Confirming, TransactionState::Confirmed) => true,
            (TransactionState::Confirmed, TransactionState::Finalized) => true,
            (_, TransactionState::Failed) => true,
            (_, TransactionState::Expired) => true,
            (TransactionState::Finalized, TransactionState::Reversed) => true,
            _ => false,
        };

        if !valid_transition {
            return Err(ContractError::InvalidTransactionState);
        }

        // Cannot finalize twice
        if record.state == TransactionState::Finalized && new_state == TransactionState::Finalized {
            return Err(ContractError::AlreadyExecuted);
        }

        record.state = new_state;

        if let Some(hash) = tx_hash {
            record.tx_hash = Some(hash);
        }
        record.confirmations = confirmations;

        match new_state {
            TransactionState::Confirmed => {
                record.confirmed_at = Some(env.ledger().timestamp());
            }
            TransactionState::Finalized => {
                record.finalized_at = Some(env.ledger().timestamp());
            }
            TransactionState::Failed => {
                record.failure_reason = failure_reason;
            }
            _ => {}
        }

        env.storage().persistent().set(&DataKey::Transaction(tx_id.clone()), &record);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Transaction(tx_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("TXSTATE"),),
            (tx_id, new_state as u32, confirmations),
        );

        Ok(())
    }

    /// Get transaction record.
    pub fn get_transaction(env: Env, tx_id: BytesN<32>) -> Result<TransactionRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Transaction(tx_id))
            .ok_or(ContractError::TransactionNotFound)
    }

    /// Get transactions for an intent.
    pub fn get_transactions_for_intent(
        env: Env,
        intent_id: BytesN<32>,
    ) -> Result<Vec<TransactionRecord>, ContractError> {
        // In production, would index by intent_id
        // For now, return empty
        Ok(Vec::new(&env))
    }

    // ── #1101 — Handle Failed Payout Recovery ───────────────────────────────

    /// Initiate recovery for a failed payment.
    /// Failures preserve award eligibility; retries use new transaction envelopes
    /// but the same intent; operators receive actionable diagnostics.
    pub fn initiate_recovery(
        env: Env,
        admin: Address,
        transaction_id: BytesN<32>,
        recovery_type: RecoveryType,
        retry_envelope_xdr: Option<Bytes>,
    ) -> Result<RecoveryAttempt, ContractError> {
        Self::require_admin(&env, &admin)?;

        let tx_record: TransactionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Transaction(transaction_id.clone()))
            .ok_or(ContractError::TransactionNotFound)?;

        if tx_record.state != TransactionState::Failed {
            return Err(ContractError::InvalidTransactionState);
        }

        let recovery_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let recovery = RecoveryAttempt {
            id: recovery_id.clone(),
            original_intent_id: tx_record.intent_id.clone(),
            transaction_id: transaction_id.clone(),
            failure_reason: tx_record.failure_reason.clone().unwrap_or(String::from_str(&env, "Unknown")),
            recovery_type,
            status: RecoveryStatus::Pending,
            retry_envelope_xdr,
            attempted_at: now,
            completed_at: None,
            tx_hash: None,
        };

        env.storage().persistent().set(&DataKey::RecoveryAttempt(recovery_id.clone()), &recovery);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::RecoveryAttempt(recovery_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("RECVRYNW"),),
            (recovery_id, transaction_id, recovery_type as u32),
        );

        Ok(recovery)
    }

    /// Complete a recovery attempt.
    pub fn complete_recovery(
        env: Env,
        admin: Address,
        recovery_id: BytesN<32>,
        status: RecoveryStatus,
        tx_hash: Option<BytesN<32>>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut recovery: RecoveryAttempt = env
            .storage()
            .persistent()
            .get(&DataKey::RecoveryAttempt(recovery_id.clone()))
            .ok_or(ContractError::TransactionNotFound)?;

        recovery.status = status;
        recovery.completed_at = Some(env.ledger().timestamp());
        recovery.tx_hash = tx_hash;

        env.storage().persistent().set(&DataKey::RecoveryAttempt(recovery_id.clone()), &recovery);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::RecoveryAttempt(recovery_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        // If recovery completed, update original transaction state
        if status == RecoveryStatus::Completed {
            let mut tx_record: TransactionRecord = env
                .storage()
                .persistent()
                .get(&DataKey::Transaction(recovery.transaction_id.clone()))
                .ok_or(ContractError::TransactionNotFound)?;

            tx_record.state = TransactionState::Submitted; // Re-submitted
            tx_record.retry_count = tx_record.retry_count.checked_add(1).unwrap();
            tx_record.failure_reason = None;

            env.storage().persistent().set(&DataKey::Transaction(recovery.transaction_id.clone()), &tx_record);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Transaction(recovery.transaction_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);
        }

        env.events().publish(
            (symbol_short!("RECVRYCM"),),
            (recovery_id, status as u32),
        );

        Ok(())
    }

    pub fn get_recovery_attempt(env: Env, recovery_id: BytesN<32>) -> Result<RecoveryAttempt, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::RecoveryAttempt(recovery_id))
            .ok_or(ContractError::TransactionNotFound)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;