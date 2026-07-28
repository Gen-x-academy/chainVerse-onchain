//! Fix for #874: reserve the claim (checks-effects) before the external
//! token transfer (interaction), so duplicate/reentrant claims can't pay
//! twice regardless of rollback semantics.
use std::collections::HashSet;

pub struct RewardLedger {
    reserved: HashSet<u64>,
    paid: HashSet<u64>,
}
impl RewardLedger {
    pub fn new() -> Self {
        Self { reserved: HashSet::new(), paid: HashSet::new() }
    }

    /// Effects happen first: reserving fails fast on any repeat call,
    /// before we ever touch the external transfer.
    pub fn reserve(&mut self, user_id: u64) -> Result<(), &'static str> {
        if self.paid.contains(&user_id) || !self.reserved.insert(user_id) {
            return Err("reward already reserved or paid");
        }
        Ok(())
    }

    /// Interaction happens only after reservation succeeded.
    pub fn confirm_paid(&mut self, user_id: u64) {
        self.paid.insert(user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_claim_cannot_reserve_twice() {
        let mut ledger = RewardLedger::new();
        ledger.reserve(1).unwrap();
        assert!(ledger.reserve(1).is_err());
    }

    #[test]
    fn paid_reward_cannot_be_reserved_again() {
        let mut ledger = RewardLedger::new();
        ledger.reserve(1).unwrap();
        ledger.confirm_paid(1);
        assert!(ledger.reserve(1).is_err());
    }
}
