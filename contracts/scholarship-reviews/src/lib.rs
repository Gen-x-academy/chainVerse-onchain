#![no_std]

//! Scholarship/bursary review aggregation contract.
//!
//! Scope of this pass (issue #1086): combine the reviews that several
//! reviewers submit for one application into a single, reproducible
//! normalized aggregate score, honoring a published rubric's per-criterion
//! weights, refusing to produce an aggregate when too few reviews exist,
//! and defining an explicit, deterministic tie policy for comparing two
//! applicants.
//!
//! Determinism/reproducibility: the aggregate for an application is a pure
//! function of immutable inputs — the applicant's stored `Review` records
//! (each holding only the numeric per-criterion scores and a submission
//! timestamp) and the published, immutable `Rubric` version they were
//! scored against. Nothing is read from off-chain state, and no randomness
//! or caller input participates, so anyone can recompute the same value.
//! The exact rounding rule is documented below and in
//! `contracts/docs/scholarship-reviews.md`.
//!
//! Precision and rounding (documented, integer-only):
//! 1. Each criterion score `s` in `[0, max_score]` is normalized to basis
//!    points of its own maximum: `normalized = s * 10_000 / max_score`,
//!    truncated toward zero.
//! 2. A review's weighted total is
//!    `sum(normalized_i * weight_i) / 10_000`, truncated toward zero.
//!    Weights are basis points and must sum to exactly `10_000`.
//! 3. The aggregate is `sum(review_totals) / review_count`, truncated toward
//!    zero. A review total and the aggregate are therefore both integers in
//!    `[0, 10_000]` (basis points), with floors applied at each step.
//!
//! Privacy-minimized by design: this contract stores only numeric scores and
//! reviewer addresses — never free-text comments, narrative feedback, or the
//! evidence backing a review. Those live entirely off-chain; the aggregate
//! is reproducible from the on-chain numeric inputs alone. See
//! `contracts/docs/scholarship-reviews.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Fixed basis-point denominator used for both weights and normalized scores.
const BPS_DENOMINATOR: u64 = 10_000;

/// Bounded storage: a rubric may define at most this many criteria, so
/// review size and aggregation cost stay predictable.
const MAX_CRITERIA: u32 = 12;

/// Bounded storage: each criterion's declared maximum score is capped so a
/// single score cannot dominate the weighted arithmetic.
const MAX_CRITERION_SCORE: u32 = 1_000;

/// Bounded storage: at most this many reviews may be recorded per application.
const MAX_REVIEWERS: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ProgramNotFound = 4,
    ProgramAlreadyExists = 5,
    ProgramInactive = 6,
    /// #1086 — min_reviews must be >= 1, max_reviews must be >= min_reviews
    /// and <= MAX_REVIEWERS.
    InvalidReviewConfig = 7,
    /// #1086 — a rubric must have 1..=MAX_CRITERIA criteria, each with a
    /// positive bounded max_score, and weights summing to exactly 10_000 bps.
    InvalidRubric = 8,
    NoRubricPublished = 9,
    RubricNotFound = 10,
    /// A reviewer may submit at most one review per (program, applicant).
    ReviewAlreadyExists = 11,
    /// The application already has MAX_REVIEWERS reviews.
    ReviewerCapacityExceeded = 12,
    /// Scores must align one-to-one with the rubric's criteria and each must
    /// be within its criterion's declared maximum.
    InvalidScores = 13,
    /// Fewer than the program's min_reviews have been recorded, so no
    /// aggregate can be produced (missing reviews are not invented).
    InsufficientReviews = 14,
    ReviewNotFound = 15,
    VersionOverflow = 16,
    ArithmeticOverflow = 17,
    /// Reviews for one application were scored against different rubric
    /// versions, so no single weighting can combine them reproducibly.
    /// Re-score the application against one rubric version to aggregate.
    MixedRubricVersions = 18,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1086 — a program's review configuration.
    ProgramConfig(BytesN<32>),
    /// #1086 — latest published rubric version for a program.
    RubricVersion(BytesN<32>),
    /// #1086 — an immutable, published rubric version.
    Rubric(BytesN<32>, u32),
    /// #1086 — one reviewer's review of one applicant for one program.
    Review(BytesN<32>, Address, Address),
    /// #1086 — bounded list of reviewers who have reviewed an applicant.
    Reviewers(BytesN<32>, Address),
}

/// #1086 — one weighted criterion of a rubric. `weight_bps` is in basis
/// points; all criteria of a rubric must sum to exactly 10_000.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Criterion {
    pub weight_bps: u32,
    pub max_score: u32,
}

