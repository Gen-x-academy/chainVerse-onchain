#![no_std]
// `map_record` takes a legacy record's fields as explicit parameters -- ten
// including `env`. Grouping them into a struct would change the contract's
// public ABI to work around a lint, which is not a trade this contract
// should make. Same reasoning as `scholarship-core` and `scholarship-outbox`.
#![allow(clippy::too_many_arguments)]

//! Legacy financial-aid record migration (issue #1143).
//!
//! Financial-aid applications exist in legacy systems that predate the
//! scholarship contracts, and they have to be brought into the scholarship
//! domain **without losing history**. "Without losing history" is the whole
//! constraint: a migrated record has to remain traceable to the record it
//! came from, with its original identifier and its original timestamp
//! intact, or the migration has quietly destroyed the evidence that an award
//! was applied for, when, and on what basis.
//!
//! ## What this contract does, and does not, do
//!
//! It **records** the mapping. It does not create scholarship applications,
//! and it cannot: ADR 0002 forbids the scholarship contracts from calling one
//! another, so a migration contract cannot reach into `scholarship-applications`
//! any more than the outbox can. The orchestrator applies the actual domain
//! write and the mapping record in **one Stellar transaction**, the same
//! atomicity pattern the outbox uses. The chain proves the mapping was
//! recorded alongside a committed state change; it does not perform the change.
//!
//! ## Why a run, and why it is resumable
//!
//! Legacy datasets do not fit in one transaction, so a migration is
//! necessarily many transactions, and any such sequence has to survive being
//! interrupted. A run carries a **checkpoint** that advances as records are
//! mapped, so a re-run resumes from the last acknowledged legacy cursor
//! instead of rescanning from zero — and, because mapping is idempotent,
//! resuming is indistinguishable from continuing.
//!
//! ## Dry run is a first-class mode, not a flag someone forgets
//!
//! `begin_run` takes the mode, and a dry run **cannot write a mapping** no
//! matter what it is later asked to do. That is the useful property: the
//! report you get from a dry run is produced by exactly the code that would
//! do the migration, so it cannot drift from what a real run would do.
//!
//! ## Privacy
//!
//! Legacy application records contain exactly the data this platform exists
//! not to put on chain: names, addresses, answers, documents. This contract
//! stores a `payload_hash` and nothing else about the record's contents. The
//! eligibility verdict is a **code** (`Eligible` / one of the
//! `UnmappableReason` variants), never a score, a ranking, or a
//! probability — a stored "likelihood" would be a derived fact about a person
//! that leaks far more than the answer it was meant to help with.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol,
};

const CONTRACT_VERSION: u32 = 1;

/// Migration records are permanent history, so they outlive webhook
/// deliveries and event envelopes: kept at least ~6 months and extended
/// toward ~14. A migration that expires is a migration that lost history.
const RECORD_MIN_TTL: u32 = 15_552_000;
const RECORD_MAX_TTL: u32 = 31_104_000;

/// Ceiling on legacy records tracked at once. Bounded so a runaway import
/// cannot grow storage without limit; a finished record is archivable.
const MAX_TRACKED_RECORDS: u64 = 50_000;

/// Ceiling on unmapped items retained per run. The *count* is unbounded and
/// exact; only the browsable detail is capped, so a pathological dataset
/// cannot fill the ledger with reasons.
const MAX_REPORTED_PER_RUN: u32 = 1_000;

/// Ceiling on concurrent runs.
const MAX_RUNS: u64 = 50;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The run id does not exist.
    RunNotFound = 4,
    /// The run has been sealed and accepts no further records.
    RunSealed = 5,
    /// No legacy record with that id has been seen.
    RecordNotFound = 6,
    /// The legacy record id is already tracked, under a different payload.
    /// A legacy id is a primary key; reusing it for different content would
    /// overwrite history.
    RecordConflict = 7,
    /// A zero hash where a commitment is required.
    EmptyCommitment = 8,
    /// The record id table is at its ceiling.
    TooManyRecords = 9,
    /// The run id table is at its ceiling.
    TooManyRuns = 10,
    /// A dry run attempted to write a mapping, which is the one thing a dry
    /// run must never do.
    DryRunIsReadOnly = 11,
    /// The reported-item window for this run is full.
    TooManyReports = 12,
    /// The legacy cursor would move backwards.
    CursorRegression = 13,
    /// A counter would overflow.
    CounterOverflow = 14,
    /// The record is tracked but has not been mapped.
    NotMapped = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NextRunId,
    OpenRuns,
    Run(u64),
    /// Legacy record id → its state. The primary key that makes mapping
    /// idempotent.
    Record(BytesN<32>),
    /// How many legacy records are tracked, for the bound.
    TrackedRecords,
    /// `(run, index)` → one unmapped item's id, so the report is browsable
    /// without an unbounded scan.
    Report(u64, u32),
    /// How many unmapped items a run has reported, exact and uncapped.
    UnmappedTotal(u64),
    /// How many a run has *retained* for browsing, which is capped.
    UnmappedRetained(u64),
}

