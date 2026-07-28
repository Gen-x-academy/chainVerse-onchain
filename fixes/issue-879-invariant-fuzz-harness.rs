//! Fix for #879: a reproducible model-testing harness that exercises
//! core invariants (e.g. balances never go negative) and records seeds
//! for any failing case, instead of relying on example tests alone.
pub struct Invariant {
    pub balance: i64,
}
impl Invariant {
    pub fn apply(&mut self, delta: i64) -> Result<(), &'static str> {
        let next = self.balance.checked_add(delta).ok_or("overflow")?;
        if next < 0 {
            return Err("balance invariant violated: went negative");
        }
        self.balance = next;
        Ok(())
    }
}

/// Deterministic pseudo-fuzz over a seed: replayable, and any step the
/// invariant correctly rejects is skipped rather than treated as a bug.
/// A real bug would be `state.balance` observed negative afterward.
pub fn run_seeded_case(seed: u64) -> i64 {
    let mut state = Invariant { balance: 0 };
    let mut x = seed;
    for _ in 0..20 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        let delta = (x % 21) as i64 - 10;
        let _ = state.apply(delta);
    }
    state.balance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_seeds_never_leave_balance_negative() {
        for seed in 0..50u64 {
            let balance = run_seeded_case(seed);
            assert!(balance >= 0, "invariant violated at seed {seed}: balance {balance}");
        }
    }
}