/// #1086 — an immutable, published rubric version. Publishing again always
/// creates version N+1, so a review scored against an older version stays
/// reproducible against that exact version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rubric {
    pub version: u32,
    pub criteria: Vec<Criterion>,
    pub published_at: u64,
}

/// #1086 — a program's review policy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramConfig {
    pub active: bool,
    /// Minimum number of recorded reviews required before an aggregate may
    /// be computed. Unsubmitted ("missing") reviews are simply absent and
    /// never counted; this gate is what prevents an under-reviewed aggregate.
    pub min_reviews: u32,
    /// Maximum number of reviews accepted per application.
    pub max_reviews: u32,
}

/// #1086 — one reviewer's numeric review of one applicant. Only numeric
/// scores are stored; narrative feedback and evidence are off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    pub program_id: BytesN<32>,
    pub applicant: Address,
    pub reviewer: Address,
    /// The rubric version these scores were validated against. Aggregation
    /// uses this version, never the program's latest, so publishing a new
    /// rubric cannot retroactively reinterpret an existing review.
    pub rubric_version: u32,
    pub scores: Vec<u32>,
    pub submitted_at: u64,
}

/// #1086 — the deterministic tie policy's result for two applicants.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompareOutcome {
    /// The first applicant ranks strictly ahead.
    ABetter,
    /// The second applicant ranks strictly ahead.
    BBetter,
    /// The two applicants are indistinguishable under the documented policy.
    Tie,
}

#[contract]
pub struct ScholarshipReviewsContract;

#[contractimpl]
impl ScholarshipReviewsContract {
    /// Initialize the contract admin. Run once.
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

    // ── #1086 — program configuration ─────────────────────────────────────

    /// Admin-only: register a program whose reviews will be aggregated.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        min_reviews: u32,
        max_reviews: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if min_reviews == 0 || max_reviews < min_reviews || max_reviews > MAX_REVIEWERS {
            return Err(ContractError::InvalidReviewConfig);
        }

