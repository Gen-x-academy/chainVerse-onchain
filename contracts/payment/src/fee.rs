use crate::errors::ContractError;

pub fn calculate_fee(amount: i128, fee_percent: u32) -> Result<i128, ContractError> {
    if fee_percent > 2000 {
        return Err(ContractError::InvalidFee);
    }
    Ok((amount * fee_percent as i128) / 10000)
}

pub fn calculate_instructor_amount(amount: i128, fee_percent: u32) -> Result<i128, ContractError> {
    let fee = calculate_fee(amount, fee_percent)?;
    Ok(amount - fee)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_fee() {
        let fee = calculate_fee(1000, 500).unwrap();
        assert_eq!(fee, 50);

        let fee = calculate_fee(10000, 500).unwrap();
        assert_eq!(fee, 500);
    }

    #[test]
    fn test_calculate_instructor_amount() {
        let instructor_amount = calculate_instructor_amount(1000, 500).unwrap();
        assert_eq!(instructor_amount, 950);
    }

    #[test]
    fn test_fee_above_2000bps_fails() {
        let result = calculate_fee(1000, 2001);
        assert!(result.is_err());
    }
}
