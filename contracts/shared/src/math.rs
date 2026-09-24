//! Checked arithmetic for seats, debt, and treasury balances (#1009).
//!
//! Every contract in the workspace that moves a seat count or a financial
//! amount goes through these helpers rather than bare `+`/`-`/`*`/`/`. The
//! point is not that `overflow-checks = true` is set in the release profile —
//! it is that an overflow should surface as a *typed contract error the client
//! can handle*, not as a host panic that looks identical to a bug.
//!
//! ## What is centralized here
//!
//! * [`MathError`] — one typed error set, so `Overflow`, `Underflow`, and
//!   `DivideByZero` mean the same thing in every contract.
//! * Checked `u32`/`u64`/`i128` add, sub, and mul.
//! * [`mul_div`] — the multiply-then-divide primitive every fee, share, and
//!   pro-rata calculation needs, with an explicit [`Rounding`] mode so the
//!   direction of the rounding error is a decision rather than an accident.
//! * [`DebtAccount`] — the deposit / charge / payment / waiver / withdrawal
//!   state machine, which is where "never create a negative liability" is
//!   actually enforced.
//!
//! ## Rounding rules
//!
//! Rounding is never implicit. [`mul_div`] requires a [`Rounding`] mode, and
//! the convention across the library domain is:
//!
//! * amounts a patron **owes** round [`Rounding::Up`] — never under-bill;
//! * amounts a patron **receives** round [`Rounding::Down`] — never over-pay;
//! * anything else states its choice at the call site.
//!
//! ## Impact
//!
//! Additive library code in the `shared` crate. No ABI, storage, event,
//! privacy, deployment, or migration impact: no contract entrypoint changes and
//! no storage key is introduced. `MathError` occupies its own discriminant
//! space and is not merged into [`crate::ContractError`].

use soroban_sdk::contracterror;

/// Typed arithmetic failures.
///
/// Discriminants start at 200 so they cannot collide with a contract's own
/// error enum when both are surfaced to a client.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MathError {
    /// The result is larger than the type can represent.
    Overflow = 200,
    /// The result is smaller than the type can represent, or a subtraction
    /// would take an unsigned quantity below zero.
    Underflow = 201,
    /// A division or `mul_div` was given a zero denominator.
    DivideByZero = 202,
    /// The operation would leave a liability or seat count negative.
    NegativeResult = 203,
    /// A financial input was negative where only non-negative values are
    /// meaningful.
    NegativeAmount = 204,
}

/// Which way an inexact division is resolved.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Rounding {
    /// Truncate toward zero.
    Down,
    /// Round away from zero whenever there is a remainder.
    Up,
}

// ── Checked primitives ─────────────────────────────────────────────────────

/// Checked `u32` addition, e.g. incrementing an allocated-seat count.
pub fn add_u32(a: u32, b: u32) -> Result<u32, MathError> {
    a.checked_add(b).ok_or(MathError::Overflow)
}

/// Checked `u32` subtraction. Returns [`MathError::Underflow`] rather than
/// wrapping, so a seat count can never wrap to `u32::MAX`.
pub fn sub_u32(a: u32, b: u32) -> Result<u32, MathError> {
    a.checked_sub(b).ok_or(MathError::Underflow)
}

/// Checked `u64` addition, e.g. advancing a timestamp or a counter.
pub fn add_u64(a: u64, b: u64) -> Result<u64, MathError> {
    a.checked_add(b).ok_or(MathError::Overflow)
}

/// Checked `u64` subtraction.
pub fn sub_u64(a: u64, b: u64) -> Result<u64, MathError> {
    a.checked_sub(b).ok_or(MathError::Underflow)
}

/// Checked `i128` addition, the width Soroban token balances use.
pub fn add_i128(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_add(b).ok_or(MathError::Overflow)
}

/// Checked `i128` subtraction.
pub fn sub_i128(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_sub(b).ok_or(MathError::Underflow)
}

/// Checked `i128` multiplication.
pub fn mul_i128(a: i128, b: i128) -> Result<i128, MathError> {
    a.checked_mul(b).ok_or(MathError::Overflow)
}

/// Checked `i128` division, truncating toward zero.
pub fn div_i128(a: i128, b: i128) -> Result<i128, MathError> {
    if b == 0 {
        return Err(MathError::DivideByZero);
    }
    a.checked_div(b).ok_or(MathError::Overflow)
}

