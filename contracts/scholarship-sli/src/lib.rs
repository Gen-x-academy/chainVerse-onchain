#![no_std]
#![allow(clippy::too_many_arguments)]

//! Bounded, privacy-minimized scholarship service-level indicators.
//!
//! An authorized recorder attests to operation timestamps and outcomes
//! collected from scholarship domain events and external notification
//! providers. The contract validates the sample and maintains daily
//! per-program aggregates; it does not store payloads or participant/payment
//! identifiers.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const INSTANCE_MIN_TTL: u32 = 3_110_400;
const INSTANCE_MAX_TTL: u32 = 6_220_800;
const MAX_SAMPLES_PER_DAY: u32 = 50_000;
const MAX_GLOBAL_SAMPLES_PER_DAY: u32 = 100_000;
const MAX_MEASUREMENT_SECONDS: u64 = 7_776_000;
const SECONDS_PER_DAY: u64 = 86_400;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotRecorder = 4,
    EmptyReference = 5,
    InvalidTimestamp = 6,
    MeasurementTooLong = 7,
    InvalidAmounts = 8,
    DuplicateSample = 9,
    DailyLimitReached = 10,
    Overflow = 11,
    MetricsNotFound = 12,
}

/// The type of operational sample summarized in a daily aggregate.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliMetric {
    /// General domain-operation latency and error rate.
    Operation = 1,
    /// Age from queue entry to queue processing.
    QueueAge = 2,
    /// Lag from notification enqueue to provider acknowledgment.
    NotificationLag = 3,
    /// Time from payment intent opening to finalization.
    PaymentFinalization = 4,
    /// Payment reconciliation amount drift; duration is also recorded.
    Reconciliation = 5,
}

/// Daily aggregate for one program and metric.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SliAggregate {
    pub program_id: BytesN<32>,
    pub metric: SliMetric,
    /// UTC day number (`ledger_timestamp / 86_400`).
    pub day: u64,
    pub sample_count: u32,
    pub failure_count: u32,
    pub duration_total_seconds: u64,
    pub duration_max_seconds: u64,
    pub reconciliation_count: u32,
    /// Sum of signed `actual_amount - expected_amount`, in token base units.
    pub net_drift_total: i128,
    /// Sum of absolute `actual_amount - expected_amount`, in token base units.
    pub absolute_drift_total: i128,
    pub absolute_drift_max: i128,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Recorder,
    Aggregate(BytesN<32>, SliMetric, u64),
    Sample(BytesN<32>, SliMetric, u64, BytesN<32>),
    DailySampleCount(u64),
}

#[contract]
pub struct ScholarshipSliContract;

