#![no_std]
// `set_config` takes a typed config's fields as explicit parameters. The
// alternative is a struct per config kind, which would multiply the ABI for
// no safety gain — the payload is validated either way. Same reasoning as
// `scholarship-core`, `scholarship-outbox`, and `scholarship-migration`.
#![allow(clippy::too_many_arguments)]

//! Scholarship rollout flags and administration configuration
//! (issues #1144, #1145).
//!
//! Two related problems, one contract, because they share an authority and
//! an audit trail: deciding **which features are exposed** (#1144), and
//! deciding **the parameters those features run under** (#1145).
//!
//! ## Flags are not permissions
//!
//! The single most important thing to say about this contract is that a
//! feature flag is **not an authorization decision**. A flag says "discovery
//! is on for cohort `early_partners`". It does not say that anyone in that
//! cohort may create an award, and nothing here can be used to conclude
//! that. The authoritative check stays server-side, in the backend that
//! already performs authorization against the scholarship contracts.
//!
//! This is why `scholarship-admin` has no function that takes a flag and
//! returns a capability, and why nothing here is an input to a transfer or
//! a payout. If a future change made a flag gate value-moving, this contract
//! would have become an authorization bug with a rollout UI attached.
//!
//! ## Flags fail closed
//!
//! A flag nobody has configured reads as [`FlagState::Off`], not `On`, and
//! an unknown flag or environment is an error rather than a default that
//! happens to enable something. A missing or malformed configuration must
//! never be the reason a feature becomes available.
//!
//! Flags are also three-state rather than boolean, because "on for two
//! cohorts" is a real rollout shape and a bool cannot express it without
//! either lying or growing a side channel:
//!
//! ```text
//! Off                 nothing is exposed
//! On                  everything is exposed
//! CohortsOnly(...)    only the named cohorts are exposed
//! ```
//!
//! ## Rollback cannot corrupt in-flight work
//!
//! Every flag and config change bumps a version and is retained, so a
//! rollback is publishing a previous version rather than editing the
//! present one. What a rollback deliberately does **not** do is unwind
//! workflows that already started: a workflow that read a flag at its start
//! keeps that decision, and `pin` records it so the choice is still legible
//! afterwards. A rollback changes what happens next, never what already
//! happened.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

/// Config and flag history is the audit trail, so it is kept longer than
/// ordinary records: at least ~6 months, extended toward ~14. An audit log
/// that expires is not one.
const RECORD_MIN_TTL: u32 = 15_552_000;
const RECORD_MAX_TTL: u32 = 31_104_000;

/// Ceiling on retained versions of one key. Old versions beyond this are
/// dropped oldest-first; the current version is never dropped, and the
/// retained range is reported so an auditor knows where the log starts.
const MAX_VERSIONS_PER_KEY: u32 = 32;

/// Ceiling on cohorts named by one flag. Bounded so a rollout cannot write
/// an unbounded list into instance storage.
const MAX_COHORTS: u32 = 32;

/// Ceiling on registered configuration keys across all kinds.
const MAX_CONFIG_KEYS: u64 = 2_000;

/// Ceiling on retained workflow pins. Pins are bounded by count as well as
/// by TTL: TTL alone would let a caller drive unbounded persistent storage
/// at a fixed cost per write.
const MAX_PINS: u64 = 5_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The flag name is not one this build knows.
    UnknownFlag = 4,
    /// The flag has never been configured, so it reads as `Off`. Returned by
    /// the *write* path; the read path answers `Off` instead, because a
    /// caller asking "is this on?" needs an answer, not an error.
    FlagNotConfigured = 5,
    /// More cohorts named than `MAX_COHORTS`.
    TooManyCohorts = 6,
    /// A cohort list with a duplicate or empty name.
    InvalidCohorts = 7,
    /// The configuration kind is not one this build knows.
    UnknownConfigKind = 8,
    /// No configuration exists at that key.
    ConfigNotFound = 9,
    /// The key table is at its ceiling.
    TooManyConfigKeys = 10,
    /// A limit is zero, or a maximum is below its minimum.
    InvalidLimit = 11,
    /// A deadline is zero, or an end is before its start.
    InvalidDeadline = 12,
    /// A risk threshold pair is out of order.
    InvalidThreshold = 13,
    /// A zero hash where a commitment or reference is required.
    EmptyReference = 14,
    /// The requested version has been evicted or never existed.
    VersionNotFound = 15,
    /// A proposed change is invalid. Returned by the preview path, which
    /// validates without writing.
    InvalidProposal = 16,
    /// No pin recorded for that workflow and flag.
    PinNotFound = 17,
    /// The pin table is at its ceiling.
    TooManyPins = 18,
}