/// Rejects a negative amount where only non-negative values are meaningful.
pub fn require_non_negative(amount: i128) -> Result<i128, MathError> {
    if amount < 0 {
        return Err(MathError::NegativeAmount);
    }
    Ok(amount)
}

/// `a * b / denominator`, computed without losing the intermediate product
/// where it would fit, with the rounding direction stated explicitly.
///
/// This is the primitive behind every fee, revenue split, and pro-rata refund.
/// Computing `a * b` first and dividing second keeps the precision that
/// `(a / denominator) * b` would throw away.
///
/// Both `a` and `b` must be non-negative; `denominator` must be strictly
/// positive.
pub fn mul_div(a: i128, b: i128, denominator: i128, rounding: Rounding) -> Result<i128, MathError> {
    require_non_negative(a)?;
    require_non_negative(b)?;
    if denominator <= 0 {
        return Err(if denominator == 0 {
            MathError::DivideByZero
        } else {
            MathError::NegativeAmount
        });
    }

    let product = mul_i128(a, b)?;
    let quotient = product / denominator;

    match rounding {
        Rounding::Down => Ok(quotient),
        Rounding::Up => {
            if product % denominator == 0 {
                Ok(quotient)
            } else {
                add_i128(quotient, 1)
            }
        }
    }
}

/// Basis points of `amount` (1 bp = 1/10,000), e.g. a 250 bp late fee.
pub fn basis_points(amount: i128, bps: i128, rounding: Rounding) -> Result<i128, MathError> {
    mul_div(amount, bps, 10_000, rounding)
}

// ── Debt accounting ────────────────────────────────────────────────────────

/// A patron's financial position with the library.
///
/// `deposit` is money the patron has placed with the library and can withdraw;
/// `outstanding` is what they owe. Neither can go negative, and every operation
/// is total: it either applies fully or returns a typed error leaving the
/// account untouched.
///
/// The conservation identity the property tests prove is:
///
/// ```text
/// outstanding == charged - paid - waived
/// deposit     == deposited - withdrawn - paid_from_deposit
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct DebtAccount {
    /// Refundable balance the patron has on account. Never negative.
    pub deposit: i128,
    /// Fines and charges not yet settled. Never negative.
    pub outstanding: i128,
}

impl DebtAccount {
    /// A new account with nothing deposited and nothing owed.
    pub fn new() -> Self {
        Self {
            deposit: 0,
            outstanding: 0,
        }
    }

    /// The patron adds refundable funds.
    pub fn deposit(&mut self, amount: i128) -> Result<(), MathError> {
        require_non_negative(amount)?;
        self.deposit = add_i128(self.deposit, amount)?;
        Ok(())
    }

    /// The library assesses a fine or fee.
    pub fn charge(&mut self, amount: i128) -> Result<(), MathError> {
        require_non_negative(amount)?;
        self.outstanding = add_i128(self.outstanding, amount)?;
        Ok(())
    }

    /// The patron settles part or all of what they owe with outside funds.
    ///
    /// Paying more than is owed is rejected rather than silently creating a
    /// credit — an over-payment is a caller bug, and turning it into a negative
    /// liability is exactly the failure this module exists to prevent.
    pub fn pay(&mut self, amount: i128) -> Result<(), MathError> {
        require_non_negative(amount)?;
        if amount > self.outstanding {
            return Err(MathError::NegativeResult);
        }
        self.outstanding = sub_i128(self.outstanding, amount)?;
        Ok(())
    }

    /// The library forgives part or all of what is owed.
    pub fn waive(&mut self, amount: i128) -> Result<(), MathError> {
        require_non_negative(amount)?;
        if amount > self.outstanding {
            return Err(MathError::NegativeResult);
        }
        self.outstanding = sub_i128(self.outstanding, amount)?;
        Ok(())
    }

    /// Settle what is owed out of the patron's deposit.
    ///
    /// Applies `min(amount, outstanding, deposit)` is **not** what happens —
    /// the call is all-or-nothing, so a caller cannot half-apply a settlement
    /// and lose track of the remainder. Returns the amount moved.
    pub fn pay_from_deposit(&mut self, amount: i128) -> Result<i128, MathError> {
        require_non_negative(amount)?;
        if amount > self.outstanding || amount > self.deposit {
            return Err(MathError::NegativeResult);
        }
        self.deposit = sub_i128(self.deposit, amount)?;
        self.outstanding = sub_i128(self.outstanding, amount)?;
        Ok(amount)
    }

