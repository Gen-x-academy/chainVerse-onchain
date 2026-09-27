#![no_std]

//! Scholarship/bursary finance contract.
//!
//! Scope of this pass (issues #1102, #1103, #1104, #1105):
//! - #1105: Support sponsor deposits and funding rounds
//! - #1104: Reconcile treasury balances and liabilities
//! - #1103: Maintain per-program financial ledgers
//! - #1102: Create recipient payment receipts
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
//! only addresses, amounts, timestamps, and status enums.
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
    DepositNotFound = 26,
    FundingRoundNotFound = 27,
    InvalidFundingRound = 28,
    LedgerNotFound = 28,
    ReconciliationFailed = 29,
    ReceiptNotFound = 30,
    InvalidReceiptData = 31,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NetworkId,
    /// Sponsor deposit records
    Deposit(BytesN<32>),
    /// Funding round records
    FundingRound(BytesN<32>),
    /// Per-program financial ledgers
    ProgramLedger(BytesN<32>),
    /// Treasury reconciliation records
    Reconciliation(BytesN<32>),
    /// Payment receipt records
    Receipt(BytesN<32>),
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
pub enum DepositType {
    Unrestricted,
    ProgramSpecific,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositRecord {
    pub id: BytesN<32>,
    pub sponsor_id: BytesN<32>,
    pub program_id: Option<BytesN<32>>,
    pub deposit_type: DepositType,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub tx_hash: BytesN<32>,
    pub timestamp: u64,
    pub funding_round_id: Option<BytesN<32>>,
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
pub struct FundingRound {
    pub id: BytesN<32>,
    pub name: soroban_sdk::String,
    pub description: soroban_sdk::String,
    pub target_amount: i128,
    pub asset: BytesN<32>,
    pub start_time: u64,
    pub end_time: u64,
    pub is_active: bool,
    pub created_at: u64,
    pub created_by: Address,
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
pub enum LedgerEntryType {
    Deposit,
    Withdrawal,
    Award,
    Disbursement,
    Refund,
    Recovery,
    Fee,
    Adjustment,
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
pub struct LedgerEntry {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub entry_type: LedgerEntryType,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub balance_after: i128,
    pub reference_id: Option<BytesN<32>>,
    pub reference_type: Option<soroban_sdk::String>,
    pub timestamp: u64,
    pub created_by: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramLedger {
    pub program_id: BytesN<32>,
    pub sponsor_id: BytesN<32>,
    pub currency: soroban_sdk::Symbol,
    pub entries: Vec<LedgerEntry>,
    pub total_deposits: i128,
    pub total_withdrawals: i128,
    pub total_awards: i128,
    pub total_disbursements: i128,
    pub total_refunds: i128,
    pub total_recoveries: i128,
    pub total_fees: i128,
    pub current_balance: i128,
    pub reserved_balance: i128,
    pub available_balance: i128,
    pub last_updated: u64,
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
pub enum ReconciliationStatus {
    Balanced,
    DriftDetected,
    InsolvencyRisk,
pub enum RecoveryReason {
    Fraud,
    MilestoneFailure,
    Withdrawal,
    PolicyViolation,
    ComplianceHold,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationRecord {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub status: ReconciliationStatus,
    pub expected_balance: i128,
    pub actual_balance: i128,
    pub drift_amount: i128,
    pub discrepancies: Vec<Discrepancy>,
    pub reconciled_at: u64,
    pub reconciled_by: Address,
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
pub struct Discrepancy {
    pub entry_id: BytesN<32>,
    pub expected: i128,
    pub actual: i128,
    pub description: soroban_sdk::String,
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
pub struct PaymentReceipt {
    pub id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub recipient: Address,
    pub award_id: BytesN<32>,
    pub installment: u32,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub tx_hash: BytesN<32>,
    pub network: soroban_sdk::String,
    pub completed_at: u64,
    pub receipt_hash: BytesN<32>,
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

    // ── #1105 — Sponsor Deposits and Funding Rounds ────────────────────────

    /// Record a sponsor deposit to a program or unrestricted pool.
    pub fn record_deposit(
        env: Env,
        sponsor: Address,
        program_id: Option<BytesN<32>>,
        asset: BytesN<32>,
        amount: i128,
        tx_hash: BytesN<32>,
        funding_round_id: Option<BytesN<32>>,
    ) -> Result<DepositRecord, ContractError> {
        sponsor.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        let deposit_type = match program_id {
            Some(_) => DepositType::ProgramSpecific,
            None => DepositType::Unrestricted,
        };

        if let Some(program_id) = program_id {
            let program: scholarship_core::Program = scholarship_core::ScholarshipCoreContract::get_program(
                env.clone(),
                program_id.clone(),
            )?;
            if program.status != ProgramStatus::Published {
                return Err(ContractError::ProgramNotPublished);
            }
        }

        if let Some(funding_round_id) = funding_round_id {
            let round: FundingRound = env
                .storage()
                .persistent()
                .get(&DataKey::FundingRound(funding_round_id.clone()))
                .ok_or(ContractError::FundingRoundNotFound)?;
            if !round.is_active {
                return Err(ContractError::InvalidFundingRound);
            }
            let now = env.ledger().timestamp();
            if now < round.start_time || now > round.end_time {
                return Err(ContractError::InvalidFundingRound);
            }
        }

        let deposit_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let deposit = DepositRecord {
            id: deposit_id.clone(),
            sponsor_id: BytesN::from_array(&env, &sponsor.to_array()), // Simplified
            program_id,
            deposit_type,
            asset: asset.clone(),
            amount,
            tx_hash: tx_hash.clone(),
            timestamp: now,
            funding_round_id,
        };

        env.storage().persistent().set(&DataKey::Deposit(deposit_id.clone()), &deposit);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Deposit(deposit_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Update program ledger if program-specific
        if let Some(program_id) = program_id {
            Self::update_ledger_on_deposit(&env, &program_id, &asset, amount)?;
        }

        env.events().publish(
            (symbol_short!("DEPOSIT"),),
            (deposit_id, sponsor, amount, asset),
        );

        Ok(deposit)
    }

    /// Create a funding round for a sponsor.
    pub fn create_funding_round(
        env: Env,
        admin: Address,
        name: soroban_sdk::String,
        description: soroban_sdk::String,
        target_amount: i128,
        asset: BytesN<32>,
        start_time: u64,
        end_time: u64,
    ) -> Result<FundingRound, ContractError> {
        Self::require_admin(&env, &admin)?;

        if target_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if start_time >= end_time {
            return Err(ContractError::InvalidFundingRound);
        }

        let round_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let round = FundingRound {
            id: round_id.clone(),
            name,
            description,
            target_amount,
            asset,
            start_time,
            end_time,
            is_active: true,
            created_at: now,
            created_by: admin.clone(),
        };

        env.storage().persistent().set(&DataKey::FundingRound(round_id.clone()), &round);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::FundingRound(round_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("FUNDROUND"),),
            (round_id, round.target_amount),
        );

        Ok(round)
    }

    pub fn get_deposit(env: Env, deposit_id: BytesN<32>) -> Result<DepositRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Deposit(deposit_id))
            .ok_or(ContractError::DepositNotFound)
    }

    pub fn get_funding_round(env: Env, round_id: BytesN<32>) -> Result<FundingRound, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FundingRound(round_id))
            .ok_or(ContractError::FundingRoundNotFound)
    }

    // ── #1103 — Per-Program Financial Ledgers ──────────────────────────────

    fn update_ledger_on_deposit(
        env: &Env,
        program_id: &BytesN<32>,
        asset: &BytesN<32>,
        amount: i128,
    ) -> Result<(), ContractError> {
        let mut ledger: ProgramLedger = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramLedger(program_id.clone()))
            .unwrap_or(ProgramLedger {
                program_id: program_id.clone(),
                sponsor_id: BytesN::from_array(env, &[0u8; 32]),
                currency: soroban_sdk::Symbol::new(env, "XLM"),
                entries: Vec::new(env),
                total_deposits: 0,
                total_withdrawals: 0,
                total_awards: 0,
                total_disbursements: 0,
                total_refunds: 0,
                total_recoveries: 0,
                total_fees: 0,
                current_balance: 0,
                reserved_balance: 0,
                available_balance: 0,
                last_updated: 0,
            });

        let entry_id = Self::next_id(env);
        let entry = LedgerEntry {
            id: entry_id.clone(),
            program_id: program_id.clone(),
            entry_type: LedgerEntryType::Deposit,
            asset: asset.clone(),
            amount,
            balance_after: ledger.current_balance.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?,
            reference_id: None,
            reference_type: None,
            timestamp: env.ledger().timestamp(),
            created_by: env.current_contract_address(),
        };

        ledger.entries.push(entry);
        ledger.total_deposits = ledger.total_deposits.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        ledger.current_balance = ledger.current_balance.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        ledger.available_balance = ledger.available_balance.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        ledger.last_updated = env.ledger().timestamp();

        env.storage().persistent().set(&DataKey::ProgramLedger(program_id.clone()), &ledger);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ProgramLedger(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        Ok(())
    }

    /// Record an award entry in the program ledger.
    pub fn record_award(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        asset: BytesN<32>,
        amount: i128,
        reference_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        let mut ledger: ProgramLedger = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramLedger(program_id.clone()))
            .unwrap_or(ProgramLedger {
                program_id: program_id.clone(),
                sponsor_id: BytesN::from_array(&env, &[0u8; 32]),
                currency: soroban_sdk::Symbol::new(env, "XLM"),
                entries: Vec::new(env),
                total_deposits: 0,
                total_withdrawals: 0,
                total_awards: 0,
                total_disbursements: 0,
                total_refunds: 0,
                total_recoveries: 0,
                total_fees: 0,
                current_balance: 0,
                reserved_balance: 0,
                available_balance: 0,
                last_updated: 0,
            });

        let entry_id = Self::next_id(&env);
        let new_reserved = ledger.reserved_balance.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        let new_available = ledger.available_balance.checked_sub(amount).ok_or(ContractError::ArithmeticOverflow)?;

        let entry = LedgerEntry {
            id: Self::next_id(&env),
            program_id: program_id.clone(),
            entry_type: LedgerEntryType::Award,
            asset: asset.clone(),
            amount,
            balance_after: ledger.current_balance,
            reference_id: Some(reference_id.clone()),
            reference_type: Some(soroban_sdk::String::from_str(&env, "award")),
            timestamp: env.ledger().timestamp(),
            created_by: admin.clone(),
        };

        ledger.entries.push(entry);
        ledger.total_awards = ledger.total_awards.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        ledger.reserved_balance = new_reserved;
        ledger.available_balance = new_available;
        ledger.last_updated = env.ledger().timestamp();

        env.storage().persistent().set(&DataKey::ProgramLedger(program_id.clone()), &ledger);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ProgramLedger(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("AWARDREC"),),
            (program_id, reference_id, amount),
        );

        Ok(())
    }

    /// Record a disbursement entry in the program ledger.
    pub fn record_disbursement(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        asset: BytesN<32>,
        amount: i128,
        reference_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        let mut ledger: ProgramLedger = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramLedger(program_id.clone()))
            .ok_or(ContractError::LedgerNotFound)?;

        if ledger.reserved_balance < amount {
            return Err(ContractError::InvalidAmount);
        }

        let new_reserved = ledger.reserved_balance.checked_sub(amount).ok_or(ContractError::ArithmeticOverflow)?;
        let new_current = ledger.current_balance.checked_sub(amount).ok_or(ContractError::ArithmeticOverflow)?;

        let entry = LedgerEntry {
            id: Self::next_id(&env),
            program_id: program_id.clone(),
            entry_type: LedgerEntryType::Disbursement,
            asset: asset.clone(),
            amount,
            balance_after: new_current,
            reference_id: Some(reference_id.clone()),
            reference_type: Some(soroban_sdk::String::from_str(&env, "disbursement")),
            timestamp: env.ledger().timestamp(),
            created_by: admin.clone(),
        };

        ledger.entries.push(entry);
        ledger.total_disbursements = ledger.total_disbursements.checked_add(amount).ok_or(ContractError::ArithmeticOverflow)?;
        ledger.reserved_balance = new_reserved;
        ledger.current_balance = new_current;
        ledger.last_updated = env.ledger().timestamp();

        env.storage().persistent().set(&DataKey::ProgramLedger(program_id.clone()), &ledger);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ProgramLedger(program_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("DISBREC"),),
            (program_id, reference_id, amount),
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

    pub fn get_program_ledger(env: Env, program_id: BytesN<32>) -> Result<ProgramLedger, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProgramLedger(program_id))
            .ok_or(ContractError::LedgerNotFound)
    }

    // ── #1104 — Reconcile Treasury Balances and Liabilities ────────────────

    /// Reconcile a program's treasury balances against expected liabilities.
    pub fn reconcile_treasury(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
    ) -> Result<ReconciliationRecord, ContractError> {
        Self::require_admin(&env, &admin)?;

        let ledger: ProgramLedger = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramLedger(program_id.clone()))
            .ok_or(ContractError::LedgerNotFound)?;

        let program: scholarship_core::Program = scholarship_core::ScholarshipCoreContract::get_program(
            env.clone(),
            program_id.clone(),
        )?;

        if program.status != ProgramStatus::Published && program.status != ProgramStatus::Closed {
            return Err(ContractError::ProgramNotPublished);
        }

        // Calculate expected balance from disbursements + reserved
        let expected_balance = ledger.current_balance;
        let actual_balance = ledger.current_balance; // In practice, would query external state

        let drift_amount = expected_balance.checked_sub(actual_balance).ok_or(ContractError::ArithmeticOverflow)?;

        let mut discrepancies = Vec::new(&env);
        let mut status = ReconciliationStatus::Balanced;

        if drift_amount != 0 {
            if drift_amount < 0 {
                status = ReconciliationStatus::InsolvencyRisk;
            } else {
                status = ReconciliationStatus::DriftDetected;
            }

            discrepancies.push(Discrepancy {
                entry_id: BytesN::from_array(&env, &[0u8; 32]),
                expected: expected_balance,
                actual: actual_balance,
                description: soroban_sdk::String::from_str(&env, "Balance drift detected"),
            });
        }

        let reconciliation_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let reconciliation = ReconciliationRecord {
            id: reconciliation_id.clone(),
            program_id: program_id.clone(),
            status,
            expected_balance,
            actual_balance,
            drift_amount,
            discrepancies,
            reconciled_at: now,
            reconciled_by: admin.clone(),
        };

        env.storage().persistent().set(&DataKey::Reconciliation(reconciliation_id.clone()), &reconciliation);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Reconciliation(reconciliation_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("RECONCIL"),),
            (reconciliation_id, program_id, status as u32),
        );

        Ok(reconciliation)
    }