/// The five independently-controllable surfaces named in #1144.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Flag {
    Discovery = 1,
    Applications = 2,
    Reviews = 3,
    Awards = 4,
    Payouts = 5,
}

impl Flag {
    /// Stable short name, spelled out rather than derived from the
    /// discriminant so reordering the enum cannot change a stored name.
    pub fn name(self, env: &Env) -> Symbol {
        match self {
            Flag::Discovery => Symbol::new(env, "discovery"),
            Flag::Applications => Symbol::new(env, "applications"),
            Flag::Reviews => Symbol::new(env, "reviews"),
            Flag::Awards => Symbol::new(env, "awards"),
            Flag::Payouts => Symbol::new(env, "payouts"),
        }
    }

    pub fn all() -> [Flag; 5] {
        [
            Flag::Discovery,
            Flag::Applications,
            Flag::Reviews,
            Flag::Awards,
            Flag::Payouts,
        ]
    }
}

/// Which environment a flag applies to, so a cohort allowlist written for
/// testnet cannot leak into production.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Testnet = 1,
    Staging = 2,
    Production = 3,
}

/// A flag's state for one environment. Three states, not a bool.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlagState {
    Off = 1,
    On = 2,
    /// Only these cohorts are exposed. Never empty — an empty list would be
    /// `Off` wearing a different hat, and the distinction is exactly what a
    /// reviewer needs to see.
    CohortsOnly = 3,
}

/// One configured flag, with the version and who changed it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlagConfig {
    pub flag: Flag,
    pub environment: Environment,
    pub state: FlagState,
    pub cohorts: Vec<Symbol>,
    pub version: u32,
    /// Ledger time the current version took effect.
    pub effective_at: u64,
    pub updated_by: Address,
}

/// The six configuration kinds named in #1145.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigKind {
    Limits = 1,
    Deadline = 2,
    Provider = 3,
    Asset = 4,
    Template = 5,
    RiskThreshold = 6,
}

impl ConfigKind {
    pub fn all() -> [ConfigKind; 6] {
        [
            ConfigKind::Limits,
            ConfigKind::Deadline,
            ConfigKind::Provider,
            ConfigKind::Asset,
            ConfigKind::Template,
            ConfigKind::RiskThreshold,
        ]
    }
}

/// A bounded, validated administration parameter.
///
/// One record for all six kinds, with `a`/`b` as the two numeric fields and
/// `ref_hash` as the optional commitment. Six parallel structs would be six
/// ABIs to keep in step; one validated record keeps the invariant "a config
/// that exists is a config that passed validation" checkable in one place.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminConfig {
    pub kind: ConfigKind,
    /// Caller-chosen identifier within the kind, e.g. a program id.
    pub subject: BytesN<32>,
    /// First numeric field: a minimum, an amount, a start, a score.
    pub a: i128,
    /// Second numeric field: a maximum, a multiplier, an end.
    pub b: i128,
    /// A commitment or *reference* — never a secret. See `set_config`.
    pub ref_hash: BytesN<32>,
    /// Bounded label, e.g. an asset's display name.
    pub label: Symbol,
    pub version: u32,
    pub updated_at: u64,
    pub updated_by: Address,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Current state of one flag in one environment.
    Flag(Flag, Environment),
    /// A retained past version of that flag.
    FlagVersion(Flag, Environment, u32),
    /// Lowest retained flag version.
    FlagFloor(Flag, Environment),
    /// Current value of one configuration key.
    Config(ConfigKind, BytesN<32>),
    /// A retained past version of that key.
    ConfigVersion(ConfigKind, BytesN<32>, u32),
    /// Lowest retained configuration version.
    ConfigFloor(ConfigKind, BytesN<32>),
    /// Subjects registered under a kind, so `list_config` does not need to
    /// scan unknown storage keys.
    ConfigIndex(ConfigKind),
    /// A workflow's flag decision, which a later rollback does not rewrite.
    Pin(BytesN<32>, Flag, Environment),
    /// Counters, held in instance storage so they cannot expire and silently
    /// reset a bound to zero.
    ConfigKeyCount,
    PinCount,
}

#[contract]
pub struct ScholarshipAdminContract;