        let key = DataKey::ProgramConfig(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        let config = ProgramConfig {
            active: true,
            min_reviews,
            max_reviews,
        };
        env.storage().persistent().set(&key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("REVPROG"),), (program_id,));
        Ok(())
    }

    /// Admin-only: activate/deactivate review collection for a program.
    pub fn set_program_active(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::ProgramConfig(program_id.clone());
        let mut config: ProgramConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        config.active = active;

        env.storage().persistent().set(&key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("REVACT"),),
            (program_id, active),
        );
        Ok(())
    }

    pub fn get_program_config(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    // ── #1086 — weighted rubrics ──────────────────────────────────────────

    /// Admin-only: publish a new, immutable rubric version for a program.
    /// Validates bounds and that weights sum to exactly 10_000 bps *before*
    /// publishing; publishing again always creates version N+1 and never
    /// mutates an existing version.
    pub fn publish_rubric(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        criteria: Vec<Criterion>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;

        if !env
            .storage()
            .persistent()
            .has(&DataKey::ProgramConfig(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }

        Self::validate_rubric(&criteria)?;

        let version_key = DataKey::RubricVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let rubric = Rubric {
            version: next_version,
            criteria,
            published_at: env.ledger().timestamp(),
        };

        let rubric_key = DataKey::Rubric(program_id.clone(), next_version);
        env.storage().persistent().set(&rubric_key, &rubric);
        env.storage()
            .persistent()
            .extend_ttl(&rubric_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&version_key, &next_version);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("RUBPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    fn validate_rubric(criteria: &Vec<Criterion>) -> Result<(), ContractError> {
        let count = criteria.len();
        if count == 0 || count > MAX_CRITERIA {
            return Err(ContractError::InvalidRubric);
        }

        let mut total_weight: u64 = 0;
        let mut i: u32 = 0;
        while i < count {
            let criterion = criteria.get(i).ok_or(ContractError::InvalidRubric)?;
            if criterion.max_score == 0 || criterion.max_score > MAX_CRITERION_SCORE {
                return Err(ContractError::InvalidRubric);
            }
            total_weight = total_weight
                .checked_add(criterion.weight_bps as u64)
                .ok_or(ContractError::ArithmeticOverflow)?;
            i += 1;
        }

        if total_weight != BPS_DENOMINATOR {
            return Err(ContractError::InvalidRubric);
        }
        Ok(())
    }

    pub fn get_latest_rubric_version(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::RubricVersion(program_id))
            .ok_or(ContractError::NoRubricPublished)
    }

    pub fn get_rubric(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<Rubric, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Rubric(program_id, version))
            .ok_or(ContractError::RubricNotFound)
    }

    fn latest_rubric(env: &Env, program_id: &BytesN<32>) -> Result<Rubric, ContractError> {
        let version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::RubricVersion(program_id.clone()))
            .ok_or(ContractError::NoRubricPublished)?;
        env.storage()
            .persistent()
            .get(&DataKey::Rubric(program_id.clone(), version))
            .ok_or(ContractError::RubricNotFound)
    }

    // ── #1086 — submitting reviews ────────────────────────────────────────

    /// Reviewer-only: record one reviewer's numeric review of one applicant.
    /// Returns the review's weighted total (basis points) so callers can
    /// surface it without a second read. Rejects duplicates (one review per
    /// reviewer per applicant) and enforces the program's max_reviews bound.
    pub fn submit_review(
        env: Env,
        reviewer: Address,
        program_id: BytesN<32>,
        applicant: Address,
        scores: Vec<u32>,
    ) -> Result<u32, ContractError> {
        reviewer.require_auth();

        let config: ProgramConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;
        if !config.active {
            return Err(ContractError::ProgramInactive);
        }

        let rubric = Self::latest_rubric(&env, &program_id)?;

        let review_key = DataKey::Review(program_id.clone(), applicant.clone(), reviewer.clone());
        if env.storage().persistent().has(&review_key) {
            return Err(ContractError::ReviewAlreadyExists);
        }

        let reviewers_key = DataKey::Reviewers(program_id.clone(), applicant.clone());
        let mut reviewers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&reviewers_key)
            .unwrap_or(Vec::new(&env));
        if reviewers.len() >= config.max_reviews {
            return Err(ContractError::ReviewerCapacityExceeded);
        }

        let total = Self::weighted_total(&rubric, &scores)?;

        let review = Review {
            program_id: program_id.clone(),
            applicant: applicant.clone(),
            reviewer: reviewer.clone(),
            rubric_version: rubric.version,
            scores,
            submitted_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&review_key, &review);
        env.storage()
            .persistent()
            .extend_ttl(&review_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        reviewers.push_back(reviewer.clone());
        env.storage().persistent().set(&reviewers_key, &reviewers);
        env.storage()
            .persistent()
            .extend_ttl(&reviewers_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("REVIEW"),),
            (program_id, applicant, reviewer, total),
        );

        Ok(total)
    }

    /// The documented integer weighting for one review (see the module docs
    /// for the exact precision/rounding rules). Pure function of the rubric
    /// and the review's scores — no storage reads — so it is trivially
    /// reproducible off-chain.
    fn weighted_total(rubric: &Rubric, scores: &Vec<u32>) -> Result<u32, ContractError> {
        let count = rubric.criteria.len();
        if scores.len() != count {
            return Err(ContractError::InvalidScores);
        }

        let mut acc: u64 = 0;
        let mut i: u32 = 0;
        while i < count {
            let criterion = rubric.criteria.get(i).ok_or(ContractError::InvalidRubric)?;
            let score = scores.get(i).ok_or(ContractError::InvalidScores)?;
            if score > criterion.max_score {
                return Err(ContractError::InvalidScores);
            }

            let normalized: u64 =
                (score as u64) * BPS_DENOMINATOR / (criterion.max_score as u64);
            let weighted = normalized
                .checked_mul(criterion.weight_bps as u64)
                .ok_or(ContractError::ArithmeticOverflow)?;
            acc = acc
                .checked_add(weighted)
                .ok_or(ContractError::ArithmeticOverflow)?;
            i += 1;
        }

        Ok((acc / BPS_DENOMINATOR) as u32)
    }

    pub fn get_review(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
        reviewer: Address,
    ) -> Result<Review, ContractError> {
        let key = DataKey::Review(program_id, applicant, reviewer);
        let review = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ReviewNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(review)
    }

    pub fn has_review(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
        reviewer: Address,
    ) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Review(program_id, applicant, reviewer))
    }

    /// Number of reviews recorded so far for an application.
    pub fn get_reviewer_count(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> u32 {
        let reviewers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Reviewers(program_id, applicant))
            .unwrap_or(Vec::new(&env));
        reviewers.len()
    }

    // ── #1086 — normalized aggregate score ────────────────────────────────

    /// Compute the normalized aggregate score (basis points, `0..=10_000`)
    /// for an application from all recorded reviews. Fails with
    /// `InsufficientReviews` when fewer than the program's `min_reviews`
    /// have been recorded — missing reviews are never invented, so an
    /// under-reviewed application simply has no aggregate.
    pub fn aggregate_score(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<u32, ContractError> {
        let config: ProgramConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;

        let reviewers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Reviewers(program_id.clone(), applicant.clone()))
            .unwrap_or(Vec::new(&env));
        let count = reviewers.len();
        if count < config.min_reviews {
            return Err(ContractError::InsufficientReviews);
        }

        // Every review records the rubric version it was scored against.
        // Aggregation uses that exact version, and requires all reviews to
        // agree on it, so the result stays reproducible even if the program
        // later publishes a new rubric.
        let first_reviewer = reviewers.get(0).ok_or(ContractError::ReviewNotFound)?;
        let first_review: Review = env
            .storage()
            .persistent()
            .get(&DataKey::Review(
                program_id.clone(),
                applicant.clone(),
                first_reviewer,
            ))
            .ok_or(ContractError::ReviewNotFound)?;
        let rubric_version = first_review.rubric_version;
        let rubric: Rubric = env
            .storage()
            .persistent()
            .get(&DataKey::Rubric(program_id.clone(), rubric_version))
            .ok_or(ContractError::RubricNotFound)?;

        let mut sum: u64 = 0;
        let mut i: u32 = 0;
        while i < count {
            let reviewer = reviewers.get(i).ok_or(ContractError::ReviewNotFound)?;
            let review: Review = env
                .storage()
                .persistent()
                .get(&DataKey::Review(
                    program_id.clone(),
                    applicant.clone(),
                    reviewer,
                ))
                .ok_or(ContractError::ReviewNotFound)?;
            if review.rubric_version != rubric_version {
                return Err(ContractError::MixedRubricVersions);
            }
            let total = Self::weighted_total(&rubric, &review.scores)?;
            sum = sum
                .checked_add(total as u64)
                .ok_or(ContractError::ArithmeticOverflow)?;
            i += 1;
        }

        // `count >= min_reviews >= 1`, so this division can never be by zero.
        Ok((sum / (count as u64)) as u32)
    }

    /// #1086 — the explicit tie policy for comparing two applicants.
    ///
    /// 1. Higher aggregate score wins.
    /// 2. On equal aggregates, the applicant whose *earliest* review was
    ///    submitted first ranks ahead (a deterministic, immutable input).
    /// 3. If both aggregate and earliest-review time are equal, the outcome
    ///    is reported as `Tie` rather than inventing a further discriminator.
    ///
    /// Both applicants must individually have enough reviews to produce an
    /// aggregate, otherwise `InsufficientReviews` is returned.
    pub fn compare_applicants(
        env: Env,
        program_id: BytesN<32>,
        applicant_a: Address,
        applicant_b: Address,
    ) -> Result<CompareOutcome, ContractError> {
        let score_a = Self::aggregate_score(env.clone(), program_id.clone(), applicant_a.clone())?;
        let score_b = Self::aggregate_score(env.clone(), program_id.clone(), applicant_b.clone())?;

        if score_a > score_b {
            return Ok(CompareOutcome::ABetter);
        }
        if score_b > score_a {
            return Ok(CompareOutcome::BBetter);
        }

        let time_a = Self::first_review_time(&env, &program_id, &applicant_a)?;
        let time_b = Self::first_review_time(&env, &program_id, &applicant_b)?;
        if time_a < time_b {
            return Ok(CompareOutcome::ABetter);
        }
        if time_b < time_a {
            return Ok(CompareOutcome::BBetter);
        }
        Ok(CompareOutcome::Tie)
    }

    /// Earliest `submitted_at` among an application's reviews — the
    /// deterministic tie-break input.
    fn first_review_time(
        env: &Env,
        program_id: &BytesN<32>,
        applicant: &Address,
    ) -> Result<u64, ContractError> {
        let reviewers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Reviewers(program_id.clone(), applicant.clone()))
            .unwrap_or(Vec::new(env));

        let mut earliest: Option<u64> = None;
        let mut i: u32 = 0;
        while i < reviewers.len() {
            let reviewer = reviewers.get(i).ok_or(ContractError::ReviewNotFound)?;
            let review: Review = env
                .storage()
                .persistent()
                .get(&DataKey::Review(
                    program_id.clone(),
                    applicant.clone(),
                    reviewer,
                ))
                .ok_or(ContractError::ReviewNotFound)?;
            match earliest {
                None => earliest = Some(review.submitted_at),
                Some(current) => {
                    if review.submitted_at < current {
                        earliest = Some(review.submitted_at);
                    }
                }
            }
            i += 1;
        }

        earliest.ok_or(ContractError::InsufficientReviews)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