    /// The patron withdraws refundable funds.
    ///
    /// Withdrawal is blocked while anything is outstanding, so a patron cannot
    /// drain their deposit and leave the library holding the debt.
    pub fn withdraw(&mut self, amount: i128) -> Result<(), MathError> {
        require_non_negative(amount)?;
        if self.outstanding > 0 {
            return Err(MathError::NegativeResult);
        }
        if amount > self.deposit {
            return Err(MathError::NegativeResult);
        }
        self.deposit = sub_i128(self.deposit, amount)?;
        Ok(())
    }

    /// True when both balances are non-negative. Always true if the type is
    /// only mutated through the methods above; asserted after every step of
    /// the property tests.
    pub fn is_well_formed(&self) -> bool {
        self.deposit >= 0 && self.outstanding >= 0
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Checked primitives: extrema ────────────────────────────────────────

    #[test]
    fn add_u32_saturating_at_the_boundary_is_an_overflow_not_a_wrap() {
        assert_eq!(add_u32(u32::MAX - 1, 1), Ok(u32::MAX));
        assert_eq!(add_u32(u32::MAX, 1), Err(MathError::Overflow));
        assert_eq!(add_u32(u32::MAX, u32::MAX), Err(MathError::Overflow));
    }

    #[test]
    fn sub_u32_below_zero_is_an_underflow_not_a_wrap() {
        assert_eq!(sub_u32(1, 1), Ok(0));
        assert_eq!(sub_u32(0, 1), Err(MathError::Underflow));
        assert_eq!(sub_u32(0, u32::MAX), Err(MathError::Underflow));
    }

    #[test]
    fn u64_helpers_hold_at_their_extrema() {
        assert_eq!(add_u64(u64::MAX - 1, 1), Ok(u64::MAX));
        assert_eq!(add_u64(u64::MAX, 1), Err(MathError::Overflow));
        assert_eq!(sub_u64(0, 1), Err(MathError::Underflow));
    }

    #[test]
    fn i128_helpers_hold_at_both_extrema() {
        assert_eq!(add_i128(i128::MAX, 1), Err(MathError::Overflow));
        assert_eq!(add_i128(i128::MIN, -1), Err(MathError::Overflow));
        assert_eq!(sub_i128(i128::MIN, 1), Err(MathError::Underflow));
        assert_eq!(mul_i128(i128::MAX, 2), Err(MathError::Overflow));
        assert_eq!(mul_i128(i128::MIN, -1), Err(MathError::Overflow));
    }

    #[test]
    fn division_by_zero_is_typed_not_a_panic() {
        assert_eq!(div_i128(1, 0), Err(MathError::DivideByZero));
        assert_eq!(div_i128(0, 0), Err(MathError::DivideByZero));
        // The one division that overflows in two's complement.
        assert_eq!(div_i128(i128::MIN, -1), Err(MathError::Overflow));
    }

    // ── mul_div and rounding ───────────────────────────────────────────────

    #[test]
    fn mul_div_keeps_precision_that_divide_first_would_lose() {
        // (1 / 3) * 3 == 0 when divided first; mul_div gets it right.
        assert_eq!(mul_div(1, 3, 3, Rounding::Down), Ok(1));
        assert_eq!(mul_div(7, 3, 10, Rounding::Down), Ok(2)); // 21/10
        assert_eq!(mul_div(7, 3, 10, Rounding::Up), Ok(3));
    }

    #[test]
    fn mul_div_rounding_is_exact_when_there_is_no_remainder() {
        assert_eq!(mul_div(10, 5, 5, Rounding::Down), Ok(10));
        assert_eq!(mul_div(10, 5, 5, Rounding::Up), Ok(10));
    }

    #[test]
    fn mul_div_rejects_a_zero_or_negative_denominator() {
        assert_eq!(mul_div(1, 1, 0, Rounding::Down), Err(MathError::DivideByZero));
        assert_eq!(
            mul_div(1, 1, -1, Rounding::Down),
            Err(MathError::NegativeAmount)
        );
    }

    #[test]
    fn mul_div_rejects_negative_operands() {
        assert_eq!(
            mul_div(-1, 1, 10, Rounding::Down),
            Err(MathError::NegativeAmount)
        );
        assert_eq!(
            mul_div(1, -1, 10, Rounding::Down),
            Err(MathError::NegativeAmount)
        );
    }

    #[test]
    fn mul_div_overflow_in_the_intermediate_product_is_reported() {
        assert_eq!(
            mul_div(i128::MAX, 2, 1, Rounding::Down),
            Err(MathError::Overflow)
        );
    }

    #[test]
    fn basis_points_rounds_in_the_stated_direction() {
        // 250 bp of 1,001 is 25.025.
        assert_eq!(basis_points(1_001, 250, Rounding::Down), Ok(25));
        assert_eq!(basis_points(1_001, 250, Rounding::Up), Ok(26));
        // A zero rate costs nothing in either direction.
        assert_eq!(basis_points(1_001, 0, Rounding::Down), Ok(0));
        assert_eq!(basis_points(1_001, 0, Rounding::Up), Ok(0));
    }

    // ── DebtAccount: named properties ──────────────────────────────────────

    #[test]
    fn paying_more_than_is_owed_is_refused_rather_than_creating_a_credit() {
        let mut account = DebtAccount::new();
        account.charge(100).unwrap();

        assert_eq!(account.pay(101), Err(MathError::NegativeResult));
        assert_eq!(account.outstanding, 100, "the refused payment mutated state");

        account.pay(100).unwrap();
        assert_eq!(account.outstanding, 0);
        assert_eq!(account.pay(1), Err(MathError::NegativeResult));
    }

    #[test]
    fn waiving_more_than_is_owed_is_refused() {
        let mut account = DebtAccount::new();
        account.charge(50).unwrap();

        assert_eq!(account.waive(51), Err(MathError::NegativeResult));
        assert_eq!(account.outstanding, 50);

        account.waive(50).unwrap();
        assert_eq!(account.outstanding, 0);
    }

    #[test]
    fn a_patron_cannot_withdraw_while_anything_is_outstanding() {
        let mut account = DebtAccount::new();
        account.deposit(100).unwrap();
        account.charge(10).unwrap();

        assert_eq!(account.withdraw(1), Err(MathError::NegativeResult));
        assert_eq!(account.deposit, 100);

        account.pay(10).unwrap();
        account.withdraw(100).unwrap();
        assert_eq!(account.deposit, 0);
    }

    #[test]
    fn withdrawing_more_than_is_on_deposit_is_refused() {
        let mut account = DebtAccount::new();
        account.deposit(10).unwrap();

        assert_eq!(account.withdraw(11), Err(MathError::NegativeResult));
        assert_eq!(account.deposit, 10);
    }

    #[test]
    fn settling_from_deposit_is_all_or_nothing() {
        let mut account = DebtAccount::new();
        account.deposit(30).unwrap();
        account.charge(50).unwrap();

        // More than the deposit covers: refused outright, nothing half-applied.
        assert_eq!(account.pay_from_deposit(40), Err(MathError::NegativeResult));
        assert_eq!(account.deposit, 30);
        assert_eq!(account.outstanding, 50);

        assert_eq!(account.pay_from_deposit(30), Ok(30));
        assert_eq!(account.deposit, 0);
        assert_eq!(account.outstanding, 20);
    }

    #[test]
    fn negative_amounts_are_refused_on_every_operation() {
        let mut account = DebtAccount::new();
        assert_eq!(account.deposit(-1), Err(MathError::NegativeAmount));
        assert_eq!(account.charge(-1), Err(MathError::NegativeAmount));
        assert_eq!(account.pay(-1), Err(MathError::NegativeAmount));
        assert_eq!(account.waive(-1), Err(MathError::NegativeAmount));
        assert_eq!(account.withdraw(-1), Err(MathError::NegativeAmount));
        assert_eq!(account.pay_from_deposit(-1), Err(MathError::NegativeAmount));
        assert_eq!(account, DebtAccount::new());
    }

    #[test]
    fn charging_past_i128_max_overflows_rather_than_wrapping_negative() {
        let mut account = DebtAccount::new();
        account.charge(i128::MAX).unwrap();
        assert_eq!(account.charge(1), Err(MathError::Overflow));
        assert_eq!(account.outstanding, i128::MAX);
        assert!(account.is_well_formed());
    }

    // ── Property tests ─────────────────────────────────────────────────────

    /// xorshift64*, so every generated sequence replays from its seed.
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

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        /// Biased toward zero, small values, and very large ones, which is
        /// where the interesting failures live.
        ///
        /// Capped at `i128::MAX / 512` so that the independent ledger the
        /// property test keeps cannot itself overflow across a 400-step
        /// sequence. The true extrema (`i128::MAX`) are covered by the named
        /// tests above, where the accumulation problem does not arise.
        fn amount(&mut self) -> i128 {
            const HUGE: i128 = i128::MAX / 512;
            match self.below(10) {
                0 => 0,
                1 => HUGE,
                2 => HUGE - 1,
                3..=6 => i128::from(self.below(1_000)),
                _ => i128::from(self.below(u64::MAX / 2)),
            }
        }
    }

