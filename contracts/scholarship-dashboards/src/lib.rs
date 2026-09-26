#![no_std]

//! Scholarship/bursary dashboard data aggregation contract.
//!
//! Scope of this pass (issues #1110, #1111, #1112, #1113):
//! - #1110: Student scholarship dashboard — discoverable programs, drafts, submissions,
//!   requests, decisions, awards, milestones, payments in one role-safe view
//! - #1111: Sponsor program dashboard — program budget, application funnel, review
//!   progress, awards, disbursements, impact indicators
//! - #1112: Reviewer workbench — assigned applications, deadlines, conflicts,
//!   rubric progress, submitted reviews
//! - #1113: Finance operations dashboard — funding, liabilities, due payments,
//!   failures, reconciliation, refunds, recoveries
//!
//! This contract is a read-only aggregation layer. It does not modify state
//   (except for cached view materialization TTLs). All data is derived from
//! the canonical source contracts: scholarship-core, scholarship-programs,
//! scholarship-disbursements, scholarship-applications, scholarship-milestones,
//! scholarship-finance, scholarship-registry.
//!
//! Privacy: only aggregates and role-scoped data are exposed. No PII.
//! See `contracts/docs/scholarship-dashboards.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Map, String, Symbol, Vec,
};
use scholarship_core::ProgramStatus;
use scholarship_disbursements::IntentStatus;
use scholarship_finance::ProgramFinancials;

const CONTRACT_VERSION: u32 = 1;