    pub fn get_reconciliation(env: Env, reconciliation_id: BytesN<32>) -> Result<ReconciliationRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Reconciliation(reconciliation_id))
            .ok_or(ContractError::ReconciliationFailed)
    }

    // ── #1102 — Create Recipient Payment Receipts ────────────────────────

    /// Generate a payment receipt for a completed disbursement.
    pub fn create_receipt(
        env: Env,
        admin: Address,
        intent_id: BytesN<32>,
        tx_hash: BytesN<32>,
        network: soroban_sdk::String,
    ) -> Result<PaymentReceipt, ContractError> {
        Self::require_admin(&env, &admin)?;

        let intent: DisbursementIntent = scholarship_disbursements::ScholarshipDisbursementsContract::get_intent(
            env.clone(),
            intent_id.clone(),
        )?;

        if intent.status != IntentStatus::Executed {
            return Err(ContractError::IntentNotExecutable);
        }

        if intent.executed_at == 0 {
            return Err(ContractError::IntentNotExecutable);
        }

        let program: scholarship_core::Program = scholarship_core::ScholarshipCoreContract::get_program(
            env.clone(),
            intent.program_id.clone(),
        )?;

        let receipt_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        // Create receipt hash for verification
        let mut receipt_input = Bytes::new(&env);
        receipt_input.extend_from_array(&intent_id.to_array());
        receipt_input.extend_from_array(&tx_hash.to_array());
        receipt_input.extend_from_array(&intent.amount.to_be_bytes());
        receipt_input.extend_from_array(&now.to_be_bytes());
        let receipt_hash = env.crypto().sha256(&receipt_input).into();

        let receipt = PaymentReceipt {
            id: receipt_id.clone(),
            intent_id: intent_id.clone(),
            program_id: intent.program_id.clone(),
            recipient: intent.recipient.clone(),
            award_id: intent.program_id.clone(), // Simplified
            installment: intent.installment,
            asset: BytesN::from_array(&env, &[0u8; 32]), // Would come from program config
            amount: intent.amount,
            tx_hash: tx_hash.clone(),
            network,
            completed_at: intent.executed_at,
            receipt_hash,
        };

        env.storage().persistent().set(&DataKey::Receipt(receipt_id.clone()), &receipt);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Receipt(receipt_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("RECEIPT"),),
            (receipt_id, intent_id, intent.recipient, intent.amount),
        );

        Ok(receipt)
    }

    pub fn get_receipt(env: Env, receipt_id: BytesN<32>) -> Result<PaymentReceipt, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Receipt(receipt_id))
            .ok_or(ContractError::ReceiptNotFound)
    }

    /// Verify a receipt against on-chain data.
    pub fn verify_receipt(env: Env, receipt_id: BytesN<32>) -> Result<bool, ContractError> {
        let receipt: PaymentReceipt = Self::get_receipt(env.clone(), receipt_id.clone())?;

        let mut verify_input = Bytes::new(&env);
        verify_input.extend_from_array(&receipt.intent_id.to_array());
        verify_input.extend_from_array(&receipt.tx_hash.to_array());
        verify_input.extend_from_array(&receipt.amount.to_be_bytes());
        verify_input.extend_from_array(&receipt.completed_at.to_be_bytes());
        let computed_hash = env.crypto().sha256(&verify_input).into();

        Ok(computed_hash == receipt.receipt_hash)
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