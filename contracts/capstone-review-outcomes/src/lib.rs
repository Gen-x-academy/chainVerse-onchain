#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_CONTRIBUTIONS: u32 = 64;
const MAX_SCORE: u32 = 100;
const MAX_WEIGHT: u32 = 100;
const PASS_THRESHOLD: u64 = 50;
const ANCHOR_SCORE: u64 = 50;
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
    InvalidWeight = 12,
    InvalidHash = 13,
    ArithmeticOverflow = 14,
    AlreadyFinalized = 15,
    DuplicateContribution = 16,
    QuorumNotMet = 17,
    InvalidQuorum = 18,
    MissingContributions = 19,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Outcome(BytesN<32>, BytesN<32>, BytesN<32>),
    Contributions(BytesN<32>, BytesN<32>, BytesN<32>),
    Contribution(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Coordinator = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingPolicy {
    Exclude = 1,
    CountAsZero = 2,
    Block = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationPolicy {
    Disabled = 1,
    LinearRescale = 2,
    PriorAnchor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TieTreatment {
    Reject = 1,
    Escalate = 2,
    Accept = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeStatus {
    Open = 1,
    Finalized = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeResult {
    Accepted = 1,
    Rejected = 2,
    Escalated = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub outcome_id: BytesN<32>,
    pub proposal_id: BytesN<32>,
    pub submission_version: u32,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub version: u32,
    pub status: OutcomeStatus,
    pub quorum: u32,
    pub received: u32,
    pub weight_total: u64,
    pub weighted_sum: u64,
    pub score_min: u32,
    pub score_max: u32,
    pub final_score: u32,
    pub missing_policy: MissingPolicy,
    pub calibration: CalibrationPolicy,
    pub tie_treatment: TieTreatment,
    pub result: OutcomeResult,
    pub explanation_hash: BytesN<32>,
    pub policy_hash: BytesN<32>,
    pub created_at: u64,
    pub finalized_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contribution {
    pub review_id: BytesN<32>,
    pub score: u32,
    pub weight: u32,
    pub recorded_at: u64,
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
pub struct CapstoneReviewOutcomesContract;

#[contractimpl]
impl CapstoneReviewOutcomesContract {
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
            .publish((symbol_short!("OCGRANT"),), (tenant_id, project_id, role));
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
            .publish((symbol_short!("OCRVOK"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn create_outcome(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        proposal_id: BytesN<32>,
        submission_version: u32,
        rubric_id: BytesN<32>,
        rubric_version: u32,
        quorum: u32,
        missing_policy: MissingPolicy,
        calibration: CalibrationPolicy,
        tie_treatment: TieTreatment,
        policy_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if expected_version != 0 {
            return Err(ContractError::StaleVersion);
        }
        if quorum == 0 || quorum > MAX_CONTRIBUTIONS {
            return Err(ContractError::InvalidQuorum);
        }
        if submission_version == 0 || rubric_version == 0 {
            return Err(ContractError::StaleVersion);
        }
        if policy_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Outcome(tenant_id.clone(), project_id.clone(), outcome_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let now = env.ledger().timestamp();
        let outcome = Outcome {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            outcome_id: outcome_id.clone(),
            proposal_id,
            submission_version,
            rubric_id,
            rubric_version,
            version: 1,
            status: OutcomeStatus::Open,
            quorum,
            received: 0,
            weight_total: 0,
            weighted_sum: 0,
            score_min: MAX_SCORE,
            score_max: 0,
            final_score: 0,
            missing_policy,
            calibration,
            tie_treatment,
            result: OutcomeResult::Rejected,
            explanation_hash: Self::zero_id(&env),
            policy_hash,
            created_at: now,
            finalized_at: 0,
        };
        Self::store_outcome(&env, &key, &outcome);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
            1,
            now,
        );
        env.events().publish(
            (symbol_short!("OCNEW"),),
            (tenant_id, project_id, outcome_id, 1u32),
        );
        Ok(1)
    }

    pub fn record_score(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        review_id: BytesN<32>,
        score: u32,
        weight: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if score > MAX_SCORE {
            return Err(ContractError::InvalidScore);
        }
        if weight == 0 || weight > MAX_WEIGHT {
            return Err(ContractError::InvalidWeight);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Outcome(tenant_id.clone(), project_id.clone(), outcome_id.clone());
        let mut outcome: Outcome = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if outcome.status != OutcomeStatus::Open {
            return Err(ContractError::AlreadyFinalized);
        }
        if outcome.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let ids_key =
            DataKey::Contributions(tenant_id.clone(), project_id.clone(), outcome_id.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));
        if ids.len() >= MAX_CONTRIBUTIONS {
            return Err(ContractError::CollectionFull);
        }
        if Self::contains_id(&ids, &review_id) {
            return Err(ContractError::DuplicateContribution);
        }
        let contribution_value = (score as u64)
            .checked_mul(weight as u64)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let weighted_sum = outcome
            .weighted_sum
            .checked_add(contribution_value)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let weight_total = outcome
            .weight_total
            .checked_add(weight as u64)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let received = outcome
            .received
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if received != ids.len() + 1 {
            return Err(ContractError::ArithmeticOverflow);
        }
        let version = outcome
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        outcome.version = version;
        outcome.weighted_sum = weighted_sum;
        outcome.weight_total = weight_total;
        outcome.received = received;
        if score < outcome.score_min {
            outcome.score_min = score;
        }
        if score > outcome.score_max {
            outcome.score_max = score;
        }
        Self::store_outcome(&env, &key, &outcome);
        ids.push_back(review_id.clone());
        env.storage().persistent().set(&ids_key, &ids);
        env.storage()
            .persistent()
            .extend_ttl(&ids_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let contribution_key = DataKey::Contribution(
            tenant_id.clone(),
            project_id.clone(),
            outcome_id.clone(),
            review_id.clone(),
        );
        let contribution = Contribution {
            review_id: review_id.clone(),
            score,
            weight,
            recorded_at: now,
        };
        env.storage()
            .persistent()
            .set(&contribution_key, &contribution);
        env.storage()
            .persistent()
            .extend_ttl(&contribution_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("OCSCOR"),),
            (tenant_id, project_id, outcome_id, review_id, version),
        );
        Ok(version)
    }

    pub fn finalize(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        explanation_hash: BytesN<32>,
    ) -> Result<OutcomeResult, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if explanation_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if let Some(_version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
        )? {
            let outcome: Outcome = env
                .storage()
                .persistent()
                .get(&DataKey::Outcome(
                    tenant_id,
                    project_id,
                    outcome_id,
                ))
                .ok_or(ContractError::NotFound)?;
            return Ok(outcome.result);
        }
        let key = DataKey::Outcome(tenant_id.clone(), project_id.clone(), outcome_id.clone());
        let mut outcome: Outcome = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if outcome.status != OutcomeStatus::Open {
            return Err(ContractError::AlreadyFinalized);
        }
        if outcome.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if outcome.received < outcome.quorum && outcome.missing_policy == MissingPolicy::Block {
            return Err(ContractError::QuorumNotMet);
        }
        let mut denominator = outcome.weight_total;
        if outcome.missing_policy == MissingPolicy::CountAsZero {
            let quorum_weight = (outcome.quorum as u64)
                .checked_mul(MAX_WEIGHT as u64)
                .ok_or(ContractError::ArithmeticOverflow)?;
            denominator = if quorum_weight > denominator {
                quorum_weight
            } else {
                denominator
            };
        }
        if denominator == 0 {
            return Err(ContractError::MissingContributions);
        }
        let weighted = outcome
            .weighted_sum
            .checked_mul(MAX_SCORE as u64)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let mut final_score = weighted
            .checked_div(denominator)
            .ok_or(ContractError::ArithmeticOverflow)?;
        final_score = Self::apply_calibration(&outcome, final_score)?;
        let result = if final_score > PASS_THRESHOLD {
            OutcomeResult::Accepted
        } else if final_score < PASS_THRESHOLD {
            OutcomeResult::Rejected
        } else {
            match outcome.tie_treatment {
                TieTreatment::Reject => OutcomeResult::Rejected,
                TieTreatment::Escalate => OutcomeResult::Escalated,
                TieTreatment::Accept => OutcomeResult::Accepted,
            }
        };
        let version = outcome
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        outcome.version = version;
        outcome.status = OutcomeStatus::Finalized;
        outcome.final_score = final_score as u32;
        outcome.result = result;
        outcome.explanation_hash = explanation_hash;
        outcome.finalized_at = now;
        Self::store_outcome(&env, &key, &outcome);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &outcome_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("OCFIN"),),
            (tenant_id, project_id, outcome_id, result, version),
        );
        Ok(result)
    }

    pub fn get_outcome(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
    ) -> Option<Outcome> {
        env.storage()
            .persistent()
            .get(&DataKey::Outcome(tenant_id, project_id, outcome_id))
    }

    pub fn get_contribution_ids(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
    ) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::Contributions(tenant_id, project_id, outcome_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_contribution(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        outcome_id: BytesN<32>,
        review_id: BytesN<32>,
    ) -> Option<Contribution> {
        env.storage().persistent().get(&DataKey::Contribution(
            tenant_id,
            project_id,
            outcome_id,
            review_id,
        ))
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

    fn apply_calibration(
        outcome: &Outcome,
        final_score: u64,
    ) -> Result<u64, ContractError> {
        match outcome.calibration {
            CalibrationPolicy::Disabled => Ok(final_score),
            CalibrationPolicy::PriorAnchor => final_score
                .checked_add(ANCHOR_SCORE)
                .and_then(|value| value.checked_div(2))
                .ok_or(ContractError::ArithmeticOverflow),
            CalibrationPolicy::LinearRescale => {
                let span = (outcome.score_max as u64)
                    .checked_sub(outcome.score_min as u64)
                    .ok_or(ContractError::ArithmeticOverflow)?;
                if span == 0 {
                    return Ok(final_score);
                }
                let offset = if final_score >= outcome.score_min as u64 {
                    final_score - outcome.score_min as u64
                } else {
                    return Err(ContractError::ArithmeticOverflow);
                };
                offset
                    .checked_mul(MAX_SCORE as u64)
                    .and_then(|value| value.checked_div(span))
                    .ok_or(ContractError::ArithmeticOverflow)
            }
        }
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

    fn store_outcome(env: &Env, key: &DataKey, outcome: &Outcome) {
        env.storage().persistent().set(key, outcome);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn contains_id(ids: &Vec<BytesN<32>>, id: &BytesN<32>) -> bool {
        let mut i = 0;
        while i < ids.len() {
            if ids.get(i).as_ref() == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }
}