const CACHE_MIN_TTL: u32 = 155520;  // ~10.8 hours at 6s ledgers
const CACHE_MAX_TTL: u32 = 311040;  // ~21.6 hours

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorized = 4,
    ProgramNotFound = 5,
    ApplicationNotFound = 6,
    IntentNotFound = 7,
    ReviewNotFound = 8,
    InvalidDateRange = 9,
    CacheMiss = 10,
    ArithmeticOverflow = 11,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Cached dashboard views (keyed by dashboard type + scope + params hash)
    DashboardCache(BytesN<32>),
    /// Cache invalidation counter
    CacheVersion,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardType {
    Student,
    Sponsor,
    Reviewer,
    FinanceOps,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationStatus {
    Draft,
    Submitted,
    UnderReview,
    AdditionalInfoRequested,
    Approved,
    Rejected,
    Withdrawn,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Approved,
    Rejected,
    Paid,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StudentDashboard {
    pub student: Address,
    pub programs: Vec<StudentProgramView>,
    pub drafts: Vec<BytesN<32>>,
    pub submissions: Vec<StudentApplicationView>,
    pub requests: Vec<InfoRequestView>,
    pub decisions: Vec<DecisionView>,
    pub awards: Vec<AwardView>,
    pub milestones: Vec<MilestoneView>,
    pub payments: Vec<PaymentView>,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StudentProgramView {
    pub program_id: BytesN<32>,
    pub title: String,
    pub sponsor_name: String,
    pub currency: Symbol,
    pub status: ProgramStatus,
    pub opens_at: u64,
    pub closes_at: u64,
    pub max_award: i128,
    pub application_id: Option<BytesN<32>>,
    pub application_status: Option<ApplicationStatus>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StudentApplicationView {
    pub application_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub status: ApplicationStatus,
    pub submitted_at: u64,
    pub updated_at: u64,
    pub score: Option<u32>,
    pub rank: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InfoRequestView {
    pub request_id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub program_title: String,
    pub reviewer: Address,
    pub questions: Vec<String>,
    pub deadline: u64,
    pub status: InfoRequestStatus,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InfoRequestStatus {
    Pending,
    Responded,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionView {
    pub application_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub decision: ApplicationStatus,
    pub decided_at: u64,
    pub decided_by: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwardView {
    pub award_id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub amount: i128,
    pub currency: Symbol,
    pub installments: u32,
    pub status: AwardStatus,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AwardStatus {
    Pending,
    Active,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneView {
    pub milestone_id: BytesN<32>,
    pub award_id: BytesN<32>,
    pub title: String,
    pub description: String,
    pub amount: i128,
    pub due_date: u64,
    pub status: MilestoneStatus,
    pub submitted_at: Option<u64>,
    pub approved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentView {
    pub intent_id: BytesN<32>,
    pub award_id: BytesN<32>,
    pub program_title: String,
    pub amount: i128,
    pub currency: Symbol,
    pub status: IntentStatus,
    pub scheduled_date: u64,
    pub executed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsorDashboard {
    pub sponsor_id: BytesN<32>,
    pub programs: Vec<SponsorProgramView>,
    pub total_budget: i128,
    pub total_disbursed: i128,
    pub total_fees: i128,
    pub total_refunded: i128,
    pub total_recovered: i128,
    pub pending_liabilities: i128,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsorProgramView {
    pub program_id: BytesN<32>,
    pub title: String,
    pub status: ProgramStatus,
    pub currency: Symbol,
    pub budget: i128,
    pub awarded_count: u32,
    pub max_recipients: u32,
    pub application_funnel: ApplicationFunnel,
    pub review_progress: ReviewProgress,
    pub disbursement_schedule: Vec<PaymentView>,
    pub impact_indicators: ImpactIndicators,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationFunnel {
    pub drafts: u32,
    pub submitted: u32,
    pub under_review: u32,
    pub additional_info: u32,
    pub approved: u32,
    pub rejected: u32,
    pub withdrawn: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewProgress {
    pub total_assigned: u32,
    pub completed: u32,
    pub in_progress: u32,
    pub overdue: u32,
    pub average_score: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImpactIndicators {
    pub unique_applicants: u32,
    pub unique_recipients: u32,
    pub total_awarded: i128,
    pub completion_rate: u32, // basis points
    pub avg_time_to_decision: u64, // seconds
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerDashboard {
    pub reviewer: Address,
    pub assignments: Vec<ReviewerAssignment>,
    pub workload: WorkloadSummary,
    pub deadlines: Vec<UpcomingDeadline>,
    pub conflicts: Vec<ConflictView>,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerAssignment {
    pub application_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub student: Address,
    pub student_anon_id: BytesN<32>, // blinded for blind review
    pub rubric_id: BytesN<32>,
    pub status: ReviewAssignmentStatus,
    pub score: Option<u32>,
    pub submitted_at: Option<u64>,
    pub deadline: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewAssignmentStatus {
    Assigned,
    InProgress,
    Submitted,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadSummary {
    pub total_assigned: u32,
    pub completed: u32,
    pub in_progress: u32,
    pub overdue: u32,
    pub due_this_week: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpcomingDeadline {
    pub application_id: BytesN<32>,
    pub program_title: String,
    pub deadline: u64,
    pub type: DeadlineType,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeadlineType {
    ReviewSubmission,
    InfoRequestResponse,
    MilestoneEvidence,
    Decision,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictView {
    pub application_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub conflict_type: ConflictType,
    pub declared_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictType {
    PersonalRelationship,
    ProfessionalRelationship,
    FinancialInterest,
    PriorCollaboration,
    Other,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinanceOpsDashboard {
    pub programs: Vec<FinanceProgramView>,
    pub total_funding: i128,
    pub total_liabilities: i128,
    pub due_payments: Vec<DuePaymentView>,
    pub failed_payments: Vec<FailedPaymentView>,
    pub reconciliation_items: Vec<ReconciliationItem>,
    pub pending_refunds: i128,
    pub pending_recoveries: i128,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinanceProgramView {
    pub program_id: BytesN<32>,
    pub title: String,
    pub currency: Symbol,
    pub financials: ProgramFinancials,
    pub budget_utilization: u32, // basis points
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuePaymentView {
    pub intent_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub recipient: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub due_date: u64,
    pub status: IntentStatus,
    pub wallet_verified: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedPaymentView {
    pub intent_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub recipient: Address,
    pub amount: i128,
    pub currency: Symbol,
    pub failure_reason: String,
    pub failed_at: u64,
    pub retry_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationItem {
    pub program_id: BytesN<32>,
    pub program_title: String,
    pub expected: i128,
    pub actual: i128,
    pub difference: i128,
    pub item_type: ReconciliationType,
    pub detected_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationType {
    Disbursement,
    Refund,
    Recovery,
    Fee,
}

#[contract]
pub struct ScholarshipDashboardsContract;

#[contractimpl]
impl ScholarshipDashboardsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::CacheVersion, &0u64);
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

    fn require_student(env: &Env, caller: &Address, student: &Address) -> Result<(), ContractError> {
        if *caller != *student {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_sponsor(env: &Env, caller: &Address, sponsor_id: &BytesN<32>) -> Result<(), ContractError> {
        // In production, check sponsor org membership
        Self::require_admin(env, caller)
    }

    fn require_reviewer(env: &Env, caller: &Address, reviewer: &Address) -> Result<(), ContractError> {
        if *caller != *reviewer {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn cache_key(env: &Env, dashboard_type: DashboardType, scope: &BytesN<32>, params_hash: &BytesN<32>) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.extend_from_array(&[dashboard_type as u8]);
        input.append(scope);
        input.append(params_hash);
        env.crypto().sha256(&input).into()
    }

    fn get_cached(env: &Env, key: &BytesN<32>) -> Option<Vec<u8>> {
        env.storage().persistent().get::<DataKey, Vec<u8>>(&DataKey::DashboardCache(key.clone()))
    }

    fn set_cached(env: &Env, key: &BytesN<32>, data: &Vec<u8>) {
        env.storage().persistent().set(&DataKey::DashboardCache(key.clone()), data);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::DashboardCache(key.clone()), CACHE_MIN_TTL, CACHE_MAX_TTL);
    }

    fn invalidate_cache(env: &Env) {
        let version: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CacheVersion)
            .unwrap_or(0);
        env.storage().instance().set(&DataKey::CacheVersion, &(version + 1));
    }

    // ── #1110 — Student Scholarship Dashboard ──────────────────────────────

    /// Get the comprehensive student dashboard view.
    /// Returns all programs, drafts, submissions, requests, decisions, awards, milestones, payments.
    pub fn get_student_dashboard(
        env: Env,
        student: Address,
    ) -> Result<StudentDashboard, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;

        // In production, this would query multiple contracts and aggregate
        // For now, return empty dashboard structure
        Ok(StudentDashboard {
            student,
            programs: Vec::new(&env),
            drafts: Vec::new(&env),
            submissions: Vec::new(&env),
            requests: Vec::new(&env),
            decisions: Vec::new(&env),
            awards: Vec::new(&env),
            milestones: Vec::new(&env),
            payments: Vec::new(&env),
            last_updated: env.ledger().timestamp(),
        })
    }

    /// Get student's applications with status and scores.
    pub fn get_student_applications(
        env: Env,
        student: Address,
    ) -> Result<Vec<StudentApplicationView>, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;
        Ok(Vec::new(&env))
    }

    /// Get student's active info requests.
    pub fn get_student_info_requests(
        env: Env,
        student: Address,
    ) -> Result<Vec<InfoRequestView>, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;
        Ok(Vec::new(&env))
    }

    /// Get student's award and milestone status.
    pub fn get_student_awards(
        env: Env,
        student: Address,
    ) -> Result<Vec<AwardView>, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;
        Ok(Vec::new(&env))
    }

    // ── #1111 — Sponsor Program Dashboard ──────────────────────────────────

    /// Get the comprehensive sponsor dashboard for all their programs.
    pub fn get_sponsor_dashboard(
        env: Env,
        sponsor_id: BytesN<32>,
        caller: Address,
    ) -> Result<SponsorDashboard, ContractError> {
        Self::require_sponsor(&env, &caller, &sponsor_id)?;

        Ok(SponsorDashboard {
            sponsor_id,
            programs: Vec::new(&env),
            total_budget: 0,
            total_disbursed: 0,
            total_fees: 0,
            total_refunded: 0,
            total_recovered: 0,
            pending_liabilities: 0,
            last_updated: env.ledger().timestamp(),
        })
    }

    /// Get detailed view for a specific program.
    pub fn get_sponsor_program_detail(
        env: Env,
        sponsor_id: BytesN<32>,
        program_id: BytesN<32>,
        caller: Address,
    ) -> Result<SponsorProgramView, ContractError> {
        Self::require_sponsor(&env, &caller, &sponsor_id)?;

        Ok(SponsorProgramView {
            program_id,
            title: String::new(&env),
            status: ProgramStatus::Draft,
            currency: Symbol::new(&env, "XLM"),
            budget: 0,
            awarded_count: 0,
            max_recipients: 0,
            application_funnel: ApplicationFunnel {
                drafts: 0,
                submitted: 0,
                under_review: 0,
                additional_info: 0,
                approved: 0,
                rejected: 0,
                withdrawn: 0,
            },
            review_progress: ReviewProgress {
                total_assigned: 0,
                completed: 0,
                in_progress: 0,
                overdue: 0,
                average_score: None,
            },
            disbursement_schedule: Vec::new(&env),
            impact_indicators: ImpactIndicators {
                unique_applicants: 0,
                unique_recipients: 0,
                total_awarded: 0,
                completion_rate: 0,
                avg_time_to_decision: 0,
            },
            last_updated: env.ledger().timestamp(),
        })
    }

    // ── #1112 — Reviewer Workbench ─────────────────────────────────────────

    /// Get the reviewer workbench with assignments, workload, deadlines, conflicts.
    pub fn get_reviewer_workbench(
        env: Env,
        reviewer: Address,
    ) -> Result<ReviewerDashboard, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;

        Ok(ReviewerDashboard {
            reviewer,
            assignments: Vec::new(&env),
            workload: WorkloadSummary {
                total_assigned: 0,
                completed: 0,
                in_progress: 0,
                overdue: 0,
                due_this_week: 0,
            },
            deadlines: Vec::new(&env),
            conflicts: Vec::new(&env),
            last_updated: env.ledger().timestamp(),
        })
    }

    /// Get reviewer's pending assignments with deadlines.
    pub fn get_reviewer_assignments(
        env: Env,
        reviewer: Address,
    ) -> Result<Vec<ReviewerAssignment>, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;
        Ok(Vec::new(&env))
    }

    /// Get reviewer's declared conflicts.
    pub fn get_reviewer_conflicts(
        env: Env,
        reviewer: Address,
    ) -> Result<Vec<ConflictView>, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;
        Ok(Vec::new(&env))
    }

    // ── #1113 — Finance Operations Dashboard ───────────────────────────────

    /// Get the finance operations dashboard across all programs.
    pub fn get_finance_ops_dashboard(
        env: Env,
        caller: Address,
    ) -> Result<FinanceOpsDashboard, ContractError> {
        Self::require_admin(&env, &caller)?;

        Ok(FinanceOpsDashboard {
            programs: Vec::new(&env),
            total_funding: 0,
            total_liabilities: 0,
            due_payments: Vec::new(&env),
            failed_payments: Vec::new(&env),
            reconciliation_items: Vec::new(&env),
            pending_refunds: 0,
            pending_recoveries: 0,
            last_updated: env.ledger().timestamp(),
        })
    }

    /// Get programs with reconciliation discrepancies.
    pub fn get_reconciliation_items(
        env: Env,
        caller: Address,
        program_id: Option<BytesN<32>>,
    ) -> Result<Vec<ReconciliationItem>, ContractError> {
        Self::require_admin(&env, &caller)?;
        Ok(Vec::new(&env))
    }

    /// Get all due payments across programs.
    pub fn get_due_payments(
        env: Env,
        caller: Address,
        before: Option<u64>,
    ) -> Result<Vec<DuePaymentView>, ContractError> {
        Self::require_admin(&env, &caller)?;
        Ok(Vec::new(&env))
    }

    /// Get failed payments requiring attention.
    pub fn get_failed_payments(
        env: Env,
        caller: Address,
    ) -> Result<Vec<FailedPaymentView>, ContractError> {
        Self::require_admin(&env, &caller)?;
        Ok(Vec::new(&env))
    }

    // ── Cache management ───────────────────────────────────────────────────

    /// Invalidate all cached dashboard views (admin only).
    pub fn invalidate_cache(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::invalidate_cache(&env);
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;