#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_SCORES: u32 = 16;
const MAX_REVIEWS_PER_SUBMISSION: u32 = 64;
const MAX_SCORE: u32 = 100;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const REQUEST_MIN_TTL: u32 = 1_555_200;
const REQUEST_MAX_TTL: u32 = 3_110_400;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidTransition = 6,
    StaleVersion = 7,
    VersionOverflow = 8,
    RequestConflict = 9,
    CollectionFull = 10,
    InvalidScore = 11,
    InvalidHash = 12,
    InvalidVersion = 13,
    ArithmeticOverflow = 14,
    InvalidReviewerRef = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Review(BytesN<32>, BytesN<32>, BytesN<32>),
    Scores(BytesN<32>, BytesN<32>, BytesN<32>),
    SubmissionIndex(BytesN<32>, BytesN<32>, BytesN<32>, u32),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Reviewer = 2,
    Coordinator = 3,
    Auditor = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recommendation {
    Approve = 1,
    Reject = 2,
    ChangesRequested = 3,
    Abstain = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    Low = 1,
    Medium = 2,
    High = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictState {
    NoConflict = 1,
    Declared = 2,
    Recused = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Submitted = 1,
    Withdrawn = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Score {
    pub criterion_id: BytesN<32>,
    pub points: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub review_id: BytesN<32>,
    pub proposal_id: BytesN<32>,
    pub submission_version: u32,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub reviewer: Address,
    pub reviewer_ref: BytesN<32>,
    pub recommendation: Recommendation,
    pub confidence: Confidence,
    pub conflict: ConflictState,
    pub status: ReviewStatus,
    pub version: u32,
    pub score_count: u32,
    pub comments_hash: BytesN<32>,
    pub submitted_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub payload_hash: BytesN<32>,
    pub result_id: BytesN<32>,
    pub result_version: u32,
    pub recorded_at: u64,
}

#[contract]
pub struct CapstoneIndependentReviewsContract;

#[contractimpl]
impl CapstoneIndependentReviewsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("RVGRANT"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().remove(&key);
        env.events()
            .publish((symbol_short!("RVRVOK"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn submit_review(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        review_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        proposal_id: BytesN<32>,
        submission_version: u32,
        rubric_id: BytesN<32>,
        rubric_version: u32,
        reviewer_ref: BytesN<32>,
        recommendation: Recommendation,
        confidence: Confidence,
        conflict: ConflictState,
        scores: Vec<Score>,
        comments_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if reviewer_ref == Self::zero_id(&env) {
            return Err(ContractError::InvalidReviewerRef);
        }
        if comments_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if submission_version == 0 || rubric_version == 0 {
            return Err(ContractError::InvalidVersion);
        }
        Self::validate_scores(&env, &scores)?;
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Review(tenant_id.clone(), project_id.clone(), review_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::SubmissionIndex(
            tenant_id.clone(),
            project_id.clone(),
            proposal_id.clone(),
            submission_version,
        );
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if index.len() >= MAX_REVIEWS_PER_SUBMISSION {
            return Err(ContractError::CollectionFull);
        }
        let now = env.ledger().timestamp();
        let review = Review {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            review_id: review_id.clone(),
            proposal_id,
            submission_version,
            rubric_id,
            rubric_version,
            reviewer: reviewer.clone(),
            reviewer_ref: reviewer_ref.clone(),
            recommendation,
            confidence,
            conflict,
            status: ReviewStatus::Submitted,
            version: 1,
            score_count: scores.len(),
            comments_hash,
            submitted_at: now,
            updated_at: now,
        };
        Self::store_review(&env, &key, &review);
        let scores_key = DataKey::Scores(tenant_id.clone(), project_id.clone(), review_id.clone());
        env.storage().persistent().set(&scores_key, &scores);
        env.storage()
            .persistent()
            .extend_ttl(&scores_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        index.push_back(review_id.clone());
        env.storage().persistent().set(&index_key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
            1,
            now,
        );
        env.events().publish(
            (symbol_short!("RVNEW"),),
            (tenant_id, project_id, review_id, 1u32),
        );
        Ok(1)
    }

    pub fn amend_review(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        review_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        recommendation: Recommendation,
        confidence: Confidence,
        conflict: ConflictState,
        scores: Vec<Score>,
        comments_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if comments_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        Self::validate_scores(&env, &scores)?;
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Review(tenant_id.clone(), project_id.clone(), review_id.clone());
        let mut review: Review = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if review.reviewer != reviewer {
            return Err(ContractError::Unauthorized);
        }
        if review.status != ReviewStatus::Submitted {
            return Err(ContractError::InvalidTransition);
        }
        if review.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let version = review
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        review.version = version;
        review.recommendation = recommendation;
        review.confidence = confidence;
        review.conflict = conflict;
        review.score_count = scores.len();
        review.comments_hash = comments_hash;
        review.updated_at = now;
        Self::store_review(&env, &key, &review);
        let scores_key = DataKey::Scores(tenant_id.clone(), project_id.clone(), review_id.clone());
        env.storage().persistent().set(&scores_key, &scores);
        env.storage()
            .persistent()
            .extend_ttl(&scores_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("RVAMD"),),
            (tenant_id, project_id, review_id, version),
        );
        Ok(version)
    }

    pub fn withdraw_review(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        review_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Review(tenant_id.clone(), project_id.clone(), review_id.clone());
        let mut review: Review = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if review.reviewer != reviewer {
            return Err(ContractError::Unauthorized);
        }
        if review.status != ReviewStatus::Submitted {
            return Err(ContractError::InvalidTransition);
        }
        if review.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let version = review
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        review.version = version;
        review.status = ReviewStatus::Withdrawn;
        review.updated_at = now;
        Self::store_review(&env, &key, &review);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &review_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("RVWDR"),),
            (tenant_id, project_id, review_id, version),
        );
        Ok(version)
    }

    pub fn get_review(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        review_id: BytesN<32>,
    ) -> Option<Review> {
        env.storage()
            .persistent()
            .get(&DataKey::Review(tenant_id, project_id, review_id))
    }

    pub fn get_scores(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        review_id: BytesN<32>,
    ) -> Vec<Score> {
        env.storage()
            .persistent()
            .get(&DataKey::Scores(tenant_id, project_id, review_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_submission_review_ids(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        proposal_id: BytesN<32>,
        submission_version: u32,
    ) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::SubmissionIndex(
                tenant_id,
                project_id,
                proposal_id,
                submission_version,
            ))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn has_role(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        role: Role,
    ) -> bool {
        Self::has_stored_role(&env, &subject, &tenant_id, &project_id, role)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &configured != admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if let Some(configured) = env.storage().instance().get::<_, Address>(&DataKey::Admin) {
            if &configured == caller {
                return Ok(());
            }
        }
        let zero = Self::zero_id(env);
        if Self::has_stored_role(env, caller, tenant_id, &zero, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, role)
        {
            return Ok(());
        }
        Err(ContractError::Unauthorized)
    }

    fn has_stored_role(
        env: &Env,
        subject: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> bool {
        let key = DataKey::Role(subject.clone(), tenant_id.clone(), project_id.clone(), role);
        env.storage()
            .persistent()
            .get::<_, bool>(&key)
            .unwrap_or(false)
    }

    fn zero_id(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn validate_scores(env: &Env, scores: &Vec<Score>) -> Result<(), ContractError> {
        if scores.len() == 0 || scores.len() > MAX_SCORES {
            return Err(ContractError::InvalidScore);
        }
        let mut i = 0;
        while i < scores.len() {
            let score = scores.get(i).ok_or(ContractError::InvalidScore)?;
            if score.points > MAX_SCORE || score.criterion_id == Self::zero_id(env) {
                return Err(ContractError::InvalidScore);
            }
            i += 1;
        }
        Ok(())
    }

    fn replay_version(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(receipt) = env.storage().temporary().get::<_, RequestReceipt>(&key) {
            if &receipt.payload_hash != payload_hash || &receipt.result_id != result_id {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .temporary()
                .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
            return Ok(Some(receipt.result_version));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
        version: u32,
        now: u64,
    ) {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let receipt = RequestReceipt {
            payload_hash: payload_hash.clone(),
            result_id: result_id.clone(),
            result_version: version,
            recorded_at: now,
        };
        env.storage().temporary().set(&key, &receipt);
        env.storage()
            .temporary()
            .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
    }

    fn store_review(env: &Env, key: &DataKey, review: &Review) {
        env.storage().persistent().set(key, review);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
