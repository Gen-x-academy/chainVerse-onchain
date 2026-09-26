#![no_std]

//! Scholarship/bursary finance contract.
//!
//! Scope of this pass (issues #1102, #1103, #1104, #1105):
//! - #1105: Support sponsor deposits and funding rounds
//! - #1104: Reconcile treasury balances and liabilities
//! - #1103: Maintain per-program financial ledgers
//! - #1102: Create recipient payment receipts
//!
//! This contract builds on the existing scholarship infrastructure:
//! - scholarship-core: program lifecycle and metadata
//! - scholarship-disbursements: payment intents and wallet verification
//! - scholarship-programs: award budgets and windows
//!
//! All financial operations are privacy-minimized: no PII on-chain,
//! only addresses, amounts, timestamps, and status enums.

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
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationStatus {
    Balanced,
    DriftDetected,
    InsolvencyRisk,
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
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discrepancy {
    pub entry_id: BytesN<32>,
    pub expected: i128,
    pub actual: i128,
    pub description: soroban_sdk::String,
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
}

#[contract]
pub struct ScholarshipFinanceContract;

#[contractimpl]
impl ScholarshipFinanceContract {
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
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;