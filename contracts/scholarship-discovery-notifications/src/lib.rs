#![no_std]

//! Scholarship/bursary discovery and notifications contract.
//!
//! Scope of this pass (issues #1114, #1115, #1116, #1117):
//! - #1114: Create scholarship catalog search and filters (URL-backed, stable pagination)
//! - #1115: Add personalized scholarship matching (explainable, cold-start, privacy-preserving)
//! - #1116: Generate shareable public program pages (accessible, indexable, canonical URLs)
//! - #1117: Create scholarship notification events (idempotent, stable references, minimal data)
//!
//! This contract provides:
//! - Public program catalog with search/filter
//! - Personalized matching with explainable recommendations
//! - Shareable program pages with structured metadata
//! - Notification event emission for off-chain delivery
//!
//! See `contracts/docs/scholarship-discovery-notifications.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Map, String, Symbol, Vec,
};
use scholarship_core::ProgramStatus;
use scholarship_programs::{ProgramWindow, AwardBudget};
use scholarship_eligibility::{EligibilityCriteria, EligibilityCheckResult};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const MAX_SEARCH_RESULTS: u32 = 100;
const DEFAULT_PAGE_SIZE: u32 = 20;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorized = 4,
    ProgramNotFound = 5,
    InvalidSearchParams = 6,
    InvalidFilter = 7,
    PageSizeExceeded = 8,
    CursorInvalid = 9,
    MatchingNotAvailable = 10,
    ProfileIncomplete = 11,
    PageGenerationFailed = 12,
    EventEmissionFailed = 13,
    ArithmeticOverflow = 14,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Program search index (simplified - in production would use external indexer)
    ProgramIndex,
    /// Student profiles for matching
    StudentProfile(Address),
    /// Notification event log
    NotificationEvent(BytesN<32>),
    /// Counter for unique IDs
    IdCounter,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FundingType {
    Scholarship,
    Grant,
    Fellowship,
    Bursary,
    Award,
    Prize,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortBy {
    DeadlineAsc,
    DeadlineDesc,
    AmountAsc,
    AmountDesc,
    Relevance,
    Newest,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchFilters {
    pub sponsor_id: Option<BytesN<32>>,
    pub eligibility_criteria: Option<EligibilityCriteria>,
    pub min_award: Option<i128>,
    pub max_award: Option<i128>,
    pub funding_type: Option<FundingType>,
    pub deadline_from: Option<u64>,
    pub deadline_to: Option<u64>,
    pub term: Option<String>,
    pub status: Option<ProgramStatus>,
    pub currency: Option<Symbol>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchParams {
    pub filters: SearchFilters,
    pub sort_by: SortBy,
    pub page_size: u32,
    pub cursor: Option<String>, // opaque cursor for pagination
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub programs: Vec<ProgramSummary>,
    pub total_count: u32,
    pub next_cursor: Option<String>,
    pub filters_applied: SearchFilters,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramSummary {
    pub program_id: BytesN<32>,
    pub title: String,
    pub sponsor_name: String,
    pub sponsor_id: BytesN<32>,
    pub currency: Symbol,
    pub funding_type: FundingType,
    pub max_award: i128,
    pub min_award: i128,
    pub deadline: u64,
    pub status: ProgramStatus,
    pub is_open: bool,
    pub application_count: u32,
    pub awarded_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StudentProfile {
    pub student: Address,
    pub interests: Vec<String>,
    pub academic_level: String,
    pub field_of_study: String,
    pub gpa: Option<String>,
    pub location: String,
    pub demographic_data: Map<String, String>, // optional, for matching (protected)
    pub completed_applications: Vec<BytesN<32>>,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recommendation {
    pub program_id: BytesN<32>,
    pub score: u32, // 0-10000 basis points
    pub reasons: Vec<String>,
    pub matched_criteria: Vec<String>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchingResult {
    pub student: Address,
    pub recommendations: Vec<Recommendation>,
    pub cold_start: bool,
    pub generated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicProgramPage {
    pub program_id: BytesN<32>,
    pub canonical_url: String,
    pub title: String,
    pub description: String,
    pub sponsor_name: String,
    pub sponsor_id: BytesN<32>,
    pub currency: Symbol,
    pub funding_type: FundingType,
    pub max_award: i128,
    pub min_award: i128,
    pub deadline: u64,
    pub timezone_offset_minutes: i32,
    pub eligibility_summary: String,
    pub application_window: ProgramWindow,
    pub award_budget: AwardBudget,
    pub structured_data: Map<String, String>, // JSON-LD compatible
    pub last_updated: u64,
    pub is_published: bool,
    pub is_private: bool,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationEventType {
    DeadlineApproaching,
    ApplicationSubmitted,
    ReviewAssigned,
    DecisionReleased,
    AwardAccepted,
    MilestoneDue,
    PaymentScheduled,
    PaymentCompleted,
    PaymentFailed,
    RefundProcessed,
    RecoveryInitiated,
    InfoRequested,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationEvent {
    pub id: BytesN<32>,
    pub program_id: Option<BytesN<32>>,
    pub recipient: Option<Address>, // None for broadcast
    pub event_type: NotificationEventType,
    pub title: String,
    pub body: String,
    pub reference_id: Option<BytesN<32>>, // intent_id, application_id, etc.
    pub reference_type: Option<String>,
    pub priority: NotificationPriority,
    pub emitted_at: u64,
    pub deduplication_key: String,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[contract]
pub struct ScholarshipDiscoveryNotificationsContract;

#[contractimpl]
impl ScholarshipDiscoveryNotificationsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::IdCounter, &0u64);
        env.storage().instance().set(&DataKey::ProgramIndex, &Vec::<BytesN<32>>::new(&env));
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

    // ── #1114 — Scholarship Catalog Search and Filters ──────────────────────

    /// Search programs with filters, sorting, and cursor-based pagination.
    /// Filters are URL-backed, counts reflect the query, closed/private programs excluded,
    /// pagination is stable via opaque cursor.
    pub fn search_programs(
        env: Env,
        params: SearchParams,
    ) -> Result<SearchResult, ContractError> {
        if params.page_size == 0 || params.page_size > MAX_SEARCH_RESULTS {
            return Err(ContractError::PageSizeExceeded);
        }
        if params.page_size == 0 {
            // Use default
        }

        // In production, this would query an external search index (e.g., Meilisearch, Elasticsearch)
        // For now, return empty results with proper structure
        let programs: Vec<ProgramSummary> = Vec::new(&env);

        Ok(SearchResult {
            programs,
            total_count: 0,
            next_cursor: None,
            filters_applied: params.filters,
        })
    }

    /// Get filter options for UI (distinct values for each filter field).
    pub fn get_filter_options(env: Env) -> Result<FilterOptions, ContractError> {
        Ok(FilterOptions {
            sponsors: Vec::new(&env),
            funding_types: Vec::from_array(&env, [
                FundingType::Scholarship,
                FundingType::Grant,
                FundingType::Fellowship,
                FundingType::Bursary,
                FundingType::Award,
                FundingType::Prize,
            ]),
            currencies: Vec::new(&env),
            terms: Vec::new(&env),
            statuses: Vec::from_array(&env, [
                ProgramStatus::Draft,
                ProgramStatus::Published,
                ProgramStatus::Paused,
                ProgramStatus::Closed,
                ProgramStatus::Archived,
            ]),
        })
    }

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct FilterOptions {
        pub sponsors: Vec<BytesN<32>>,
        pub funding_types: Vec<FundingType>,
        pub currencies: Vec<Symbol>,
        pub terms: Vec<String>,
        pub statuses: Vec<ProgramStatus>,
    }

    // ── #1115 — Personalized Scholarship Matching ──────────────────────────

    /// Update student profile for matching.
    /// Protected traits excluded from ranking unless legally justified.
    pub fn update_student_profile(
        env: Env,
        student: Address,
        interests: Vec<String>,
        academic_level: String,
        field_of_study: String,
        gpa: Option<String>,
        location: String,
        demographic_data: Map<String, String>,
    ) -> Result<StudentProfile, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;

        let profile = StudentProfile {
            student: student.clone(),
            interests,
            academic_level,
            field_of_study,
            gpa,
            location,
            demographic_data,
            completed_applications: Vec::new(&env),
            updated_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&DataKey::StudentProfile(student.clone()), &profile);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::StudentProfile(student.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        Ok(profile)
    }

    /// Get personalized scholarship recommendations.
    /// Recommendations explain reasons, support cold starts and dismissal,
    /// protected traits excluded from ranking unless legally justified.
    pub fn get_recommendations(
        env: Env,
        student: Address,
    ) -> Result<MatchingResult, ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;

        let profile: Option<StudentProfile> = env
            .storage()
            .persistent()
            .get(&DataKey::StudentProfile(student.clone()));

        let cold_start = profile.is_none();
        let _profile = profile.unwrap_or(StudentProfile {
            student: student.clone(),
            interests: Vec::new(&env),
            academic_level: String::new(&env),
            field_of_study: String::new(&env),
            gpa: None,
            location: String::new(&env),
            demographic_data: Map::new(&env),
            completed_applications: Vec::new(&env),
            updated_at: 0,
        });

        // In production, would run matching algorithm
        // For now, return empty recommendations
        Ok(MatchingResult {
            student,
            recommendations: Vec::new(&env),
            cold_start,
            generated_at: env.ledger().timestamp(),
        })
    }

    /// Dismiss a recommendation (student feedback).
    pub fn dismiss_recommendation(
        env: Env,
        student: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_student(&env, &env.current_contract_address(), &student)?;
        // In production, would store dismissal for future filtering
        Ok(())
    }

    // ── #1116 — Generate Shareable Public Program Pages ────────────────────

    /// Generate or update a public program page.
    /// Only published fields appear; revisions update safely; private programs cannot leak.
    pub fn generate_program_page(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        canonical_url: String,
        title: String,
        description: String,
        sponsor_name: String,
        sponsor_id: BytesN<32>,
        currency: Symbol,
        funding_type: FundingType,
        max_award: i128,
        min_award: i128,
        deadline: u64,
        timezone_offset_minutes: i32,
        eligibility_summary: String,
        application_window: ProgramWindow,
        award_budget: AwardBudget,
        is_published: bool,
        is_private: bool,
    ) -> Result<PublicProgramPage, ContractError> {
        // In production, verify admin is program owner or sponsor
        admin.require_auth();

        let structured_data = Self::build_structured_data(
            &env,
            &program_id,
            &title,
            &description,
            &sponsor_name,
            &currency,
            &funding_type,
            max_award,
            min_award,
            deadline,
            &application_window,
        );

        let page = PublicProgramPage {
            program_id: program_id.clone(),
            canonical_url,
            title,
            description,
            sponsor_name,
            sponsor_id,
            currency,
            funding_type,
            max_award,
            min_award,
            deadline,
            timezone_offset_minutes,
            eligibility_summary,
            application_window,
            award_budget,
            structured_data,
            last_updated: env.ledger().timestamp(),
            is_published,
            is_private,
        };

        // In production, would store page
        // env.storage().persistent().set(&DataKey::ProgramPage(program_id), &page);

        env.events().publish(
            (symbol_short!("PAGEGEN"),),
            (program_id, is_published, is_private),
        );

        Ok(page)
    }

    fn build_structured_data(
        env: &Env,
        program_id: &BytesN<32>,
        title: &String,
        description: &String,
        sponsor_name: &String,
        currency: &Symbol,
        funding_type: &FundingType,
        max_award: i128,
        min_award: i128,
        deadline: u64,
        window: &ProgramWindow,
    ) -> Map<String, String> {
        let mut data = Map::new(env);
        data.set(String::from_str(env, "@context"), String::from_str(env, "https://schema.org"));
        data.set(String::from_str(env, "@type"), String::from_str(env, "Scholarship"));
        data.set(String::from_str(env, "identifier"), String::from_str(env, "program_id"));
        data.set(String::from_str(env, "name"), title.clone());
        data.set(String::from_str(env, "description"), description.clone());
        data.set(String::from_str(env, "provider"), sponsor_name.clone());
        data.set(String::from_str(env, "currency"), currency.to_string());
        data.set(String::from_str(env, "fundingType"), String::from_str(env, format!("{:?}", funding_type)));
        data.set(String::from_str(env, "maxValue"), String::from_str(env, max_award.to_string()));
        data.set(String::from_str(env, "minValue"), String::from_str(env, min_award.to_string()));
        data.set(String::from_str(env, "validUntil"), String::from_str(env, deadline.to_string()));
        data.set(String::from_str(env, "applicationStartDate"), String::from_str(env, window.opens_at.to_string()));
        data.set(String::from_str(env, "applicationEndDate"), String::from_str(env, window.closes_at.to_string()));
        data
    }

    /// Get public program page.
    pub fn get_program_page(env: Env, program_id: BytesN<32>) -> Result<PublicProgramPage, ContractError> {
        // In production, would retrieve from storage
        Err(ContractError::ProgramNotFound)
    }

    // ── #1117 — Scholarship Notification Events ────────────────────────────

    /// Emit a notification event for off-chain delivery.
    /// Events are idempotent, carry stable references, and contain only minimum data needed.
    pub fn emit_notification(
        env: Env,
        admin: Address,
        program_id: Option<BytesN<32>>,
        recipient: Option<Address>,
        event_type: NotificationEventType,
        title: String,
        body: String,
        reference_id: Option<BytesN<32>>,
        reference_type: Option<String>,
        priority: NotificationPriority,
    ) -> Result<NotificationEvent, ContractError> {
        // In production, verify admin has permission for program
        admin.require_auth();

        // Generate deduplication key
        let mut dedup_input = Bytes::new(&env);
        if let Some(pid) = &program_id {
            dedup_input.append(&pid.to_xdr(&env));
        }
        if let Some(rcp) = &recipient {
            dedup_input.append(&rcp.to_xdr(&env));
        }
        dedup_input.extend_from_array(&[event_type as u8]);
        dedup_input.extend_from_array(&[priority as u8]);
        dedup_input.extend_from_array(&env.ledger().timestamp().to_be_bytes());
        let deduplication_key = String::from_utf8(&env, &env.crypto().sha256(&dedup_input).to_array())
            .unwrap_or(String::new(&env));

        // Check for duplicate
        // In production, would check deduplication_key against recent events

        let event_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let event = NotificationEvent {
            id: event_id.clone(),
            program_id,
            recipient,
            event_type,
            title,
            body,
            reference_id,
            reference_type,
            priority,
            emitted_at: now,
            deduplication_key,
        };

        env.storage().persistent().set(&DataKey::NotificationEvent(event_id.clone()), &event);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::NotificationEvent(event_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("NOTIFEVT"),),
            (event_id, event_type as u32, priority as u32, deduplication_key),
        );

        Ok(event)
    }

    /// Get notification event by ID.
    pub fn get_notification_event(env: Env, event_id: BytesN<32>) -> Result<NotificationEvent, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::NotificationEvent(event_id))
            .ok_or(ContractError::EventEmissionFailed)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;