#[contractimpl]
impl ScholarshipAdminContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // ═════════════════════════════════════════════════════════════════════
    // Feature flags (#1144)
    // ═════════════════════════════════════════════════════════════════════

    /// Configure a flag for one environment.
    ///
    /// Emits the new version alongside the version it replaced, so the
    /// exposure change is auditable from the event stream alone without
    /// reconstructing it from storage.
    pub fn set_flag(
        env: Env,
        admin: Address,
        flag: Flag,
        environment: Environment,
        state: FlagState,
        cohorts: Vec<Symbol>,
    ) -> Result<u32, ContractError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        Self::check_cohorts(&cohorts, &state)?;

        let key = DataKey::Flag(flag, environment);
        let previous: Option<FlagConfig> = env.storage().persistent().get(&key);
        if previous.is_none() {
            Self::reserve_config_key(&env)?;
        }

        let version = match &previous {
            Some(p) => p
                .version
                .checked_add(1)
                .ok_or(ContractError::InvalidLimit)?,
            None => 1,
        };
        let config = FlagConfig {
            flag,
            environment,
            state: state.clone(),
            cohorts,
            version,
            effective_at: env.ledger().timestamp(),
            updated_by: admin.clone(),
        };
        Self::save_flag(&env, &config)?;

        env.events().publish(
            (symbol_short!("flag"), flag.name(&env), environment),
            (
                version,
                state,
                previous.map(|p| p.version).unwrap_or(0),
                admin,
                config.effective_at,
            ),
        );
        Ok(version)
    }

    /// Whether a cohort may use a surface.
    ///
    /// **Read-only, fail-closed, and never an authorization decision.** An
    /// unconfigured flag or environment answers `false` rather than erroring,
    /// because the caller asking this question needs a boolean to gate a
    /// menu with — and the safe answer to "is this configured?" is no.
    ///
    /// A real permission check belongs server-side, in the backend that
    /// already authorizes against the scholarship contracts.
    pub fn is_enabled(env: Env, flag: Flag, environment: Environment, cohort: Symbol) -> bool {
        let config: Option<FlagConfig> = env
            .storage()
            .persistent()
            .get(&DataKey::Flag(flag, environment));
        match config {
            // Unconfigured fails closed.
            None => false,
            Some(c) => match c.state {
                FlagState::Off => false,
                FlagState::On => true,
                FlagState::CohortsOnly => c.cohorts.contains(&cohort),
            },
        }
    }

    /// The configured state, for an operator reading the rollout.
    ///
    /// An unconfigured flag reads as `Off` at version 0 so the whole
    /// exposure surface can be listed in one pass. In that case
    /// `updated_by` is the contract admin and `effective_at` is 0: nobody
    /// configured it, and attributing it to a fabricated address would be a
    /// worse lie than attributing it to the authority that could have.
    ///
    /// Errors on an uninitialized contract rather than inventing an
    /// administrator to attribute the read to.
    pub fn get_flag(
        env: Env,
        flag: Flag,
        environment: Environment,
    ) -> Result<FlagConfig, ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Flag(flag, environment))
            .unwrap_or(FlagConfig {
                flag,
                environment,
                state: FlagState::Off,
                cohorts: Vec::new(&env),
                version: 0,
                effective_at: 0,
                updated_by: admin,
            }))
    }

    /// Every flag's state in one environment, so the whole exposure surface
    /// is inspectable at once rather than five calls that might disagree
    /// with each other by the time they are read.
    pub fn list_flags(
        env: Env,
        environment: Environment,
    ) -> Result<Vec<FlagConfig>, ContractError> {
        let mut out = Vec::new(&env);
        for flag in Flag::all() {
            out.push_back(Self::get_flag(env.clone(), flag, environment)?);
        }
        Ok(out)
    }

    /// A specific retained past version of a flag, for auditing what was
    /// exposed when.
    pub fn flag_version(
        env: Env,
        flag: Flag,
        environment: Environment,
        version: u32,
    ) -> Result<FlagConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FlagVersion(flag, environment, version))
            .ok_or(ContractError::VersionNotFound)
    }

    /// The lowest retained flag version, so an auditor knows where the
    /// history begins rather than assuming version 1 is still available.
    pub fn flag_version_floor(env: Env, flag: Flag, environment: Environment) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::FlagFloor(flag, environment))
            .unwrap_or(1u32)
    }

    /// Record a workflow's flag decision so a later rollback does not make
    /// it illegible.
    ///
    /// A workflow that read a flag at its start keeps that decision. Without
    /// a pin, rolling a flag back would leave in-flight work referring to a
    /// configuration that no longer exists anywhere, and the only way to
    /// explain it would be the deploy log.
    pub fn pin(
        env: Env,
        flag: Flag,
        environment: Environment,
        workflow: BytesN<32>,
    ) -> Result<FlagState, ContractError> {
        let config = Self::get_flag(env.clone(), flag, environment)?;
        let key = DataKey::Pin(workflow, flag, environment);
        // Count only a pin that is new, so re-pinning the same workflow is
        // not a way to ratchet the counter up to the ceiling.
        if !env.storage().persistent().has(&key) {
            Self::reserve_pin(&env)?;
        }
        env.storage().persistent().set(&key, &config.state);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(config.state)
    }

    /// What a workflow decided when it started, which may differ from the
    /// flag's current value. This is the answer to "why did this award go
    /// through when the flag says payouts are off?" — it started when the
    /// flag said otherwise.
    pub fn pinned_state(
        env: Env,
        flag: Flag,
        environment: Environment,
        workflow: BytesN<32>,
    ) -> Result<FlagState, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Pin(workflow, flag, environment))
            .ok_or(ContractError::PinNotFound)
    }

    // ═════════════════════════════════════════════════════════════════════
    // Administration configuration (#1145)
    // ═════════════════════════════════════════════════════════════════════

    /// Set a configuration value after validating it.
    ///
    /// Returns the new version. Every write is versioned and retained, and
    /// emits an event carrying the version it replaced — so "who changed
    /// what, from which version" is answerable from the event stream
    /// without replaying storage.
    ///
    /// **`ref_hash` is a reference or a commitment, never a secret.** A
    /// provider's API credential is held in an off-chain secret manager and
    /// referenced by hash from here; this contract never stores, accepts, or
    /// returns a secret value. A view returning one would put the credential
    /// into every `simulate` and indexer.
    pub fn set_config(
        env: Env,
        admin: Address,
        kind: ConfigKind,
        subject: BytesN<32>,
        a: i128,
        b: i128,
        ref_hash: BytesN<32>,
        label: Symbol,
    ) -> Result<u32, ContractError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;
        Self::validate(kind, a, b, &ref_hash)?;

        let key = DataKey::Config(kind, subject.clone());
        let previous: Option<AdminConfig> = env.storage().persistent().get(&key);
        if previous.is_none() {
            Self::reserve_config_key(&env)?;
            Self::index_config(&env, kind, &subject);
        }

        let version = match &previous {
            Some(p) => p
                .version
                .checked_add(1)
                .ok_or(ContractError::InvalidLimit)?,
            None => 1,
        };
        let config = AdminConfig {
            kind,
            subject: subject.clone(),
            a,
            b,
            ref_hash,
            label,
            version,
            updated_at: env.ledger().timestamp(),
            updated_by: admin.clone(),
        };
        Self::save_config(&env, &config)?;

        env.events().publish(
            (symbol_short!("cfg"), kind, subject),
            (
                version,
                previous.map(|p| p.version).unwrap_or(0),
                admin,
                config.updated_at,
            ),
        );
        Ok(version)
    }

    /// Check a proposed configuration **without writing it**.
    ///
    /// The point of a preview is to run the *same* validation the write path
    /// runs, so what an operator is shown cannot disagree with what the
    /// transaction will do. Returns `Ok(())` or the exact error the write
    /// would produce, and touches no state.
    pub fn preview_config(
        kind: ConfigKind,
        a: i128,
        b: i128,
        ref_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::validate(kind, a, b, &ref_hash)
    }

    /// Republish a previous version as the current one — a rollback.
    ///
    /// This *appends* a version rather than rewinding the counter, so the
    /// rollback is itself auditable and history stays append-only. An
    /// in-flight workflow keeps its pinned decision; only future reads see
    /// the restored value.
    pub fn rollback_config(
        env: Env,
        admin: Address,
        kind: ConfigKind,
        subject: BytesN<32>,
        version: u32,
    ) -> Result<u32, ContractError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let target: AdminConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ConfigVersion(kind, subject.clone(), version))
            .ok_or(ContractError::VersionNotFound)?;
        let current: AdminConfig = env
            .storage()
            .persistent()
            .get(&DataKey::Config(kind, subject.clone()))
            .ok_or(ContractError::ConfigNotFound)?;

        // The new version continues from the *current* head, not from the
        // restored one. Deriving it from the restored version would write
        // over a version that already exists and destroy history, which is
        // the opposite of what a rollback is for.
        let new_version = current
            .version
            .checked_add(1)
            .ok_or(ContractError::InvalidLimit)?;
        let restored = AdminConfig {
            version: new_version,
            updated_at: env.ledger().timestamp(),
            updated_by: admin.clone(),
            ..target
        };
        Self::save_config(&env, &restored)?;

        env.events().publish(
            (symbol_short!("rbck"), kind, subject),
            (version, new_version, admin, restored.updated_at),
        );
        Ok(new_version)
    }

    pub fn get_config(
        env: Env,
        kind: ConfigKind,
        subject: BytesN<32>,
    ) -> Result<AdminConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Config(kind, subject))
            .ok_or(ContractError::ConfigNotFound)
    }

    pub fn config_version(
        env: Env,
        kind: ConfigKind,
        subject: BytesN<32>,
        version: u32,
    ) -> Result<AdminConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ConfigVersion(kind, subject, version))
            .ok_or(ContractError::VersionNotFound)
    }

    /// The lowest retained version, so an auditor knows where the history
    /// begins rather than assuming version 1 is available.
    pub fn config_version_floor(env: Env, kind: ConfigKind, subject: BytesN<32>) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ConfigFloor(kind, subject))
            .unwrap_or(1u32)
    }

    /// Every subject configured under a kind, so an operator can audit one
    /// category without having to know its keys in advance.
    pub fn list_config(env: Env, kind: ConfigKind) -> Vec<AdminConfig> {
        let index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::ConfigIndex(kind))
            .unwrap_or(Vec::new(&env));
        let mut out = Vec::new(&env);
        for subject in index.iter() {
            if let Ok(c) = Self::get_config(env.clone(), kind, subject) {
                out.push_back(c);
            }
        }
        out
    }

    // ── Views ─────────────────────────────────────────────────────────────

    pub fn tracked_config_keys(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::ConfigKeyCount)
            .unwrap_or(0u64)
    }

    pub fn tracked_pins(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::PinCount)
            .unwrap_or(0u64)
    }

    pub fn version() -> u32 {
        CONTRACT_VERSION
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Internals
// ═══════════════════════════════════════════════════════════════════════════

impl ScholarshipAdminContract {
    fn require_admin(env: &Env, account: &Address) -> Result<(), ContractError> {
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match admin {
            Some(a) if a == *account => Ok(()),
            _ => Err(ContractError::NotAdmin),
        }
    }

    /// Rejects a cohort list that is too long, or that repeats or empties a
    /// name.
    ///
    /// The duplicate check matters: two identical cohort names would make
    /// `is_enabled` and any operator reading the list disagree about how
    /// many cohorts are targeted, which is exactly the kind of discrepancy
    /// a staged rollout is supposed to rule out.
    fn check_cohorts(cohorts: &Vec<Symbol>, state: &FlagState) -> Result<(), ContractError> {
        if cohorts.len() > MAX_COHORTS {
            return Err(ContractError::TooManyCohorts);
        }
        let mut seen: Vec<Symbol> = Vec::new(cohorts.env());
        for cohort in cohorts.iter() {
            if cohort == Symbol::new(cohorts.env(), "") {
                return Err(ContractError::InvalidCohorts);
            }
            if seen.contains(&cohort) {
                return Err(ContractError::InvalidCohorts);
            }
            seen.push_back(cohort);
        }
        // `CohortsOnly` with no cohorts would be `Off` under another name,
        // and an operator reading the state would be told a cohort allowlist
        // exists when none does.
        if *state == FlagState::CohortsOnly && cohorts.is_empty() {
            return Err(ContractError::InvalidCohorts);
        }
        Ok(())
    }

    /// The single validation gate for configuration values.
    ///
    /// Every kind's rules live here, so "a config that exists passed
    /// validation" is one property checkable in one place — and so the
    /// preview path provably runs the same code as the write path, since
    /// both call this.
    fn validate(
        kind: ConfigKind,
        a: i128,
        b: i128,
        ref_hash: &BytesN<32>,
    ) -> Result<(), ContractError> {
        match kind {
            // A limit of zero, or a maximum below its minimum, is not a
            // limit — it is a value that will reject everything or nothing.
            ConfigKind::Limits => {
                if a <= 0 || b < a {
                    return Err(ContractError::InvalidLimit);
                }
            }
            // `a` is a start, `b` an end. An end before its start is a
            // deadline nobody can meet.
            ConfigKind::Deadline => {
                if a <= 0 || b <= 0 || b < a {
                    return Err(ContractError::InvalidDeadline);
                }
            }
            // A provider must reference *something*; an all-zero hash would
            // be a provider with no credential reference, discovered only
            // when the first payment fails.
            ConfigKind::Provider => {
                if ref_hash.to_array().iter().all(|byte| *byte == 0) {
                    return Err(ContractError::EmptyReference);
                }
            }
            ConfigKind::Asset => {
                if a <= 0 {
                    return Err(ContractError::InvalidLimit);
                }
            }
            // A template carries only a label and a reference, neither of
            // which admits an invalid value.
            ConfigKind::Template => {}
            // A risk band must be ordered, or the classification it drives
            // is undefined for every value in the gap.
            ConfigKind::RiskThreshold => {
                if a < 0 || b < a {
                    return Err(ContractError::InvalidThreshold);
                }
            }
        }
        Ok(())
    }

    fn reserve_config_key(env: &Env) -> Result<(), ContractError> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ConfigKeyCount)
            .unwrap_or(0u64);
        if count >= MAX_CONFIG_KEYS {
            return Err(ContractError::TooManyConfigKeys);
        }
        env.storage().instance().set(
            &DataKey::ConfigKeyCount,
            &count.checked_add(1).ok_or(ContractError::InvalidLimit)?,
        );
        Ok(())
    }

    fn reserve_pin(env: &Env) -> Result<(), ContractError> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PinCount)
            .unwrap_or(0u64);
        if count >= MAX_PINS {
            return Err(ContractError::TooManyPins);
        }
        env.storage().instance().set(
            &DataKey::PinCount,
            &count.checked_add(1).ok_or(ContractError::InvalidLimit)?,
        );
        Ok(())
    }

    fn index_config(env: &Env, kind: ConfigKind, subject: &BytesN<32>) {
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::ConfigIndex(kind))
            .unwrap_or(Vec::new(env));
        if !index.contains(subject) {
            index.push_back(subject.clone());
        }
        env.storage()
            .persistent()
            .set(&DataKey::ConfigIndex(kind), &index);
    }

    fn save_flag(env: &Env, config: &FlagConfig) -> Result<(), ContractError> {
        let key = DataKey::Flag(config.flag, config.environment);
        env.storage().persistent().set(&key, config);
        let version_key = DataKey::FlagVersion(config.flag, config.environment, config.version);
        env.storage().persistent().set(&version_key, config);

        if config.version == 1 {
            env.storage()
                .persistent()
                .set(&DataKey::FlagFloor(config.flag, config.environment), &1u32);
        }
        // Evict the oldest retained version once the window is full. The
        // floor can only ever reach `version - MAX_VERSIONS_PER_KEY + 1`, so
        // the current version is never a candidate.
        if config.version > MAX_VERSIONS_PER_KEY {
            let stale = config
                .version
                .checked_sub(MAX_VERSIONS_PER_KEY)
                .ok_or(ContractError::InvalidLimit)?;
            env.storage().persistent().remove(&DataKey::FlagVersion(
                config.flag,
                config.environment,
                stale,
            ));
            let floor = stale.checked_add(1).ok_or(ContractError::InvalidLimit)?;
            env.storage()
                .persistent()
                .set(&DataKey::FlagFloor(config.flag, config.environment), &floor);
        }

        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn save_config(env: &Env, config: &AdminConfig) -> Result<(), ContractError> {
        let key = DataKey::Config(config.kind, config.subject.clone());
        env.storage().persistent().set(&key, config);
        let version_key =
            DataKey::ConfigVersion(config.kind, config.subject.clone(), config.version);
        env.storage().persistent().set(&version_key, config);

        if config.version == 1 {
            env.storage().persistent().set(
                &DataKey::ConfigFloor(config.kind, config.subject.clone()),
                &1u32,
            );
        }
        if config.version > MAX_VERSIONS_PER_KEY {
            let stale = config
                .version
                .checked_sub(MAX_VERSIONS_PER_KEY)
                .ok_or(ContractError::InvalidLimit)?;
            env.storage().persistent().remove(&DataKey::ConfigVersion(
                config.kind,
                config.subject.clone(),
                stale,
            ));
            let floor = stale.checked_add(1).ok_or(ContractError::InvalidLimit)?;
            env.storage().persistent().set(
                &DataKey::ConfigFloor(config.kind, config.subject.clone()),
                &floor,
            );
        }

        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
