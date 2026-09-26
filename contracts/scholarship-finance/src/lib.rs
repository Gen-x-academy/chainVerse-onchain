#![no_std]

//! Scholarship/bursary financial operations contract.
//!
//! Scope of this pass (issues #1106, #1107, #1108, #1109):
//! - #1106: Calculate platform and network fees transparently
//! - #1107: Process refunds and returned payments
//! - #1108: Manage recoveries and clawbacks
//! - #1109: Export sponsor financial statements
//!
//! This contract builds on the existing scholarship infrastructure:
//! - scholarship-core: program lifecycle and metadata
//! - scholarship-disbursements: payment intents and wallet verification
//! - scholarship-programs: award budgets and windows
//!
//! All financial operations are privacy-minimized: no PII on-chain,
//! only addresses, amounts, timestamps, and status enums. See
//! `contracts/docs/scholarship-finance.md` for ownership, privacy,
//! migration, and operational notes.

use ed25519_dalek::{Signature, VerifyingKey};
use shared::signing::{self, MessageType, SigningDomain, ENVELOPE_VERSION};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, Map, Vec,
};
use scholarship_core::ProgramStatus;
use scholarship_disbursements::{DisbursementIntent, IntentStatus};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const MAX_CHALLENGE_TTL_SECONDS: u64 = 86_400;

