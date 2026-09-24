//! Centralized configuration and dependency validation (#1002).
//!
//! An invalid configuration value does not fail loudly — it bricks circulation
//! quietly. A loan window with `not_before >= expires_at` makes every checkout
//! impossible; a basis-point rate above 10,000 charges more than the principal;
//! a dependency address pointed at the wrong deployment makes every
//! cross-contract call fail at the far end, where the error is unrecognisable.
//!
//! Every setter in the library domain routes its inputs through this module
//! **before** touching storage, so a rejected update leaves the previous
//! configuration exactly as it was.
//!
//! ## Why validate-then-store rather than store-then-check
//!
//! Soroban rolls a failed invocation back, so in-transaction damage is undone
//! either way. The reason the order still matters is partial application across
//! *fields*: a setter that writes three values and validates the fourth has
//! already emitted events and bumped TTLs for the first three. [`Validated`]
//! makes the ordering explicit — a value cannot reach storage without having
//! passed its check first.
//!
//! ## Impact
//!
//! Additive library code in the `shared` crate. No ABI, storage, event,
//! privacy, deployment, or migration impact: no contract entrypoint changes and
//! no storage key is introduced. Adopting a validator in a setter that
//! previously accepted a now-rejected value is a behaviour change for that
//! contract and should be called out in the PR that does it.

use soroban_sdk::{contracterror, Address, Bytes, Env, String};

/// One basis point is 1/10,000. A rate of 10,000 bp is 100%.
pub const BASIS_POINTS_DENOMINATOR: u32 = 10_000;

/// Typed configuration failures.
///
/// Discriminants start at 240 so they cannot collide with `ContractError`
/// (1–15), `MathError` (200+), or `SigningError` (220+).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ConfigError {
    /// A time range whose start is not strictly before its end.
    InvalidTimeRange = 240,
    /// A window that has already closed relative to the current ledger time.
    WindowAlreadyClosed = 241,
    /// A window longer than the configured maximum.
    WindowTooLong = 242,
    /// A basis-point rate above 10,000 (100%).
    BasisPointsOutOfRange = 243,
    /// A size or count limit outside its permitted band.
    LimitOutOfRange = 244,
    /// A required text value was empty.
    EmptyValue = 245,
    /// A text value longer than its permitted maximum.
    ValueTooLong = 246,
    /// A dependency address equal to the configuring contract itself.
    SelfReferentialDependency = 247,
    /// A dependency reporting an interface version this build cannot use.
    IncompatibleInterfaceVersion = 248,
}

/// A value that has passed its validator.
///
/// Construct one only through a `validate_*` function. Contracts take
/// `Validated<T>` at the point of storage, so "was this checked?" is answered
/// by the type rather than by reading the call site.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Validated<T>(T);

impl<T> Validated<T> {
    /// Unwraps the checked value at the point of storage.
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Borrows the checked value.
    pub fn get(&self) -> &T {
        &self.0
    }
}

// ── Time ranges ────────────────────────────────────────────────────────────

/// A validity window, already checked.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TimeRange {
    /// Inclusive start.
    pub not_before: u64,
    /// Exclusive end.
    pub expires_at: u64,
}

/// Validates `[not_before, expires_at)` against the current ledger time.
///
/// * The range must be non-empty: `not_before < expires_at`.
/// * It must not have already closed: `expires_at > now`. A window that is
///   dead on arrival is always a caller mistake, and storing one produces a
///   configuration nothing can ever satisfy.
/// * Its duration must not exceed `max_duration_secs`, which bounds how far a
///   single configuration change can commit the library.
///
/// A window that has not opened yet is accepted — scheduling ahead is
/// legitimate.
pub fn validate_time_range(
    not_before: u64,
    expires_at: u64,
    now: u64,
    max_duration_secs: u64,
) -> Result<Validated<TimeRange>, ConfigError> {
    if not_before >= expires_at {
        return Err(ConfigError::InvalidTimeRange);
    }
    if expires_at <= now {
        return Err(ConfigError::WindowAlreadyClosed);
    }
    // `not_before < expires_at` is established, so this cannot underflow.
    if expires_at - not_before > max_duration_secs {
        return Err(ConfigError::WindowTooLong);
    }
    Ok(Validated(TimeRange {
        not_before,
        expires_at,
    }))
}

