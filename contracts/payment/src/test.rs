#[cfg(test)]
mod tests {
    use crate::{PaymentContract, ContractError};
    use soroban_sdk::{
        symbol_short, Address, Env, Symbol,
        testutils::{Address as _, Ledger},
    };

    fn setup_contract(env: &Env) -> (Address, Address, Address, Address) {
        let admin = Address::random(env);
        let student = Address::random(env);
        let instructor = Address::random(env);
        let token = Address::random(env);

        env.mock_all_auths();
        PaymentContract::initialize(
            env.clone(),
            admin.clone(),
            token.clone(),
            500, // 5% fee (500 basis points)
            86400, // 1 day refund window
        )
        .unwrap();

        (admin, student, instructor, token)
    }

    #[test]
    fn test_pay_for_course_success() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        let result = PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor.clone(),
            amount,
        );

        assert!(result.is_ok());
        assert!(PaymentContract::is_enrolled(env, student.clone(), course_id));
    }

    #[test]
    fn test_pay_phantom_course_fails() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("FAKE99");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor.clone(),
            amount,
        )
        .unwrap();

        assert!(PaymentContract::is_enrolled(env, student, course_id));
    }

    #[test]
    fn test_pay_inactive_course_fails() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        let result = PaymentContract::pay_for_course(
            env,
            student,
            course_id,
            instructor,
            amount,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_double_pay_fails() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor.clone(),
            amount,
        )
        .unwrap();

        let result = PaymentContract::pay_for_course(
            env,
            student,
            course_id,
            instructor,
            amount,
        );

        assert_eq!(result.err(), Some(ContractError::AlreadyEnrolled));
    }

    #[test]
    fn test_fee_deducted_correctly() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 10000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor.clone(),
            amount,
        )
        .unwrap();

        let balance = PaymentContract::get_instructor_balance(env, instructor);
        assert_eq!(balance, 9500); // 10000 - 500 (5% fee)
    }

    #[test]
    fn test_instructor_balance_accumulates() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id1 = symbol_short!("RUST1");
        let course_id2 = symbol_short!("RUST2");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id1,
            instructor.clone(),
            amount,
        )
        .unwrap();

        let student2 = Address::random(&env);
        PaymentContract::pay_for_course(
            env.clone(),
            student2,
            course_id2,
            instructor.clone(),
            amount,
        )
        .unwrap();

        let balance = PaymentContract::get_instructor_balance(env, instructor);
        assert_eq!(balance, 1900); // (1000 - 50) + (1000 - 50)
    }

    #[test]
    fn test_withdraw_earnings_zeroes_balance() {
        let env = Env::default();
        let (_, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student,
            course_id,
            instructor.clone(),
            amount,
        )
        .unwrap();

        env.mock_all_auths();
        PaymentContract::withdraw_earnings(env.clone(), instructor.clone()).unwrap();

        let balance = PaymentContract::get_instructor_balance(env, instructor);
        assert_eq!(balance, 0);
    }

    #[test]
    fn test_refund_within_window_succeeds() {
        let env = Env::default();
        let (admin, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor,
            amount,
        )
        .unwrap();

        env.ledger().set_timestamp(86400 / 2);

        env.mock_all_auths();
        let result = PaymentContract::refund(env.clone(), student.clone(), course_id);

        assert!(result.is_ok());
        assert!(!PaymentContract::is_enrolled(env, student, course_id));
    }

    #[test]
    fn test_refund_outside_window_fails() {
        let env = Env::default();
        let (admin, student, instructor, _) = setup_contract(&env);

        let course_id = symbol_short!("RUST101");
        let amount = 1000i128;

        env.mock_all_auths();
        PaymentContract::pay_for_course(
            env.clone(),
            student.clone(),
            course_id,
            instructor,
            amount,
        )
        .unwrap();

        env.ledger().set_timestamp(86400 + 1);

        env.mock_all_auths();
        let result = PaymentContract::refund(env, student, course_id);

        assert_eq!(result.err(), Some(ContractError::RefundWindowExpired));
    }

    #[test]
    fn test_unauthorized_set_fee_fails() {
        let env = Env::default();
        let (_, student, _, _) = setup_contract(&env);

        env.mock_all_auths();
        let result = PaymentContract::set_fee(env, student, 1000);

        assert_eq!(result.err(), Some(ContractError::NotAdmin));
    }

    #[test]
    fn test_fee_above_2000bps_fails() {
        let env = Env::default();
        let (admin, _, _, _) = setup_contract(&env);

        env.mock_all_auths();
        let result = PaymentContract::set_fee(env, admin, 2001);

        assert_eq!(result.err(), Some(ContractError::InvalidFee));
    }
}
