#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

/// Minimum cohort size required before aggregate counts are exposed without suppression.
pub const SUPPRESSION_THRESHOLD: u32 = 5;

/// TTL thresholds and bump amounts for persistent storage bounds.
const PERSISTENT_LIFETIME_THRESHOLD: u32 = 518_400;
const PERSISTENT_BUMP_AMOUNT: u32 = 2_592_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AnalyticsError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidTenant = 4,
    DuplicateAttestation = 5,
    InvalidStateTransition = 6,
    ExpiredWindow = 7,
    InvalidSuppressionThreshold = 8,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MetricStage {
    ProposalSubmitted = 1,
    ProposalApproved = 2,
    TeamFormed = 3,
    ProgressMilestone = 4,
    Completed = 5,
    Published = 6,
    Funded = 7,
    OutcomeRecorded = 8,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramAnalyticsAggregate {
    pub proposals: u32,
    pub approvals: u32,
    pub teams: u32,
    pub progress_checkpoints: u32,
    pub completions: u32,
    pub publications: u32,
    pub funded_projects: u32,
    pub outcomes: u32,
    pub total_funding_allocated: i128,
    pub suppressed: bool,
    pub last_updated_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestationCommitment {
    pub stage: MetricStage,
    pub commitment_hash: BytesN<32>,
    pub funding_amount: i128,
    pub recorded_at_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TenantReporter(BytesN<32>, Address),
    SuppressionFloor(BytesN<32>),
    Aggregate(BytesN<32>),
    Commitment(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneProgramAnalyticsContract;

#[contractimpl]
impl CapstoneProgramAnalyticsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), AnalyticsError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(AnalyticsError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn authorize_tenant_reporter(
        env: Env,
        tenant_id: BytesN<32>,
        reporter: Address,
        suppression_floor: u32,
    ) -> Result<(), AnalyticsError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(AnalyticsError::NotInitialized)?;
        admin.require_auth();

        if suppression_floor < SUPPRESSION_THRESHOLD {
            return Err(AnalyticsError::InvalidSuppressionThreshold);
        }

        let reporter_key = DataKey::TenantReporter(tenant_id.clone(), reporter.clone());
        env.storage().persistent().set(&reporter_key, &true);
        env.storage().persistent().extend_ttl(
            &reporter_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        let floor_key = DataKey::SuppressionFloor(tenant_id.clone());
        env.storage()
            .persistent()
            .set(&floor_key, &suppression_floor);
        env.storage().persistent().extend_ttl(
            &floor_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.events().publish(
            (symbol_short!("rep_auth"), tenant_id),
            (reporter, suppression_floor),
        );
        Ok(())
    }

    /// Records a privacy-minimized stage attestation using bounded hashes and amounts.
    /// Rejects duplicate/replayed attestation IDs and expired windows.
    pub fn record_metric_attestation(
        env: Env,
        tenant_id: BytesN<32>,
        reporter: Address,
        attestation_id: BytesN<32>,
        commitment_hash: BytesN<32>,
        stage: MetricStage,
        funding_amount: i128,
        expires_at_ledger: u32,
    ) -> Result<(), AnalyticsError> {
        reporter.require_auth();

        let reporter_key = DataKey::TenantReporter(tenant_id.clone(), reporter.clone());
        let is_authorized: bool = env
            .storage()
            .persistent()
            .get(&reporter_key)
            .unwrap_or(false);
        if !is_authorized {
            return Err(AnalyticsError::Unauthorized);
        }

        let current_ledger = env.ledger().sequence();
        if expires_at_ledger <= current_ledger {
            return Err(AnalyticsError::ExpiredWindow);
        }

        if funding_amount < 0 || (stage != MetricStage::Funded && funding_amount != 0) {
            return Err(AnalyticsError::InvalidStateTransition);
        }

        // Replay / idempotency guard: prevent duplicate attestation commits
        let commit_key = DataKey::Commitment(tenant_id.clone(), attestation_id.clone());
        if env.storage().persistent().has(&commit_key) {
            return Err(AnalyticsError::DuplicateAttestation);
        }

        let commitment = AttestationCommitment {
            stage,
            commitment_hash: commitment_hash.clone(),
            funding_amount,
            recorded_at_ledger: current_ledger,
            expires_at_ledger,
        };
        env.storage().persistent().set(&commit_key, &commitment);
        env.storage().persistent().extend_ttl(
            &commit_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        let agg_key = DataKey::Aggregate(tenant_id.clone());
        let mut agg: ProgramAnalyticsAggregate = env
            .storage()
            .persistent()
            .get(&agg_key)
            .unwrap_or(ProgramAnalyticsAggregate {
                proposals: 0,
                approvals: 0,
                teams: 0,
                progress_checkpoints: 0,
                completions: 0,
                publications: 0,
                funded_projects: 0,
                outcomes: 0,
                total_funding_allocated: 0,
                suppressed: true,
                last_updated_ledger: current_ledger,
            });

        match stage {
            MetricStage::ProposalSubmitted => agg.proposals = agg.proposals.saturating_add(1),
            MetricStage::ProposalApproved => agg.approvals = agg.approvals.saturating_add(1),
            MetricStage::TeamFormed => agg.teams = agg.teams.saturating_add(1),
            MetricStage::ProgressMilestone => {
                agg.progress_checkpoints = agg.progress_checkpoints.saturating_add(1)
            }
            MetricStage::Completed => agg.completions = agg.completions.saturating_add(1),
            MetricStage::Published => agg.publications = agg.publications.saturating_add(1),
            MetricStage::Funded => {
                agg.funded_projects = agg.funded_projects.saturating_add(1);
                agg.total_funding_allocated =
                    agg.total_funding_allocated.saturating_add(funding_amount);
            }
            MetricStage::OutcomeRecorded => agg.outcomes = agg.outcomes.saturating_add(1),
        }
        agg.last_updated_ledger = current_ledger;

        env.storage().persistent().set(&agg_key, &agg);
        env.storage().persistent().extend_ttl(
            &agg_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        // Privacy-safe event schema: emits only bounded hashes and stage identifiers
        env.events().publish(
            (symbol_short!("cap_met"), tenant_id, stage),
            (attestation_id, commitment_hash, current_ledger),
        );

        Ok(())
    }

    /// Returns tenant capstone metrics with small-cohort suppression applied.
    pub fn get_program_analytics(
        env: Env,
        tenant_id: BytesN<32>,
    ) -> Result<ProgramAnalyticsAggregate, AnalyticsError> {
        let floor: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SuppressionFloor(tenant_id.clone()))
            .ok_or(AnalyticsError::InvalidTenant)?;

        let agg: ProgramAnalyticsAggregate = env
            .storage()
            .persistent()
            .get(&DataKey::Aggregate(tenant_id))
            .unwrap_or(ProgramAnalyticsAggregate {
                proposals: 0,
                approvals: 0,
                teams: 0,
                progress_checkpoints: 0,
                completions: 0,
                publications: 0,
                funded_projects: 0,
                outcomes: 0,
                total_funding_allocated: 0,
                suppressed: true,
                last_updated_ledger: env.ledger().sequence(),
            });

        // Suppress granular breakdowns when cohort size is below the privacy floor
        if agg.proposals < floor {
            return Ok(ProgramAnalyticsAggregate {
                proposals: 0,
                approvals: 0,
                teams: 0,
                progress_checkpoints: 0,
                completions: 0,
                publications: 0,
                funded_projects: 0,
                outcomes: 0,
                total_funding_allocated: 0,
                suppressed: true,
                last_updated_ledger: agg.last_updated_ledger,
            });
        }

        Ok(ProgramAnalyticsAggregate {
            suppressed: false,
            ..agg
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

    #[test]
    fn test_authorization_replay_protection_and_suppression() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(CapstoneProgramAnalyticsContract, ());
        let client = CapstoneProgramAnalyticsContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let reporter = Address::generate(&env);
        let tenant = BytesN::from_array(&env, &[1u8; 32]);

        client.initialize(&admin);
        client.authorize_tenant_reporter(&tenant, &reporter, &5);

        env.ledger().with_mut(|li| {
            li.sequence_number = 100;
        });

        for i in 0..4u8 {
            let att_id = BytesN::from_array(&env, &[i + 10; 32]);
            let commit_hash = BytesN::from_array(&env, &[i + 50; 32]);
            client.record_metric_attestation(
                &tenant,
                &reporter,
                &att_id,
                &commit_hash,
                &MetricStage::ProposalSubmitted,
                &0,
                &500,
            );
        }

        // Below suppression floor (4 < 5): metrics remain suppressed
        let suppressed = client.get_program_analytics(&tenant);
        assert!(suppressed.suppressed);
        assert_eq!(suppressed.proposals, 0);

        // Replay of an existing attestation_id fails deterministically
        let dup_id = BytesN::from_array(&env, &[10u8; 32]);
        let commit_hash = BytesN::from_array(&env, &[99u8; 32]);
        let replay = client.try_record_metric_attestation(
            &tenant,
            &reporter,
            &dup_id,
            &commit_hash,
            &MetricStage::ProposalSubmitted,
            &0,
            &500,
        );
        assert_eq!(replay, Err(Ok(AnalyticsError::DuplicateAttestation)));

        // Fifth proposal meets suppression floor (5 >= 5): metrics are revealed
        let fifth_id = BytesN::from_array(&env, &[20u8; 32]);
        client.record_metric_attestation(
            &tenant,
            &reporter,
            &fifth_id,
            &commit_hash,
            &MetricStage::ProposalSubmitted,
            &0,
            &500,
        );

        let unsuppressed = client.get_program_analytics(&tenant);
        assert!(!unsuppressed.suppressed);
        assert_eq!(unsuppressed.proposals, 5);
    }
}