// ── Rates and limits ───────────────────────────────────────────────────────

/// Validates a basis-point rate, rejecting anything above 100%.
///
/// Zero is accepted: a zero rate is a legitimate configuration (no fee), and
/// rejecting it would force callers to model "no fee" as an absent value.
pub fn validate_basis_points(bps: u32) -> Result<Validated<u32>, ConfigError> {
    if bps > BASIS_POINTS_DENOMINATOR {
        return Err(ConfigError::BasisPointsOutOfRange);
    }
    Ok(Validated(bps))
}

/// Validates a size, count, or page limit against an inclusive band.
///
/// Both ends are checked, because an unreasonably *small* limit bricks
/// circulation just as effectively as an unreasonably large one — a maximum
/// batch size of zero means no batch can ever be submitted.
pub fn validate_limit(value: u32, min: u32, max: u32) -> Result<Validated<u32>, ConfigError> {
    if value < min || value > max {
        return Err(ConfigError::LimitOutOfRange);
    }
    Ok(Validated(value))
}

// ── Text ───────────────────────────────────────────────────────────────────

/// Validates a required text value's length.
///
/// Length is measured in bytes, matching how Soroban charges for the value.
pub fn validate_text(value: &String, max_len: u32) -> Result<(), ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::EmptyValue);
    }
    if value.len() > max_len {
        return Err(ConfigError::ValueTooLong);
    }
    Ok(())
}

/// Validates a required byte string's length.
pub fn validate_bytes(value: &Bytes, min_len: u32, max_len: u32) -> Result<(), ConfigError> {
    if value.len() < min_len {
        return Err(ConfigError::EmptyValue);
    }
    if value.len() > max_len {
        return Err(ConfigError::ValueTooLong);
    }
    Ok(())
}

// ── Dependencies ───────────────────────────────────────────────────────────

/// Validates a dependency contract address.
///
/// The only structural check available on-chain is that a contract is not
/// pointed at itself — a self-referential dependency turns a cross-contract
/// call into unbounded recursion. Address *wellformedness* is already
/// guaranteed by the `Address` type, so there is nothing further to check here;
/// whether the address hosts the expected contract is what
/// [`validate_interface_version`] is for.
pub fn validate_dependency(env: &Env, dependency: &Address) -> Result<(), ConfigError> {
    if *dependency == env.current_contract_address() {
        return Err(ConfigError::SelfReferentialDependency);
    }
    Ok(())
}