/// Why a legacy record could not be mapped (issue #1143: "reports unmapped
/// data").
///
/// Codes, never free text. A reason string on chain would be a place for
/// applicant information to end up, and would cost more per record than the
/// mapping it is explaining.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnmappableReason {
    /// Applicant does not meet the program's published criteria.
    NotEligible = 1,
    /// No scholarship program corresponds to the legacy award type.
    NoMatchingProgram = 2,
    /// The legacy system already contains a duplicate of this application.
    DuplicateInLegacy = 3,
    /// The record is structurally unusable — missing required fields.
    MalformedRecord = 4,
    /// Withdrawn by the applicant before the migration ran.
    WithdrawnByApplicant = 5,
    /// The legacy record is too old to be admitted under current policy.
    OutsideRetentionWindow = 6,
}

/// Where a legacy record ended up.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordState {
    /// Seen and eligible, not yet written to the scholarship domain.
    Eligible = 1,
    /// Mapped: a domain record was committed in the same transaction.
    Mapped = 2,
    /// Reported as unmappable, with a reason code.
    Unmappable = 3,
    /// Archived after mapping. The mapping history remains in the event
    /// log; only the browsable record is dropped.
    Archived = 4,
}

/// One legacy record, mapped or reported.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyRecord {
    /// The legacy system's own identifier, **preserved verbatim**. Never
    /// rewritten, never re-derived. This is the link back to the record of
    /// authority.
    pub legacy_id: BytesN<32>,
    /// Which legacy system the record came from.
    pub source_system: Symbol,
    /// The legacy system's own submission timestamp, **preserved verbatim**
    /// rather than replaced with the migration time. A record that arrived
    /// before the scholarship contracts existed must keep saying so.
    pub submitted_at: u64,
    /// `sha256` of the legacy record's contents. The contents themselves are
    /// never stored — see the module docs.
    pub payload_hash: BytesN<32>,
    /// The scholarship program this record maps into.
    pub program: BytesN<32>,
    pub state: RecordState,
    /// Zero when `state` is `Mapped` or `Eligible`.
    pub reason: u32,
    /// Ties the mapping to the outbox event that announced the domain
    /// change, so the two halves of the transaction can be correlated.
    pub correlation: BytesN<32>,
    /// Ledger time the mapping was recorded, or 0. Distinct from
    /// `submitted_at`, and never a substitute for it.
    pub mapped_at: u64,
}

/// A migration run.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationRun {
    pub id: u64,
    pub source_system: Symbol,
    /// Whether this run may write mappings. A dry run reports and stops.
    pub dry_run: bool,
    /// The legacy cursor this run has reached. Resuming continues from
    /// here rather than rescanning.
    pub cursor: u64,
    /// How many records this run has mapped.
    pub mapped: u32,
    pub sealed: bool,
    pub started_at: u64,
    pub sealed_at: u64,
}

/// The numbers an operator needs to decide whether a migration is done.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSummary {
    pub id: u64,
    pub source_system: Symbol,
    pub dry_run: bool,
    pub cursor: u64,
    pub mapped: u32,
    /// Exact, uncapped.
    pub unmapped: u64,
    /// Bounded: how many unmapped items are still browsable by index.
    pub unmapped_retained: u32,
    pub sealed: bool,
}

#[contract]
pub struct ScholarshipMigrationContract;