#[contractimpl]
impl ScholarshipSliContract {
    /// Initialize the contract with a governance admin and trusted recorder.
    pub fn initialize(
        env: Env,
        admin: Address,
        recorder: Address,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Recorder, &recorder);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Rotate the recorder without changing the admin.
    pub fn set_recorder(
        env: Env,
        admin: Address,
        recorder: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Recorder, &recorder);
        Self::extend_instance_ttl(&env);
        env.events()
            .publish((symbol_short!("SLIREC"),), recorder);
        Ok(())
    }

    /// Submit one trusted operational observation. Times are ledger seconds
    /// copied from authoritative source events; `sample_id` is a unique
    /// high-entropy event/correlation commitment used only for deduplication.
    ///
    /// For `Reconciliation`, expected and actual nonnegative token amounts
    /// produce the absolute drift aggregate. For all other metrics both
    /// amounts must be zero.
    pub fn record_sample(
        env: Env,
        recorder: Address,
        program_id: BytesN<32>,
        metric: SliMetric,
        sample_id: BytesN<32>,
        started_at: u64,
        observed_at: u64,
        succeeded: bool,
        expected_amount: i128,
        actual_amount: i128,
    ) -> Result<SliAggregate, ContractError> {
        Self::require_recorder(&env, &recorder)?;
        if program_id == Self::zero_hash(&env) || sample_id == Self::zero_hash(&env) {
            return Err(ContractError::EmptyReference);
        }

        let now = env.ledger().timestamp();
        if started_at > observed_at || observed_at > now {
            return Err(ContractError::InvalidTimestamp);
        }
        let duration = observed_at
            .checked_sub(started_at)
            .ok_or(ContractError::InvalidTimestamp)?;
        if duration > MAX_MEASUREMENT_SECONDS {
            return Err(ContractError::MeasurementTooLong);
        }
        let (net_drift, absolute_drift) = match metric {
            SliMetric::Reconciliation => {
                if expected_amount < 0 || actual_amount < 0 {
                    return Err(ContractError::InvalidAmounts);
                }
                let net = actual_amount
                    .checked_sub(expected_amount)
                    .ok_or(ContractError::Overflow)?;
                let absolute = net.checked_abs().ok_or(ContractError::Overflow)?;
                (net, absolute)
            }
            _ => {
                if expected_amount != 0 || actual_amount != 0 {
                    return Err(ContractError::InvalidAmounts);
                }
                (0, 0)
            }
        };

        let day = observed_at / SECONDS_PER_DAY;
        let sample_key = DataKey::Sample(
            program_id.clone(),
            metric,
            day,
            sample_id,
        );
        if env.storage().persistent().has(&sample_key) {
            return Err(ContractError::DuplicateSample);
        }
        let aggregate_key = DataKey::Aggregate(program_id.clone(), metric, day);
        let mut aggregate: SliAggregate = env
            .storage()
            .persistent()
            .get(&aggregate_key)
            .unwrap_or(SliAggregate {
                program_id: program_id.clone(),
                metric,
                day,
                sample_count: 0,
                failure_count: 0,
                duration_total_seconds: 0,
                duration_max_seconds: 0,
                reconciliation_count: 0,
                net_drift_total: 0,
                absolute_drift_total: 0,
                absolute_drift_max: 0,
                updated_at: 0,
            });
        if aggregate.sample_count >= MAX_SAMPLES_PER_DAY {
            return Err(ContractError::DailyLimitReached);
        }
        let daily_count_key = DataKey::DailySampleCount(day);
        let daily_count: u32 = env
            .storage()
            .persistent()
            .get(&daily_count_key)
            .unwrap_or(0);
        if daily_count >= MAX_GLOBAL_SAMPLES_PER_DAY {
            return Err(ContractError::DailyLimitReached);
        }
        let next_daily_count = daily_count
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        aggregate.sample_count = aggregate
            .sample_count
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        if !succeeded {
            aggregate.failure_count = aggregate
                .failure_count
                .checked_add(1)
                .ok_or(ContractError::Overflow)?;
        }
        aggregate.duration_total_seconds = aggregate
            .duration_total_seconds
            .checked_add(duration)
            .ok_or(ContractError::Overflow)?;
        aggregate.duration_max_seconds = aggregate.duration_max_seconds.max(duration);
        if metric == SliMetric::Reconciliation {
            aggregate.reconciliation_count = aggregate
                .reconciliation_count
                .checked_add(1)
                .ok_or(ContractError::Overflow)?;
            aggregate.net_drift_total = aggregate
                .net_drift_total
                .checked_add(net_drift)
                .ok_or(ContractError::Overflow)?;
            aggregate.absolute_drift_total = aggregate
                .absolute_drift_total
                .checked_add(absolute_drift)
                .ok_or(ContractError::Overflow)?;
            aggregate.absolute_drift_max = aggregate.absolute_drift_max.max(absolute_drift);
        }
        aggregate.updated_at = now;

        env.storage().persistent().set(&sample_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&sample_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage()
            .persistent()
            .set(&aggregate_key, &aggregate);
        env.storage()
            .persistent()
            .extend_ttl(&aggregate_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage()
            .persistent()
            .set(&daily_count_key, &next_daily_count);
        env.storage()
            .persistent()
            .extend_ttl(&daily_count_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::extend_instance_ttl(&env);
        env.events().publish(
            (symbol_short!("SLIUPD"), program_id, metric, day),
            aggregate.clone(),
        );
        Ok(aggregate)
    }

    /// Public aggregate query. No individual sample identifiers are returned.
    pub fn get_aggregate(
        env: Env,
        program_id: BytesN<32>,
        metric: SliMetric,
        day: u64,
    ) -> Result<SliAggregate, ContractError> {
        let key = DataKey::Aggregate(program_id, metric, day);
        let aggregate = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::MetricsNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::extend_instance_ttl(&env);
        Ok(aggregate)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if admin != *caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_recorder(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let recorder: Address = env
            .storage()
            .instance()
            .get(&DataKey::Recorder)
            .ok_or(ContractError::NotInitialized)?;
        if recorder != *caller {
            return Err(ContractError::NotRecorder);
        }
        caller.require_auth();
        Self::extend_instance_ttl(env);
        Ok(())
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_MIN_TTL, INSTANCE_MAX_TTL);
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
