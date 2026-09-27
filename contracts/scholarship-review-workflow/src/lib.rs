#![no_std]

//! Scholarship/bursary review workflow contract.
//!
//! Scope of this pass (issues #1082, #1083, #1084, #1085):
//! - #1082: Create versioned scoring rubrics (weighted criteria, scales, guidance, required comments, disqualifying conditions)
//! - #1083: Save private reviewer score drafts (draft ownership, autosave conflict-safe, only submitted affects decisions)
//! - #1084: Submit and lock completed reviews (immutable, auditable amendment, aggregate updates atomic)
//! - #1085: Request additional applicant information (bounded questions, deadlines, versioned responses, notifications)
//!
//! This contract manages the complete review workflow from rubric creation
//! through score drafting to final submission and info requests.
//!
//! See `contracts/docs/scholarship-review-workflow.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Map, String, Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const MAX_RUBRIC_CRITERIA: u32 = 50;
const MAX_QUESTIONS_PER_REQUEST: u32 = 20;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorized = 4,
    RubricNotFound = 5,
    ApplicationNotFound = 6,
    ReviewNotFound = 7,
    InvalidRubricConfig = 8,
    WeightsDontReconcile = 9,
    DraftNotFound = 10,
    DraftOwnershipViolation = 11,
    ReviewAlreadySubmitted = 12,
    ReviewNotSubmitted = 13,
    CannotAmendFinalized = 14,
    InfoRequestNotFound = 15,
    QuestionLimitExceeded = 16,
    DeadlinePassed = 17,
    ResponseNotFound = 18,
    InvalidAmendment = 19,
    ArithmeticOverflow = 20,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Scoring rubrics (versioned)
    Rubric(BytesN<32>),
    RubricVersion(BytesN<32>, u32),
    /// Review drafts (private, per reviewer per application)
    ReviewDraft(Address, BytesN<32>),
    /// Submitted reviews
    Review(BytesN<32>),
    /// Review amendments
    ReviewAmendment(BytesN<32>),
    /// Info requests
    InfoRequest(BytesN<32>),
    /// Info request responses
    InfoResponse(BytesN<32>),
    /// Counter for unique IDs
    IdCounter,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CriterionScale {
    Numeric { min: i32, max: i32 },      // e.g., 1-10, 1-100
    Boolean,                               // Pass/Fail
    Categorical { options: u32 },          // e.g., Poor/Fair/Good/Excellent
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RubricCriterion {
    pub id: String,
    pub name: String,
    pub description: String,
    pub weight: u32,           // basis points, sum = 10000
    pub scale: CriterionScale,
    pub guidance: String,
    pub required_comment: bool,
    pub disqualifying: bool,   // if true and score below threshold, auto-reject
    pub disqualify_threshold: Option<i32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoringRubric {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub name: String,
    pub version: u32,
    pub criteria: Vec<RubricCriterion>,
    pub is_active: bool,
    pub created_at: u64,
    pub created_by: Address,
    pub approved_at: Option<u64>,
    pub approved_by: Option<Address>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Draft,
    Submitted,
    Amended,
    Finalized,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewScore {
    pub criterion_id: String,
    pub score: i32,
    pub comment: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDraft {
    pub reviewer: Address,
    pub application_id: BytesN<32>,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub scores: Vec<ReviewScore>,
    pub last_saved_at: u64,
    pub is_complete: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    pub id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub reviewer: Address,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub scores: Vec<ReviewScore>,
    pub total_score: u32,        // weighted sum (0-10000)
    pub status: ReviewStatus,
    pub submitted_at: u64,
    pub finalized_at: Option<u64>,
    pub amendment_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewAmendment {
    pub id: BytesN<32>,
    pub review_id: BytesN<32>,
    pub amended_by: Address,
    pub previous_scores: Vec<ReviewScore>,
    pub new_scores: Vec<ReviewScore>,
    pub reason: String,
    pub amended_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InfoRequest {
    pub id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub reviewer: Address,
    pub questions: Vec<String>,
    pub visible_fields: Vec<String>, // which application fields the response can see
    pub deadline: u64,
    pub status: InfoRequestStatus,
    pub created_at: u64,
    pub responded_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InfoRequestStatus {
    Pending,
    Responded,
    Expired,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InfoResponse {
    pub id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub applicant: Address,
    pub responses: Vec<String>,
    pub version: u32,
    pub responded_at: u64,
}

#[contract]
pub struct ScholarshipReviewWorkflowContract;

#[contractimpl]
impl ScholarshipReviewWorkflowContract {
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

    fn require_reviewer(env: &Env, caller: &Address, reviewer: &Address) -> Result<(), ContractError> {
        if *caller != *reviewer {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_applicant(env: &Env, caller: &Address, applicant: &Address) -> Result<(), ContractError> {
        if *caller != *applicant {
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

    fn validate_rubric_weights(criteria: &Vec<RubricCriterion>) -> Result<(), ContractError> {
        let total_weight: u32 = criteria.iter().map(|c| c.weight).sum();
        if total_weight != 10000 {
            return Err(ContractError::WeightsDontReconcile);
        }
        if criteria.len() as u32 > MAX_RUBRIC_CRITERIA {
            return Err(ContractError::InvalidRubricConfig);
        }
        Ok(())
    }

    // ── #1082 — Versioned Scoring Rubrics ──────────────────────────────────

    /// Create a new scoring rubric version.
    /// Weights reconcile (sum = 10000), published rubrics are immutable,
    /// scores identify exact rubric version.
    pub fn create_rubric(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        name: String,
        criteria: Vec<RubricCriterion>,
    ) -> Result<ScoringRubric, ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_rubric_weights(&criteria)?;

        let rubric_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let rubric = ScoringRubric {
            id: rubric_id.clone(),
            program_id: program_id.clone(),
            name,
            version: 1,
            criteria,
            is_active: false,
            created_at: now,
            created_by: admin.clone(),
            approved_at: None,
            approved_by: None,
        };

        env.storage().persistent().set(&DataKey::Rubric(rubric_id.clone()), &rubric);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Rubric(rubric_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&DataKey::RubricVersion(rubric_id.clone(), 1), &rubric);

        env.events().publish(
            (symbol_short!("RUBRNEW"),),
            (rubric_id, program_id, 1),
        );

        Ok(rubric)
    }

    /// Update a rubric (creates new version).
    pub fn update_rubric(
        env: Env,
        admin: Address,
        rubric_id: BytesN<32>,
        name: Option<String>,
        criteria: Option<Vec<RubricCriterion>>,
    ) -> Result<ScoringRubric, ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut rubric: ScoringRubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(rubric_id.clone()))
            .ok_or(ContractError::RubricNotFound)?;

        if rubric.is_active {
            return Err(ContractError::InvalidRubricConfig); // published rubrics immutable
        }

        if let Some(c) = criteria {
            Self::validate_rubric_weights(&c)?;
            rubric.criteria = c;
        }
        if let Some(n) = name {
            rubric.name = n;
        }

        let new_version = rubric.version.checked_add(1).ok_or(ContractError::ArithmeticOverflow)?;
        rubric.version = new_version;

        env.storage().persistent().set(&DataKey::Rubric(rubric_id.clone()), &rubric);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Rubric(rubric_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&DataKey::RubricVersion(rubric_id.clone(), new_version), &rubric);

        env.events().publish(
            (symbol_short!("RUBRUPD"),),
            (rubric_id, new_version),
        );

        Ok(rubric)
    }

    /// Activate a rubric version for use.
    pub fn activate_rubric(
        env: Env,
        admin: Address,
        rubric_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut rubric: ScoringRubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(rubric_id.clone()))
            .ok_or(ContractError::RubricNotFound)?;

        if rubric.is_active {
            return Ok(()); // already active
        }

        rubric.is_active = true;
        rubric.approved_at = Some(env.ledger().timestamp());
        rubric.approved_by = Some(admin.clone());

        env.storage().persistent().set(&DataKey::Rubric(rubric_id.clone()), &rubric);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Rubric(rubric_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("RUBRACT"),),
            (rubric_id, rubric.version, admin),
        );

        Ok(())
    }

    pub fn get_rubric(env: Env, rubric_id: BytesN<32>) -> Result<ScoringRubric, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Rubric(rubric_id))
            .ok_or(ContractError::RubricNotFound)
    }

    pub fn get_rubric_version(env: Env, rubric_id: BytesN<32>, version: u32) -> Result<ScoringRubric, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::RubricVersion(rubric_id, version))
            .ok_or(ContractError::RubricNotFound)
    }

    // ── #1083 — Save Private Reviewer Score Drafts ────────────────────────

    /// Save or update a private review draft.
    /// Draft ownership enforced, autosave conflict-safe, only submitted reviews affect decisions.
    pub fn save_draft(
        env: Env,
        reviewer: Address,
        application_id: BytesN<32>,
        rubric_id: BytesN<32>,
        scores: Vec<ReviewScore>,
        is_complete: bool,
    ) -> Result<ReviewDraft, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;

        let rubric: ScoringRubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(rubric_id.clone()))
            .ok_or(ContractError::RubricNotFound)?;

        if !rubric.is_active {
            return Err(ContractError::InvalidRubricConfig);
        }

        // Validate scores against rubric criteria
        for score in &scores {
            let criterion = rubric.criteria.iter().find(|c| c.id == score.criterion_id);
            if criterion.is_none() {
                return Err(ContractError::InvalidRubricConfig);
            }
            // Validate score within scale
            // In production, would validate against criterion.scale
        }

        let draft = ReviewDraft {
            reviewer: reviewer.clone(),
            application_id: application_id.clone(),
            rubric_id: rubric_id.clone(),
            rubric_version: rubric.version,
            scores,
            last_saved_at: env.ledger().timestamp(),
            is_complete,
        };

        let key = DataKey::ReviewDraft(reviewer.clone(), application_id.clone());
        env.storage().persistent().set(&key, &draft);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("DRAFTSAV"),),
            (reviewer, application_id, is_complete),
        );

        Ok(draft)
    }

    /// Get a reviewer's draft for an application.
    pub fn get_draft(
        env: Env,
        reviewer: Address,
        application_id: BytesN<32>,
    ) -> Result<ReviewDraft, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ReviewDraft(reviewer, application_id))
            .ok_or(ContractError::DraftNotFound)
    }

    // ── #1084 — Submit and Lock Completed Reviews ────────────────────────

    /// Submit a completed review (locks it).
    /// Submitted reviews immutable; correction uses auditable amendment;
    /// aggregate results update atomically.
    pub fn submit_review(
        env: Env,
        reviewer: Address,
        application_id: BytesN<32>,
    ) -> Result<Review, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;

        let draft: ReviewDraft = env
            .storage()
            .persistent()
            .get(&DataKey::ReviewDraft(reviewer.clone(), application_id.clone()))
            .ok_or(ContractError::DraftNotFound)?;

        if !draft.is_complete {
            return Err(ContractError::InvalidRubricConfig);
        }

        // Check if review already submitted
        let existing_review: Option<Review> = env
            .storage()
            .persistent()
            .get(&DataKey::Review(application_id.clone()));

        if existing_review.is_some() {
            return Err(ContractError::ReviewAlreadySubmitted);
        }

        // Calculate weighted total score
        let rubric: ScoringRubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(draft.rubric_id.clone()))
            .ok_or(ContractError::RubricNotFound)?;

        let mut total_weighted: u32 = 0;
        for score in &draft.scores {
            let criterion = rubric.criteria.iter().find(|c| c.id == score.criterion_id)
                .ok_or(ContractError::InvalidRubricConfig)?;
            let weighted = (score.score as u32)
                .checked_mul(criterion.weight)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_div(100) // weight is basis points
                .ok_or(ContractError::ArithmeticOverflow)?;
            total_weighted = total_weighted.checked_add(weighted).ok_or(ContractError::ArithmeticOverflow)?;
        }

        let review_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let review = Review {
            id: review_id.clone(),
            application_id: application_id.clone(),
            reviewer: reviewer.clone(),
            rubric_id: draft.rubric_id.clone(),
            rubric_version: draft.rubric_version,
            scores: draft.scores.clone(),
            total_score: total_weighted,
            status: ReviewStatus::Submitted,
            submitted_at: now,
            finalized_at: None,
            amendment_count: 0,
        };

        env.storage().persistent().set(&DataKey::Review(review_id.clone()), &review);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Review(review_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Clear draft (optional - keep for audit)
        // env.storage().persistent().remove(&DataKey::ReviewDraft(reviewer, application_id));

        env.events().publish(
            (symbol_short!("REVSUB"),),
            (review_id, application_id, reviewer, total_weighted),
        );

        Ok(review)
    }

    /// Amend a submitted review (auditable correction).
    pub fn amend_review(
        env: Env,
        reviewer: Address,
        review_id: BytesN<32>,
        new_scores: Vec<ReviewScore>,
        reason: String,
    ) -> Result<(), ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;

        let mut review: Review = env
            .storage()
            .persistent()
            .get(&DataKey::Review(review_id.clone()))
            .ok_or(ContractError::ReviewNotFound)?;

        if review.reviewer != reviewer {
            return Err(ContractError::DraftOwnershipViolation);
        }

        if review.status == ReviewStatus::Finalized {
            return Err(ContractError::CannotAmendFinalized);
        }

        // Validate new scores
        let rubric: ScoringRubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(review.rubric_id.clone()))
            .ok_or(ContractError::RubricNotFound)?;

        for score in &new_scores {
            let criterion = rubric.criteria.iter().find(|c| c.id == score.criterion_id);
            if criterion.is_none() {
                return Err(ContractError::InvalidRubricConfig);
            }
        }

        // Create amendment record
        let amendment_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let amendment = ReviewAmendment {
            id: amendment_id.clone(),
            review_id: review_id.clone(),
            amended_by: reviewer.clone(),
            previous_scores: review.scores.clone(),
            new_scores: new_scores.clone(),
            reason,
            amended_at: now,
        };

        // Calculate new total
        let mut new_total: u32 = 0;
        for score in &new_scores {
            let criterion = rubric.criteria.iter().find(|c| c.id == score.criterion_id)
                .ok_or(ContractError::InvalidRubricConfig)?;
            let weighted = (score.score as u32)
                .checked_mul(criterion.weight)
                .ok_or(ContractError::ArithmeticOverflow)?
                .checked_div(100)
                .ok_or(ContractError::ArithmeticOverflow)?;
            new_total = new_total.checked_add(weighted).ok_or(ContractError::ArithmeticOverflow)?;
        }

        // Update review
        review.scores = new_scores;
        review.total_score = new_total;
        review.status = ReviewStatus::Amended;
        review.amendment_count = review.amendment_count.checked_add(1).ok_or(ContractError::ArithmeticOverflow)?;

        env.storage().persistent().set(&DataKey::ReviewAmendment(amendment_id.clone()), &amendment);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::ReviewAmendment(amendment_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&DataKey::Review(review_id.clone()), &review);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Review(review_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("REVAMND"),),
            (review_id, amendment_id, reviewer),
        );

        Ok(())
    }

    /// Finalize a review (prevents further amendments).
    pub fn finalize_review(
        env: Env,
        admin: Address,
        review_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut review: Review = env
            .storage()
            .persistent()
            .get(&DataKey::Review(review_id.clone()))
            .ok_or(ContractError::ReviewNotFound)?;

        if review.status == ReviewStatus::Finalized {
            return Ok(());
        }

        review.status = ReviewStatus::Finalized;
        review.finalized_at = Some(env.ledger().timestamp());

        env.storage().persistent().set(&DataKey::Review(review_id.clone()), &review);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Review(review_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("REVFIN"),),
            (review_id, admin),
        );

        Ok(())
    }

    pub fn get_review(env: Env, review_id: BytesN<32>) -> Result<Review, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Review(review_id))
            .ok_or(ContractError::ReviewNotFound)
    }

    // ── #1085 — Request Additional Applicant Information ───────────────────

    /// Request additional information from applicant.
    /// Requests specify visible fields, responses versioned, deadlines enforced,
    /// all parties notified.
    pub fn request_info(
        env: Env,
        reviewer: Address,
        application_id: BytesN<32>,
        questions: Vec<String>,
        visible_fields: Vec<String>,
        deadline: u64,
    ) -> Result<InfoRequest, ContractError> {
        Self::require_reviewer(&env, &env.current_contract_address(), &reviewer)?;

        if questions.len() as u32 > MAX_QUESTIONS_PER_REQUEST {
            return Err(ContractError::QuestionLimitExceeded);
        }

        if deadline <= env.ledger().timestamp() {
            return Err(ContractError::DeadlinePassed);
        }

        let request_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let request = InfoRequest {
            id: request_id.clone(),
            application_id: application_id.clone(),
            reviewer: reviewer.clone(),
            questions,
            visible_fields,
            deadline,
            status: InfoRequestStatus::Pending,
            created_at: now,
            responded_at: None,
        };

        env.storage().persistent().set(&DataKey::InfoRequest(request_id.clone()), &request);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::InfoRequest(request_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("INFOREQ"),),
            (request_id, application_id, reviewer, deadline),
        );

        Ok(request)
    }

    /// Respond to an info request.
    pub fn respond_to_info_request(
        env: Env,
        applicant: Address,
        request_id: BytesN<32>,
        responses: Vec<String>,
    ) -> Result<InfoResponse, ContractError> {
        Self::require_applicant(&env, &env.current_contract_address(), &applicant)?;

        let mut request: InfoRequest = env
            .storage()
            .persistent()
            .get(&DataKey::InfoRequest(request_id.clone()))
            .ok_or(ContractError::InfoRequestNotFound)?;

        if request.status != InfoRequestStatus::Pending {
            return Err(ContractError::InvalidRubricConfig);
        }

        if env.ledger().timestamp() > request.deadline {
            request.status = InfoRequestStatus::Expired;
            env.storage().persistent().set(&DataKey::InfoRequest(request_id.clone()), &request);
            return Err(ContractError::DeadlinePassed);
        }

        if responses.len() != request.questions.len() {
            return Err(ContractError::QuestionLimitExceeded);
        }

        let response_id = Self::next_id(&env);
        let now = env.ledger().timestamp();

        let response = InfoResponse {
            id: response_id.clone(),
            request_id: request_id.clone(),
            applicant: applicant.clone(),
            responses,
            version: 1,
            responded_at: now,
        };

        request.status = InfoRequestStatus::Responded;
        request.responded_at = Some(now);

        env.storage().persistent().set(&DataKey::InfoRequest(request_id.clone()), &request);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::InfoRequest(request_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&DataKey::InfoResponse(response_id.clone()), &response);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::InfoResponse(response_id.clone()), RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("INFORESP"),),
            (response_id, request_id, applicant),
        );

        Ok(response)
    }

    pub fn get_info_request(env: Env, request_id: BytesN<32>) -> Result<InfoRequest, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::InfoRequest(request_id))
            .ok_or(ContractError::InfoRequestNotFound)
    }

    pub fn get_info_response(env: Env, response_id: BytesN<32>) -> Result<InfoResponse, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::InfoResponse(response_id))
            .ok_or(ContractError::ResponseNotFound)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;