#[contractimpl]
impl ScholarshipMigrationContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextRunId, &1u64);
        env.storage().instance().set(&DataKey::OpenRuns, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::TrackedRecords, &0u64);
        Ok(())
    }

    /// Start a run against one legacy system.
    ///
    /// `dry_run` is fixed here and cannot be changed later, so a run cannot
    /// start as a rehearsal and be promoted to a real migration halfway
    /// through — which would leave the first half of the dataset mapped and
    /// the operator believing the whole thing was rehearsed.
    pub fn begin_run(
        env: Env,
        operator: Address,
        source_system: Symbol,
        dry_run: bool,
    ) -> Result<u64, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let open: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OpenRuns)
            .unwrap_or(0u64);
        if open >= MAX_RUNS {
            return Err(ContractError::TooManyRuns);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextRunId)
            .ok_or(ContractError::NotInitialized)?;
        let run = MigrationRun {
            id,
            source_system: source_system.clone(),
            dry_run,
            // The cursor is the legacy system's own ordering, so a resume
            // asks the source for "everything after N" rather than
            // re-reading what it already gave us.
            cursor: 0,
            mapped: 0,
            sealed: false,
            started_at: env.ledger().timestamp(),
            sealed_at: 0,
        };
        Self::save_run(&env, &run);
        env.storage().instance().set(
            &DataKey::NextRunId,
            &id.checked_add(1).ok_or(ContractError::CounterOverflow)?,
        );
        env.storage().instance().set(
            &DataKey::OpenRuns,
            &open.checked_add(1).ok_or(ContractError::CounterOverflow)?,
        );

        env.events().publish(
            (soroban_sdk::symbol_short!("rbeg"), id),
            (source_system, dry_run),
        );
        Ok(id)
    }

    /// Record a legacy record as eligible and, in a real run, map it.
    ///
    /// Idempotent in the strong sense: mapping the same `legacy_id` with the
    /// same contents returns the recorded state and writes nothing. That is
    /// what makes an interrupted migration safe to restart, and it is why a
    /// dry run and a real run can share this code path — the only difference
    /// is whether the mapping is stored.
    ///
    /// The domain write happens in the *same* transaction as this call, by
    /// the orchestrator, per ADR 0002. This contract records that both
    /// happened; it cannot cause the domain write.
    pub fn map_record(
        env: Env,
        operator: Address,
        run_id: u64,
        legacy_id: BytesN<32>,
        source_system: Symbol,
        submitted_at: u64,
        payload_hash: BytesN<32>,
        program: BytesN<32>,
        correlation: BytesN<32>,
        legacy_cursor: u64,
    ) -> Result<RecordState, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;
        Self::require_commitment(&env, &payload_hash)?;

        let run = Self::load_run(&env, run_id)?;
        if run.sealed {
            return Err(ContractError::RunSealed);
        }
        if run.source_system != source_system {
            // A run is against one system. Accepting a record from another
            // would make the cursor meaningless, because the two systems'
            // orderings have nothing to do with each other.
            return Err(ContractError::RecordConflict);
        }

        // Resume point: a record at or before the cursor has already been
        // offered to this run, so re-offering it is a no-op rather than a
        // second pass over the same data.
        if legacy_cursor <= run.cursor {
            return Self::existing_state(&env, &legacy_id);
        }

        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, LegacyRecord>(&DataKey::Record(legacy_id.clone()))
        {
            // A legacy id is a primary key. Same id with different contents
            // is a conflict, not an update — silently accepting it would let
            // a re-export overwrite an applicant's history.
            if existing.payload_hash != payload_hash || existing.submitted_at != submitted_at {
                return Err(ContractError::RecordConflict);
            }
            // Identical: already mapped by an earlier attempt.
            Self::advance(&env, run, legacy_cursor)?;
            return Ok(existing.state);
        }

        let tracked: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TrackedRecords)
            .unwrap_or(0u64);
        if tracked >= MAX_TRACKED_RECORDS {
            return Err(ContractError::TooManyRecords);
        }

        if run.dry_run {
            // The whole point of a rehearsal: advance the cursor so the
            // operator sees how far a real run would get, and write
            // nothing. `mapped` is deliberately not incremented either, so a
            // dry run's summary cannot be mistaken for a real one.
            Self::advance(&env, run, legacy_cursor)?;
            return Ok(RecordState::Eligible);
        }

        let record = LegacyRecord {
            legacy_id: legacy_id.clone(),
            source_system: source_system.clone(),
            submitted_at,
            payload_hash,
            program: program.clone(),
            state: RecordState::Mapped,
            reason: 0,
            correlation,
            mapped_at: env.ledger().timestamp(),
        };
        Self::save_record(&env, &record);
        env.storage().instance().set(
            &DataKey::TrackedRecords,
            &tracked
                .checked_add(1)
                .ok_or(ContractError::CounterOverflow)?,
        );

        let mut run = run;
        run.mapped = run
            .mapped
            .checked_add(1)
            .ok_or(ContractError::CounterOverflow)?;
        run.cursor = legacy_cursor;
        Self::save_run(&env, &run);

        env.events().publish(
            (soroban_sdk::symbol_short!("mapd"), legacy_id),
            (run_id, program, record.submitted_at, record.mapped_at),
        );
        Ok(RecordState::Mapped)
    }

    /// Report a legacy record that cannot be mapped.
    ///
    /// The count is exact and uncapped so a dataset with a systematic problem
    /// cannot hide inside a truncated list; the browsable detail is capped so
    /// a pathological dataset cannot fill the ledger with reasons.
    pub fn report_unmapped(
        env: Env,
        operator: Address,
        run_id: u64,
        legacy_id: BytesN<32>,
        reason: UnmappableReason,
        legacy_cursor: u64,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let run = Self::load_run(&env, run_id)?;
        if run.sealed {
            return Err(ContractError::RunSealed);
        }
        if legacy_cursor <= run.cursor {
            // Already accounted for by an earlier pass.
            return Ok(());
        }

        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, LegacyRecord>(&DataKey::Record(legacy_id.clone()))
        {
            if existing.state != RecordState::Eligible || existing.state == RecordState::Archived {
                return Err(ContractError::RecordConflict);
            }
        }

        let total: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::UnmappedTotal(run_id))
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &DataKey::UnmappedTotal(run_id),
            &total.checked_add(1).ok_or(ContractError::CounterOverflow)?,
        );

        let retained: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::UnmappedRetained(run_id))
            .unwrap_or(0u32);
        if retained < MAX_REPORTED_PER_RUN {
            env.storage()
                .persistent()
                .set(&DataKey::Report(run_id, retained), &legacy_id);
            env.storage().persistent().set(
                &DataKey::UnmappedRetained(run_id),
                &retained
                    .checked_add(1)
                    .ok_or(ContractError::CounterOverflow)?,
            );
        }

        let mut run = run;
        run.cursor = legacy_cursor;
        Self::save_run(&env, &run);

        env.events().publish(
            (soroban_sdk::symbol_short!("unmp"), legacy_id),
            (run_id, reason),
        );
        Ok(())
    }

    /// Seal a run. Idempotent, so a retry after a crash mid-seal is safe.
    pub fn seal_run(env: Env, operator: Address, run_id: u64) -> Result<RunSummary, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut run = Self::load_run(&env, run_id)?;
        if !run.sealed {
            run.sealed = true;
            run.sealed_at = env.ledger().timestamp();
            let open: u64 = env
                .storage()
                .instance()
                .get(&DataKey::OpenRuns)
                .unwrap_or(0u64);
            env.storage()
                .instance()
                .set(&DataKey::OpenRuns, &open.saturating_sub(1));
            Self::save_run(&env, &run);
            env.events().publish(
                (soroban_sdk::symbol_short!("rsel"), run_id),
                Self::summary(&env, &run),
            );
        }
        Ok(Self::summary(&env, &run))
    }

    /// Drop a mapped record from the browsable table.
    ///
    /// Only `Mapped` records may be archived, so an unmapped or unresolved
    /// record cannot be discarded to make room. The archival emits an event,
    /// so the mapping remains in the event log after the row is gone.
    pub fn archive_record(
        env: Env,
        operator: Address,
        legacy_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut record: LegacyRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Record(legacy_id.clone()))
            .ok_or(ContractError::RecordNotFound)?;
        if record.state != RecordState::Mapped {
            return Err(ContractError::RecordConflict);
        }
        record.state = RecordState::Archived;
        env.storage()
            .persistent()
            .set(&DataKey::Record(legacy_id.clone()), &record);
        let tracked: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TrackedRecords)
            .unwrap_or(0u64);
        env.storage()
            .instance()
            .set(&DataKey::TrackedRecords, &tracked.saturating_sub(1));

        env.events().publish(
            (soroban_sdk::symbol_short!("arch"), legacy_id),
            record.mapped_at,
        );
        Ok(())
    }

    // ── Views ─────────────────────────────────────────────────────────────

    pub fn get_run(env: Env, run_id: u64) -> Result<MigrationRun, ContractError> {
        Self::load_run(&env, run_id)
    }

    pub fn run_summary(env: Env, run_id: u64) -> Result<RunSummary, ContractError> {
        Ok(Self::summary(&env, &Self::load_run(&env, run_id)?))
    }

    pub fn get_record(env: Env, legacy_id: BytesN<32>) -> Result<LegacyRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Record(legacy_id))
            .ok_or(ContractError::RecordNotFound)
    }

    /// The state of a record a run has already accounted for, or
    /// `NotMapped` if this run has not seen it.
    pub fn record_state(env: Env, legacy_id: BytesN<32>) -> Result<RecordState, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Record(legacy_id))
            .map(|r: LegacyRecord| r.state)
            .ok_or(ContractError::NotMapped)
    }

    /// One reported-unmapped item by index. `unmapped_total` is the exact
    /// count; this is the browsable subset.
    pub fn get_reported(env: Env, run_id: u64, index: u32) -> Result<BytesN<32>, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Report(run_id, index))
            .ok_or(ContractError::RecordNotFound)
    }

    pub fn unmapped_total(env: Env, run_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::UnmappedTotal(run_id))
            .unwrap_or(0u64)
    }

    pub fn tracked_records(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::TrackedRecords)
            .unwrap_or(0u64)
    }

    pub fn version() -> u32 {
        CONTRACT_VERSION
    }
}