    #[test]
    fn debt_account_never_goes_negative_under_random_operation_sequences() {
        for seed in [1u64, 5, 17, 101, 7_919, 1_000_003] {
            let mut rng = Rng(seed);
            let mut account = DebtAccount::new();

            // Independent ledger of what was accepted, used to prove the
            // conservation identity rather than re-deriving it from the fields
            // the operations themselves wrote.
            let mut deposited: i128 = 0;
            let mut withdrawn: i128 = 0;
            let mut charged: i128 = 0;
            let mut paid: i128 = 0;
            let mut waived: i128 = 0;
            let mut paid_from_deposit: i128 = 0;

            for step in 0..400u32 {
                let amount = rng.amount();
                let before = account;

                let accepted = match rng.below(6) {
                    0 => account.deposit(amount).map(|()| {
                        deposited += amount;
                    }),
                    1 => account.charge(amount).map(|()| {
                        charged += amount;
                    }),
                    2 => account.pay(amount).map(|()| {
                        paid += amount;
                    }),
                    3 => account.waive(amount).map(|()| {
                        waived += amount;
                    }),
                    4 => account.withdraw(amount).map(|()| {
                        withdrawn += amount;
                    }),
                    _ => account.pay_from_deposit(amount).map(|moved| {
                        paid_from_deposit += moved;
                    }),
                };

                if accepted.is_err() {
                    assert_eq!(
                        account, before,
                        "seed {seed} step {step}: a rejected operation mutated the account"
                    );
                }

                assert!(
                    account.is_well_formed(),
                    "seed {seed} step {step}: {account:?} has a negative balance"
                );
                assert_eq!(
                    account.outstanding,
                    charged - paid - waived - paid_from_deposit,
                    "seed {seed} step {step}: outstanding does not reconcile"
                );
                assert_eq!(
                    account.deposit,
                    deposited - withdrawn - paid_from_deposit,
                    "seed {seed} step {step}: deposit does not reconcile"
                );
            }
        }
    }

