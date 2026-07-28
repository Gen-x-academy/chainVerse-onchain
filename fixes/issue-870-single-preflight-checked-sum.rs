//! Fix for #870: one checked summation and balance preflight before any
//! transfers, instead of duplicated validation passes, so the batch
//! stays atomic.
pub struct PayoutBatch {
    pub amounts: Vec<u64>,
}
impl PayoutBatch {
    /// Single checked-sum preflight: overflow or insufficient balance
    /// aborts before any transfer is attempted.
    pub fn preflight(&self, treasury_balance: u64) -> Result<u64, &'static str> {
        let mut total: u64 = 0;
        for amount in &self.amounts {
            total = total.checked_add(*amount).ok_or("total overflow")?;
        }
        if total > treasury_balance {
            return Err("insufficient treasury balance");
        }
        Ok(total)
    }

    /// Batch executes only if preflight succeeds; no partial transfers.
    pub fn execute(&self, treasury_balance: u64) -> Result<Vec<u64>, &'static str> {
        self.preflight(treasury_balance)?;
        Ok(self.amounts.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflowing_batch_is_rejected_before_transfer() {
        let batch = PayoutBatch { amounts: vec![u64::MAX, 1] };
        assert!(batch.execute(u64::MAX).is_err());
    }

    #[test]
    fn underfunded_batch_is_rejected() {
        let batch = PayoutBatch { amounts: vec![100, 200] };
        assert!(batch.execute(250).is_err());
        assert_eq!(batch.execute(300).unwrap(), vec![100, 200]);
    }
}