/// Validates a dependency's reported interface version under semantic
/// versioning: the major version must match exactly, and the minor version must
/// be at least what this build requires.
///
/// A dependency ahead on minor is fine — it has everything this build calls. A
/// dependency behind on minor, or on any other major, is not.
pub fn validate_interface_version(
    required_major: u32,
    required_minor: u32,
    reported_major: u32,
    reported_minor: u32,
) -> Result<(), ConfigError> {
    if reported_major != required_major || reported_minor < required_minor {
        return Err(ConfigError::IncompatibleInterfaceVersion);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{contract, contractimpl, testutils::Address as _, Env};

    const DAY: u64 = 86_400;
    const MAX_WINDOW: u64 = 365 * DAY;

    // ── Time ranges: boundaries ────────────────────────────────────────────

    #[test]
    fn a_window_is_accepted_when_it_opens_before_it_closes() {
        let range = validate_time_range(1_000, 2_000, 500, MAX_WINDOW).unwrap();
        assert_eq!(range.get().not_before, 1_000);
        assert_eq!(range.get().expires_at, 2_000);
    }

    #[test]
    fn an_empty_or_inverted_window_is_rejected() {
        assert_eq!(
            validate_time_range(2_000, 2_000, 0, MAX_WINDOW),
            Err(ConfigError::InvalidTimeRange)
        );
        assert_eq!(
            validate_time_range(2_001, 2_000, 0, MAX_WINDOW),
            Err(ConfigError::InvalidTimeRange)
        );
    }

    #[test]
    fn a_window_that_has_already_closed_is_rejected() {
        // Closing exactly now is closed: the end is exclusive.
        assert_eq!(
            validate_time_range(1_000, 2_000, 2_000, MAX_WINDOW),
            Err(ConfigError::WindowAlreadyClosed)
        );
        assert_eq!(
            validate_time_range(1_000, 2_000, 2_001, MAX_WINDOW),
            Err(ConfigError::WindowAlreadyClosed)
        );
        // One second before it closes it is still valid.
        assert!(validate_time_range(1_000, 2_000, 1_999, MAX_WINDOW).is_ok());
    }

    #[test]
    fn a_window_that_has_not_opened_yet_is_accepted() {
        assert!(validate_time_range(10_000, 20_000, 500, MAX_WINDOW).is_ok());
    }

    #[test]
    fn a_window_longer_than_the_maximum_is_rejected_at_the_boundary() {
        assert!(validate_time_range(0, MAX_WINDOW, 0, MAX_WINDOW).is_ok());
        assert_eq!(
            validate_time_range(0, MAX_WINDOW + 1, 0, MAX_WINDOW),
            Err(ConfigError::WindowTooLong)
        );
    }

    #[test]
    fn an_extreme_window_does_not_overflow_the_duration_check() {
        assert_eq!(
            validate_time_range(0, u64::MAX, 0, MAX_WINDOW),
            Err(ConfigError::WindowTooLong)
        );
        assert_eq!(
            validate_time_range(u64::MAX - 1, u64::MAX, 0, MAX_WINDOW),
            Ok(Validated(TimeRange {
                not_before: u64::MAX - 1,
                expires_at: u64::MAX,
            }))
        );
    }

    // ── Basis points ───────────────────────────────────────────────────────

    #[test]
    fn basis_points_accepts_zero_through_one_hundred_percent() {
        assert_eq!(validate_basis_points(0).unwrap().into_inner(), 0);
        assert_eq!(validate_basis_points(250).unwrap().into_inner(), 250);
        assert_eq!(validate_basis_points(10_000).unwrap().into_inner(), 10_000);
    }

    #[test]
    fn basis_points_above_one_hundred_percent_are_rejected() {
        assert_eq!(
            validate_basis_points(10_001),
            Err(ConfigError::BasisPointsOutOfRange)
        );
        assert_eq!(
            validate_basis_points(u32::MAX),
            Err(ConfigError::BasisPointsOutOfRange)
        );
    }

    // ── Limits ─────────────────────────────────────────────────────────────

    #[test]
    fn a_limit_is_accepted_on_both_inclusive_bounds() {
        assert!(validate_limit(1, 1, 100).is_ok());
        assert!(validate_limit(100, 1, 100).is_ok());
        assert!(validate_limit(50, 1, 100).is_ok());
    }

    #[test]
    fn a_limit_outside_its_band_is_rejected_on_either_side() {
        // Zero is as much a brick as u32::MAX: no batch could ever be accepted.
        assert_eq!(validate_limit(0, 1, 100), Err(ConfigError::LimitOutOfRange));
        assert_eq!(
            validate_limit(101, 1, 100),
            Err(ConfigError::LimitOutOfRange)
        );
        assert_eq!(
            validate_limit(u32::MAX, 1, 100),
            Err(ConfigError::LimitOutOfRange)
        );
    }

    #[test]
    fn a_degenerate_band_accepts_only_its_single_value() {
        assert!(validate_limit(7, 7, 7).is_ok());
        assert_eq!(validate_limit(6, 7, 7), Err(ConfigError::LimitOutOfRange));
        assert_eq!(validate_limit(8, 7, 7), Err(ConfigError::LimitOutOfRange));
    }

    // ── Text ───────────────────────────────────────────────────────────────

    #[test]
    fn text_validation_rejects_empty_and_oversized_values() {
        let env = Env::default();
        assert_eq!(
            validate_text(&String::from_str(&env, ""), 8),
            Err(ConfigError::EmptyValue)
        );
        assert!(validate_text(&String::from_str(&env, "12345678"), 8).is_ok());
        assert_eq!(
            validate_text(&String::from_str(&env, "123456789"), 8),
            Err(ConfigError::ValueTooLong)
        );
    }

    #[test]
    fn byte_validation_checks_both_bounds() {
        let env = Env::default();
        let four = Bytes::from_slice(&env, &[1, 2, 3, 4]);
        assert!(validate_bytes(&four, 4, 4).is_ok());
        assert_eq!(validate_bytes(&four, 5, 8), Err(ConfigError::EmptyValue));
        assert_eq!(validate_bytes(&four, 1, 3), Err(ConfigError::ValueTooLong));
    }

    // ── Dependencies ───────────────────────────────────────────────────────

    #[contract]
    struct Probe;

    #[contractimpl]
    impl Probe {
        /// Runs `validate_dependency` from inside a contract context, which is
        /// where `current_contract_address` is meaningful.
        pub fn check(env: Env, dependency: Address) -> Result<(), ConfigError> {
            validate_dependency(&env, &dependency)
        }
    }

    #[test]
    fn a_dependency_pointed_at_a_different_contract_is_accepted() {
        let env = Env::default();
        let probe_id = env.register(Probe, ());
        let client = ProbeClient::new(&env, &probe_id);
        let other = Address::generate(&env);

        assert_eq!(client.try_check(&other), Ok(Ok(())));
    }

    #[test]
    fn a_self_referential_dependency_is_rejected() {
        let env = Env::default();
        let probe_id = env.register(Probe, ());
        let client = ProbeClient::new(&env, &probe_id);

        assert_eq!(
            client.try_check(&probe_id),
            Err(Ok(ConfigError::SelfReferentialDependency))
        );
    }

    // ── Interface versions ─────────────────────────────────────────────────

    #[test]
    fn an_exact_interface_version_match_is_accepted() {
        assert!(validate_interface_version(1, 2, 1, 2).is_ok());
    }

    #[test]
    fn a_dependency_ahead_on_minor_is_accepted() {
        assert!(validate_interface_version(1, 2, 1, 3).is_ok());
        assert!(validate_interface_version(1, 0, 1, u32::MAX).is_ok());
    }

    #[test]
    fn a_dependency_behind_on_minor_is_rejected() {
        assert_eq!(
            validate_interface_version(1, 2, 1, 1),
            Err(ConfigError::IncompatibleInterfaceVersion)
        );
    }

    #[test]
    fn any_other_major_version_is_rejected_in_both_directions() {
        assert_eq!(
            validate_interface_version(1, 0, 2, 0),
            Err(ConfigError::IncompatibleInterfaceVersion)
        );
        assert_eq!(
            validate_interface_version(2, 0, 1, 0),
            Err(ConfigError::IncompatibleInterfaceVersion)
        );
        // A higher minor does not rescue a wrong major.
        assert_eq!(
            validate_interface_version(1, 0, 2, u32::MAX),
            Err(ConfigError::IncompatibleInterfaceVersion)
        );
    }

    // ── Prior state survives a failed update ───────────────────────────────

    /// A configuration record standing in for a real contract's stored config.
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Config {
        late_fee_bps: u32,
        max_batch: u32,
        window: TimeRange,
    }

    /// Validate-then-store: every field is checked before any is written.
    fn apply_update(
        current: &Config,
        late_fee_bps: u32,
        max_batch: u32,
        not_before: u64,
        expires_at: u64,
        now: u64,
    ) -> Result<Config, ConfigError> {
        let bps = validate_basis_points(late_fee_bps)?;
        let batch = validate_limit(max_batch, 1, 100)?;
        let window = validate_time_range(not_before, expires_at, now, MAX_WINDOW)?;
        let _ = current;
        Ok(Config {
            late_fee_bps: bps.into_inner(),
            max_batch: batch.into_inner(),
            window: window.into_inner(),
        })
    }

    fn baseline() -> Config {
        Config {
            late_fee_bps: 100,
            max_batch: 10,
            window: TimeRange {
                not_before: 0,
                expires_at: 10_000,
            },
        }
    }

    #[test]
    fn a_valid_update_replaces_every_field() {
        let before = baseline();
        let after = apply_update(&before, 250, 20, 1_000, 5_000, 0).unwrap();

        assert_eq!(after.late_fee_bps, 250);
        assert_eq!(after.max_batch, 20);
        assert_eq!(after.window.not_before, 1_000);
    }

    #[test]
    fn a_failure_on_the_last_field_leaves_the_prior_config_untouched() {
        let before = baseline();

        // First two fields are fine; the window is inverted.
        let result = apply_update(&before, 250, 20, 5_000, 1_000, 0);

        assert_eq!(result, Err(ConfigError::InvalidTimeRange));
        assert_eq!(before, baseline(), "the prior config was mutated");
    }

    #[test]
    fn each_field_reports_its_own_error() {
        let before = baseline();
        assert_eq!(
            apply_update(&before, 10_001, 20, 1_000, 5_000, 0),
            Err(ConfigError::BasisPointsOutOfRange)
        );
        assert_eq!(
            apply_update(&before, 250, 0, 1_000, 5_000, 0),
            Err(ConfigError::LimitOutOfRange)
        );
        assert_eq!(
            apply_update(&before, 250, 20, 1_000, 5_000, 9_000),
            Err(ConfigError::WindowAlreadyClosed)
        );
    }

    // ── Property tests ─────────────────────────────────────────────────────

    /// xorshift64*, so every generated case replays from its seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    #[test]
    fn a_validated_window_always_satisfies_every_rule_it_was_checked_against() {
        for seed in [2u64, 23, 233, 2_333] {
            let mut rng = Rng(seed);
            for _ in 0..2_000u32 {
                let not_before = rng.next() % 100_000;
                let expires_at = rng.next() % 100_000;
                let now = rng.next() % 100_000;

                match validate_time_range(not_before, expires_at, now, MAX_WINDOW) {
                    Ok(range) => {
                        let r = range.get();
                        assert!(r.not_before < r.expires_at);
                        assert!(r.expires_at > now);
                        assert!(r.expires_at - r.not_before <= MAX_WINDOW);
                    }
                    Err(e) => {
                        // Every rejection is explained by exactly one rule.
                        let inverted = not_before >= expires_at;
                        let closed = !inverted && expires_at <= now;
                        let too_long =
                            !inverted && !closed && expires_at - not_before > MAX_WINDOW;
                        assert!(
                            inverted || closed || too_long,
                            "seed {seed}: rejected a window that breaks no rule ({e:?})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn validators_are_total_and_never_panic_on_extremes() {
        for seed in [5u64, 55, 555] {
            let mut rng = Rng(seed);
            for _ in 0..2_000u32 {
                let a = rng.next() as u32;
                let b = rng.next() as u32;
                let c = rng.next() as u32;

                // A result either way is fine; a panic is not.
                let _ = validate_basis_points(a);
                let _ = validate_limit(a, b.min(c), b.max(c));
                let _ = validate_interface_version(a, b, c, a);
                let _ = validate_time_range(u64::from(a), u64::from(b), u64::from(c), MAX_WINDOW);
            }
        }
    }

    #[test]
    fn a_limit_band_is_respected_for_every_generated_triple() {
        for seed in [9u64, 99] {
            let mut rng = Rng(seed);
            for _ in 0..2_000u32 {
                let value = rng.next() as u32;
                let x = rng.next() as u32;
                let y = rng.next() as u32;
                let (min, max) = (x.min(y), x.max(y));

                match validate_limit(value, min, max) {
                    Ok(v) => {
                        let v = v.into_inner();
                        assert!(v >= min && v <= max);
                    }
                    Err(e) => {
                        assert_eq!(e, ConfigError::LimitOutOfRange);
                        assert!(value < min || value > max);
                    }
                }
            }
        }
    }
}