// ── Internals ──────────────────────────────────────────────────────────────

impl ScholarshipMigrationContract {
    fn require_admin(env: &Env, account: &Address) -> Result<(), ContractError> {
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match admin {
            Some(a) if a == *account => Ok(()),
            _ => Err(ContractError::NotAdmin),
        }
    }

    fn require_commitment(env: &Env, commitment: &BytesN<32>) -> Result<(), ContractError> {
        if *commitment == BytesN::from_array(env, &[0u8; 32]) {
            return Err(ContractError::EmptyCommitment);
        }
        Ok(())
    }

    fn load_run(env: &Env, run_id: u64) -> Result<MigrationRun, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Run(run_id))
            .ok_or(ContractError::RunNotFound)
    }

    fn save_run(env: &Env, run: &MigrationRun) {
        env.storage().persistent().set(&DataKey::Run(run.id), run);
        env.storage().persistent().extend_ttl(
            &DataKey::Run(run.id),
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );
    }

    fn save_record(env: &Env, record: &LegacyRecord) {
        env.storage()
            .persistent()
            .set(&DataKey::Record(record.legacy_id.clone()), record);
        env.storage().persistent().extend_ttl(
            &DataKey::Record(record.legacy_id.clone()),
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );
    }

    /// A record's state if this run already accounted for it.
    fn existing_state(env: &Env, legacy_id: &BytesN<32>) -> Result<RecordState, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Record(legacy_id.clone()))
            .map(|r: LegacyRecord| r.state)
            .ok_or(ContractError::NotMapped)
    }

    /// Move a dry run's cursor forward without recording anything else.
    fn advance(env: &Env, mut run: MigrationRun, cursor: u64) -> Result<(), ContractError> {
        // Monotonic: a cursor that could go backwards would let a re-export
        // replay records the run has already passed, and a "resume" would
        // silently skip the middle of a dataset.
        if cursor < run.cursor {
            return Err(ContractError::CursorRegression);
        }
        run.cursor = cursor;
        Self::save_run(env, &run);
        Ok(())
    }

    fn summary(env: &Env, run: &MigrationRun) -> RunSummary {
        RunSummary {
            id: run.id,
            source_system: run.source_system.clone(),
            dry_run: run.dry_run,
            cursor: run.cursor,
            mapped: run.mapped,
            unmapped: env
                .storage()
                .persistent()
                .get(&DataKey::UnmappedTotal(run.id))
                .unwrap_or(0u64),
            unmapped_retained: env
                .storage()
                .persistent()
                .get(&DataKey::UnmappedRetained(run.id))
                .unwrap_or(0u32),
            sealed: run.sealed,
        }
    }
}

#[cfg(test)]
mod tests;