    #[test]
    fn mul_div_up_is_never_more_than_one_above_mul_div_down() {
        for seed in [3u64, 29, 4_099] {
            let mut rng = Rng(seed);
            for _ in 0..500u32 {
                let a = i128::from(rng.below(1_000_000));
                let b = i128::from(rng.below(1_000_000));
                let d = i128::from(rng.below(1_000) + 1);

                let down = mul_div(a, b, d, Rounding::Down).unwrap();
                let up = mul_div(a, b, d, Rounding::Up).unwrap();

                assert!(up == down || up == down + 1, "{up} is not within 1 of {down}");
                // Down never over-states and Up never under-states the exact value.
                assert!(down * d <= a * b);
                assert!(up * d >= a * b);
            }
        }
    }

    #[test]
    fn checked_helpers_agree_with_the_primitive_ops_whenever_they_do_not_trap() {
        for seed in [11u64, 13, 17] {
            let mut rng = Rng(seed);
            for _ in 0..1_000u32 {
                let a = rng.below(u64::MAX) as u32;
                let b = rng.below(u64::MAX) as u32;

                match add_u32(a, b) {
                    Ok(sum) => assert_eq!(sum, a.wrapping_add(b)),
                    Err(e) => {
                        assert_eq!(e, MathError::Overflow);
                        assert!(a.checked_add(b).is_none());
                    }
                }
                match sub_u32(a, b) {
                    Ok(diff) => assert_eq!(diff, a - b),
                    Err(e) => {
                        assert_eq!(e, MathError::Underflow);
                        assert!(a < b);
                    }
                }
            }
        }
    }
}