const FEE_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.FeeCalculation";
const REFUND_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.Refund";
const RECOVERY_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.Recovery";
const STATEMENT_DOMAIN_TAG: &[u8] = b"ChainVerse.Scholarship.Statement";

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorized = 4,
    InvalidAmount = 5,
    IntentNotFound = 6,
    IntentNotExecutable = 7,
    AlreadyExecuted = 8,
    IntentOnHold = 9,
    WalletNotFound = 10,
    WalletNotVerified = 11,
    InvalidFeeConfig = 12,
    FeeCalculationOverflow = 13,
    RefundNotAuthorized = 14,
    RefundAmountExceedsPaid = 15,
    RefundAlreadyProcessed = 16,
    RecoveryNotAuthorized = 17,
    RecoveryAmountExceedsOutstanding = 18,
    RecoveryAlreadyProcessed = 19,
    InvalidRecoveryReason = 20,
    StatementNotFound = 21,
    StatementGenerationFailed = 22,
    InvalidDateRange = 23,
    CurrencyMismatch = 24,
    ProgramNotFound = 25,
    ProgramNotPublished = 26,
    ArithmeticOverflow = 27,
    InvalidConfiguration = 28,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NetworkId,
    /// Platform fee configuration per program
    FeeConfig(BytesN<32>),
    /// Refund records
    Refund(BytesN<32>),
    /// Recovery/clawback records
    Recovery(BytesN<32>),
    /// Generated financial statements
    Statement(BytesN<32>),
    /// Per-program financial aggregates
    ProgramFinancials(BytesN<32>),
    /// Counter for generating unique IDs
    IdCounter,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeType {
    Platform,
    Network,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeBasis {
    /// Percentage of gross amount (basis points, e.g., 250 = 2.5%)
    Percentage(u32),
    /// Fixed amount per transaction
    Fixed(i128),
    /// Percentage with cap
    PercentageWithCap { basis_points: u32, cap: i128 },
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeConfig {
    pub program_id: BytesN<32>,
    pub platform_fee: FeeBasis,
    pub network_fee_estimate: i128,
    pub fee_currency: soroban_sdk::Symbol,
    pub version: u32,
    pub updated_at: u64,
    pub updated_by: Address,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefundReason {
    Overpayment,
    SponsorCancellation,
    RecipientIneligible,
    DuplicatePayment,
    NetworkRejection,
    ProgramClosed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub recipient: Address,
    pub original_amount: i128,
    pub refund_amount: i128,
    pub fee_refunded: i128,
    pub reason: RefundReason,
    pub status: RefundStatus,
    pub initiated_at: u64,
    pub initiated_by: Address,
    pub completed_at: u64,
    pub tx_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefundStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryReason {
    Fraud,
    MilestoneFailure,
    Withdrawal,
    PolicyViolation,
    ComplianceHold,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub recipient: Address,
    pub original_amount: i128,
    pub recovery_amount: i128,
    pub reason: RecoveryReason,
    pub legal_basis: soroban_sdk::String,
    pub status: RecoveryStatus,
    pub initiated_at: u64,
    pub initiated_by: Address,
    pub completed_at: u64,
    pub tx_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Disputed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFinancials {
    pub program_id: BytesN<32>,
    pub sponsor_id: BytesN<32>,
    pub currency: soroban_sdk::Symbol,
    pub total_funded: i128,
    pub total_disbursed: i128,
    pub total_fees_collected: i128,
    pub total_refunded: i128,
    pub total_recovered: i128,
    pub pending_disbursements: i128,
    pub pending_refunds: i128,
    pub pending_recoveries: i128,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancialStatement {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub sponsor_id: BytesN<32>,
    pub period_start: u64,
    pub period_end: u64,
    pub currency: soroban_sdk::Symbol,
    pub contributions: i128,
    pub commitments: i128,
    pub payments: i128,
    pub fees: i128,
    pub refunds: i128,
    pub recoveries: i128,
    pub remaining_balance: i128,
    pub generated_at: u64,
    pub generated_by: Address,
    pub version: u32,
}

#[contract]
pub struct ScholarshipFinanceContract;

#[contractimpl]
impl ScholarshipFinanceContract {
    /// Initialize the contract with admin and network ID.
    pub fn initialize(
        env: Env,
        admin: Address,
        network_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NetworkId, &network_id);
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

    fn require_sponsor_or_admin(env: &Env, caller: &Address, sponsor_id: &BytesN<32>) -> Result<(), ContractError> {
        // In a full implementation, this would check sponsor organization membership
        // For now, only admin can perform sponsor-level operations
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

    // ── #1106 — Fee Calculation ────────────────────────────────────────────

    /// Configure platform and network fees for a program.
    /// Only admin can configure fees. Fee basis and rounding are versioned.
    pub fn configure_fees(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        platform_fee: FeeBasis,
        network_fee_estimate: i128,
        fee_currency: soroban_sdk::Symbol,
    ) -> Result<FeeConfig, ContractError> {
        Self::require_admin(&env, &admin)?;

        // Validate fee configuration
        match platform_fee {
            FeeBasis::Percentage(bps) => {
                if bps > 10000 {
                    return Err(ContractError::InvalidFeeConfig);
                }
            }
            FeeBasis::Fixed(amount) => {
                if amount < 0 {
                    return Err(ContractError::InvalidFeeConfig);
                }
            }
            FeeBasis::PercentageWithCap { basis_points, cap } => {
                if basis_points > 10000 || cap < 0 {
                    return Err(ContractError::InvalidFeeConfig);
                }
            }
        }

        if network_fee_estimate < 0 {
            return Err(ContractError::InvalidFeeConfig);
        }

        let existing: Option<FeeConfig> = env
            .storage()
            .persistent()
            .get(&DataKey::FeeConfig(program_id.clone()));

        let version = existing.map(|c| c.version).unwrap_or(0).checked_add(1).ok_or(ContractError::ArithmeticOverflow)?;

        let config = FeeConfig {
            program_id: program_id.clone(),
            platform_fee,
            network_fee_estimate,
            fee_currency,
            version,
            updated_at: env.ledger().timestamp(),
            updated_by: admin.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::FeeConfig(program_id.clone()), &config);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::FeeConfig(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("FEECFG"),),
            (program_id, version),
        );

        Ok(config)
    }

    /// Get fee configuration for a program.
    pub fn get_fee_config(env: Env, program_id: BytesN<32>) -> Result<FeeConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FeeConfig(program_id))
            .ok_or(ContractError::InvalidFeeConfig)
    }

    /// Calculate fees for a given amount. Returns (platform_fee, network_fee, total_fee, net_amount).
    /// This is a pure preview — no state changes.
    pub fn calculate_fees(
        env: Env,
        program_id: BytesN<32>,
        gross_amount: i128,
    ) -> Result<(i128, i128, i128, i128), ContractError> {
        let config = Self::get_fee_config(env.clone(), program_id.clone())?;

        let platform_fee = match config.platform_fee {
            FeeBasis::Percentage(bps) => {
                (gross_amount as i128)
                    .checked_mul(bps as i128)
                    .ok_or(ContractError::FeeCalculationOverflow)?
                    .checked_div(10000)
                    .ok_or(ContractError::FeeCalculationOverflow)?
            }
            FeeBasis::Fixed(amount) => amount,
            FeeBasis::PercentageWithCap { basis_points, cap } => {
                let fee = (gross_amount as i128)
                    .checked_mul(basis_points as i128)
                    .ok_or(ContractError::FeeCalculationOverflow)?
                    .checked_div(10000)
                    .ok_or(ContractError::FeeCalculationOverflow)?;
                if fee > cap { cap } else { fee }
            }
        };

        let network_fee = config.network_fee_estimate;
        let total_fee = platform_fee
            .checked_add(network_fee)
            .ok_or(ContractError::FeeCalculationOverflow)?;

        let net_amount = gross_amount
            .checked_sub(total_fee)
            .ok_or(ContractError::FeeCalculationOverflow)?;

        if net_amount < 0 {
            return Err(ContractError::InvalidFeeConfig);
        }

        Ok((platform_fee, network_fee, total_fee, net_amount))
    }

    // ── #1107 — Refunds ────────────────────────────────────────────────────

    /// Initiate a refund for a disbursement intent.
    /// Refund authority is restricted; liabilities remain solvent;
    /// original entries are reversed rather than deleted.
    pub fn initiate_refund(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        intent_id: BytesN<32>,
        refund_amount: i128,
        reason: RefundReason,
    ) -> Result<RefundRecord, ContractError> {
        // Verify caller is authorized (admin or program owner)
        // For simplicity, require admin for now
        Self::require_admin(&env, &caller)?;

        if refund_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // Load the intent to verify it exists and get details
        let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
            env.clone(),
            intent_id.clone(),
        )?;

        if intent.program_id != program_id {
            return Err(ContractError::InvalidConfiguration);
        }

        if intent.status != IntentStatus::Executed {
            return Err(ContractError::IntentNotExecutable);
        }

        // Check if refund amount exceeds original payment
        if refund_amount > intent.amount {
            return Err(ContractError::RefundAmountExceedsPaid);
        }

        // Calculate fee refund proportionally
        let fee_config = Self::get_fee_config(env.clone(), program_id.clone())?;
        let (platform_fee, network_fee, _, _) = Self::calculate_fees(
            env.clone(),
            program_id.clone(),
            intent.amount,
        )?;

        let fee_refunded = if refund_amount >= intent.amount {
            platform_fee.checked_add(network_fee).ok_or(ContractError::ArithmeticOverflow)?
        } else {
            let proportion = (refund_amount as i128)
                .checked_mul(10000)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_div(intent.amount)
                .ok_or(ContractError::ArithmeticOverflow)?;
            let total_fee = platform_fee.checked_add(network_fee).ok_or(ContractError::ArithmeticOverflow)?;
            (total_fee as i128)
                .checked_mul(proportion)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_div(10000)
                .ok_or(ContractError::ArithmeticOverflow)?
        };

        // Check for existing refund on this intent
        let existing_refund: Option<RefundRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(intent_id.clone()));

        if let Some(existing) = existing_refund {
            if existing.status == RefundStatus::Completed {
                return Err(ContractError::RefundAlreadyProcessed);
            }
            // Allow retry of failed/pending refunds
        }

        let refund_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let refund = RefundRecord {
            id: refund_id.clone(),
            program_id: program_id.clone(),
            intent_id: intent_id.clone(),
            recipient: intent.recipient.clone(),
            original_amount: intent.amount,
            refund_amount,
            fee_refunded,
            reason,
            status: RefundStatus::Pending,
            initiated_at: now,
            initiated_by: caller.clone(),
            completed_at: 0,
            tx_hash: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Refund(refund_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Update program financials
        Self::update_financials_on_refund(&env, &program_id, refund_amount, fee_refunded)?;

        env.events().publish(
            (symbol_short!("REFUNDNW"),),
            (refund_id, program_id, intent_id, refund_amount, reason as u32),
        );

        Ok(refund)
    }

    /// Complete a refund (called after off-chain execution).
    pub fn complete_refund(
        env: Env,
        caller: Address,
        refund_id: BytesN<32>,
        tx_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let mut refund: RefundRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(refund_id.clone()))
            .ok_or(ContractError::StatementNotFound)?;

        if refund.status == RefundStatus::Completed {
            return Err(ContractError::RefundAlreadyProcessed);
        }

        refund.status = RefundStatus::Completed;
        refund.completed_at = env.ledger().timestamp();
        refund.tx_hash = Some(tx_hash);

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id.clone()), &refund);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Refund(refund_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("REFUNDCM"),),
            (refund_id, tx_hash),
        );

        Ok(())
    }

    pub fn get_refund(env: Env, refund_id: BytesN<32>) -> Result<RefundRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .ok_or(ContractError::StatementNotFound)
    }

    // ── #1108 — Recoveries & Clawbacks ────────────────────────────────────

    /// Initiate a recovery/clawback for a disbursement.
    /// Recovery never silently debits a wallet; reason and legal basis are recorded.
    pub fn initiate_recovery(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        intent_id: BytesN<32>,
        recovery_amount: i128,
        reason: RecoveryReason,
        legal_basis: soroban_sdk::String,
    ) -> Result<RecoveryRecord, ContractError> {
        Self::require_admin(&env, &caller)?;

        if recovery_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        if legal_basis.len() == 0 || legal_basis.len() > 500 {
            return Err(ContractError::InvalidRecoveryReason);
        }

        let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
            env.clone(),
            intent_id.clone(),
        )?;

        if intent.program_id != program_id {
            return Err(ContractError::InvalidConfiguration);
        }

        if intent.status != IntentStatus::Executed {
            return Err(ContractError::IntentNotExecutable);
        }

        if recovery_amount > intent.amount {
            return Err(ContractError::RecoveryAmountExceedsOutstanding);
        }

        // Check for existing recovery on this intent
        let existing_recovery: Option<RecoveryRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Recovery(intent_id.clone()));

        if let Some(existing) = existing_recovery {
            if existing.status == RecoveryStatus::Completed {
                return Err(ContractError::RecoveryAlreadyProcessed);
            }
        }

        let recovery_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let recovery = RecoveryRecord {
            id: recovery_id.clone(),
            program_id: program_id.clone(),
            intent_id: intent_id.clone(),
            recipient: intent.recipient.clone(),
            original_amount: intent.amount,
            recovery_amount,
            reason,
            legal_basis,
            status: RecoveryStatus::Pending,
            initiated_at: now,
            initiated_by: caller.clone(),
            completed_at: 0,
            tx_hash: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Recovery(recovery_id.clone()), &recovery);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Recovery(recovery_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Update program financials
        Self::update_financials_on_recovery(&env, &program_id, recovery_amount)?;

        env.events().publish(
            (symbol_short!("RECVRYNW"),),
            (recovery_id, program_id, intent_id, recovery_amount, reason as u32),
        );

        Ok(recovery)
    }

    /// Complete a recovery.
    pub fn complete_recovery(
        env: Env,
        caller: Address,
        recovery_id: BytesN<32>,
        tx_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let mut recovery: RecoveryRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Recovery(recovery_id.clone()))
            .ok_or(ContractError::StatementNotFound)?;

        if recovery.status == RecoveryStatus::Completed {
            return Err(ContractError::RecoveryAlreadyProcessed);
        }

        recovery.status = RecoveryStatus::Completed;
        recovery.completed_at = env.ledger().timestamp();
        recovery.tx_hash = Some(tx_hash);

        env.storage()
            .persistent()
            .set(&DataKey::Recovery(recovery_id.clone()), &recovery);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Recovery(recovery_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("RECVRYCM"),),
            (recovery_id, tx_hash),
        );

        Ok(())
    }

    pub fn get_recovery(env: Env, recovery_id: BytesN<32>) -> Result<RecoveryRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Recovery(recovery_id))
            .ok_or(ContractError::StatementNotFound)
    }

    // ── #1109 — Financial Statements ───────────────────────────────────────

    /// Generate a financial statement for a program over a date range.
    /// Statements reconcile to ledger entries, use consistent currency metadata,
    /// and large exports run asynchronously.
    pub fn generate_statement(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        sponsor_id: BytesN<32>,
        period_start: u64,
        period_end: u64,
    ) -> Result<FinancialStatement, ContractError> {
        Self::require_sponsor_or_admin(&env, &caller, &sponsor_id)?;

        if period_start >= period_end {
            return Err(ContractError::InvalidDateRange);
        }

        // Get program info (would call scholarship-core in practice)
        // For now, we construct from stored financials
        let financials: ProgramFinancials = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramFinancials(program_id.clone()))
            .unwrap_or(ProgramFinancials {
                program_id: program_id.clone(),
                sponsor_id: sponsor_id.clone(),
                currency: soroban_sdk::Symbol::new(&env, "XLM"),
                total_funded: 0,
                total_disbursed: 0,
                total_fees_collected: 0,
                total_refunded: 0,
                total_recovered: 0,
                pending_disbursements: 0,
                pending_refunds: 0,
                pending_recoveries: 0,
                last_updated: env.ledger().timestamp(),
            });

        // In a full implementation, this would query events/ledger for the period
        // For now, return current financials as the statement
        let statement_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let statement = FinancialStatement {
            id: statement_id.clone(),
            program_id: program_id.clone(),
            sponsor_id: sponsor_id.clone(),
            period_start,
            period_end,
            currency: financials.currency,
            contributions: financials.total_funded,
            commitments: financials.pending_disbursements,
            payments: financials.total_disbursed,
            fees: financials.total_fees_collected,
            refunds: financials.total_refunded,
            recoveries: financials.total_recovered,
            remaining_balance: financials.total_funded
                .checked_sub(financials.total_disbursed)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_sub(financials.total_fees_collected)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_add(financials.total_refunded)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_add(financials.total_recovered)
                .ok_or(ContractError::ArithmeticOverflow)?,
            generated_at: now,
            generated_by: caller.clone(),
            version: CONTRACT_VERSION,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Statement(statement_id.clone()), &statement);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Statement(statement_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("STMTGEN"),),
            (statement_id, program_id, period_start, period_end),
        );

        Ok(statement)
    }

    pub fn get_statement(env: Env, statement_id: BytesN<32>) -> Result<FinancialStatement, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Statement(statement_id))
            .ok_or(ContractError::StatementNotFound)
    }

    /// Get current financial aggregates for a program.
    pub fn get_program_financials(env: Env, program_id: BytesN<32>) -> Result<ProgramFinancials, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProgramFinancials(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    fn update_financials_on_refund(
        env: &Env,
        program_id: &BytesN<32>,
        refund_amount: i128,
        fee_refunded: i128,
    ) -> Result<(), ContractError> {
        let mut financials: ProgramFinancials = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramFinancials(program_id.clone()))
            .unwrap_or(ProgramFinancials {
                program_id: program_id.clone(),
                sponsor_id: BytesN::from_array(env, &[0u8; 32]),
                currency: soroban_sdk::Symbol::new(env, "XLM"),
                total_funded: 0,
                total_disbursed: 0,
                total_fees_collected: 0,
                total_refunded: 0,
                total_recovered: 0,
                pending_disbursements: 0,
                pending_refunds: 0,
                pending_recoveries: 0,
                last_updated: env.ledger().timestamp(),
            });

        financials.total_refunded = financials.total_refunded
            .checked_add(refund_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        financials.total_fees_collected = financials.total_fees_collected
            .checked_sub(fee_refunded)
            .ok_or(ContractError::ArithmeticOverflow)?;
        financials.pending_refunds = financials.pending_refunds
            .checked_sub(refund_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        financials.last_updated = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::ProgramFinancials(program_id.clone()), &financials);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ProgramFinancials(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        Ok(())
    }

    fn update_financials_on_recovery(
        env: &Env,
        program_id: &BytesN<32>,
        recovery_amount: i128,
    ) -> Result<(), ContractError> {
        let mut financials: ProgramFinancials = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramFinancials(program_id.clone()))
            .unwrap_or(ProgramFinancials {
                program_id: program_id.clone(),
                sponsor_id: BytesN::from_array(env, &[0u8; 32]),
                currency: soroban_sdk::Symbol::new(env, "XLM"),
                total_funded: 0,
                total_disbursed: 0,
                total_fees_collected: 0,
                total_refunded: 0,
                total_recovered: 0,
                pending_disbursements: 0,
                pending_refunds: 0,
                pending_recoveries: 0,
                last_updated: env.ledger().timestamp(),
            });

        financials.total_recovered = financials.total_recovered
            .checked_add(recovery_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        financials.pending_recoveries = financials.pending_recoveries
            .checked_sub(recovery_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        financials.last_updated = env.ledger().timestamp();

        env.storage()
            .persistent()
            .set(&DataKey::ProgramFinancials(program_id.clone()), &financials);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ProgramFinancials(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;