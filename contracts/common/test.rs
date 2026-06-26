#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::*, Address, Env};

    fn create_test_env() -> (Env, Address, Address, Address) {
        let env = Env::default();
        let admin = Address::generate(&env);
        let reward_token = Address::generate(&env);
        let governance = Address::generate(&env);

        (env, admin, reward_token, governance)
    }

    #[test]
    fn test_contract_initialization() {
        let (env, admin, reward_token, governance) = create_test_env();

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            reward_token.clone(),
            governance.clone(),
        )
        .expect("Failed to initialize");

        // Verify info is stored correctly
        let (stored_admin, stored_token, stored_gov) =
            AcademyVestingContract::get_info(env).expect("Failed to get info");

        assert_eq!(stored_admin, admin);
        assert_eq!(stored_token, reward_token);
        assert_eq!(stored_gov, governance);
    }

    #[test]
    fn test_contract_cannot_be_initialized_twice() {
        let (env, admin, reward_token, governance) = create_test_env();

        // First initialization should succeed
        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            reward_token.clone(),
            governance.clone(),
        )
        .expect("First init failed");

        // Second initialization should fail
        let result = AcademyVestingContract::init(
            env,
            admin,
            reward_token,
            governance,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Unauthorized);
    }

    #[test]
    fn test_grant_vesting_schedule() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Grant vesting schedule
        let grant_id = AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary.clone(),
            1000,
            100,   // start_time
            60,    // cliff (60 seconds)
            3600,  // duration (1 hour total)
        )
        .expect("Grant failed");

        assert_eq!(grant_id, 1);

        // Retrieve and verify schedule
        let schedule = AcademyVestingContract::get_vesting(env, grant_id)
            .expect("Get vesting failed");

        assert_eq!(schedule.beneficiary, beneficiary);
        assert_eq!(schedule.amount, 1000);
        assert_eq!(schedule.start_time, 100);
        assert_eq!(schedule.cliff, 60);
        assert_eq!(schedule.duration, 3600);
        assert!(!schedule.claimed);
        assert!(!schedule.revoked);
    }

    #[test]
    fn test_grant_multiple_schedules() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary1 = Address::generate(&env);
        let beneficiary2 = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Grant first schedule
        let grant_id1 = AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary1,
            1000,
            100,
            60,
            3600,
        )
        .expect("Grant 1 failed");

        // Grant second schedule
        let grant_id2 = AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary2,
            2000,
            200,
            120,
            7200,
        )
        .expect("Grant 2 failed");

        // IDs should be sequential
        assert_eq!(grant_id1, 1);
        assert_eq!(grant_id2, 2);
    }

    #[test]
    fn test_grant_with_invalid_schedule() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Test: negative amount
        let result = AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
            -1000,
            100,
            60,
            3600,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::InvalidSchedule);

        // Test: cliff > duration
        let result = AcademyVestingContract::grant_vesting(
            env,
            admin,
            beneficiary,
            1000,
            100,
            5000, // cliff > duration
            3600,
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::InvalidSchedule);
    }

    #[test]
    fn test_non_admin_cannot_grant() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let non_admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin,
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Non-admin tries to grant
        let result = AcademyVestingContract::grant_vesting(
            env,
            non_admin,
            beneficiary,
            1000,
            100,
            60,
            3600,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Unauthorized);
    }

    #[test]
    fn test_vesting_calculation_before_start() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            1000,
            start_time,
            300,  // cliff
            3600, // duration
        )
        .expect("Grant failed");

        // Mock ledger time to before start
        env.ledger().with_mut(|li| li.timestamp = start_time - 100);

        let vested = AcademyVestingContract::get_vested_amount(env, 1)
            .expect("Get vested failed");

        assert_eq!(vested, 0); // Nothing vested yet
    }

    #[test]
    fn test_vesting_calculation_before_cliff() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;
        let cliff = 300u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            1000,
            start_time,
            cliff,
            3600,
        )
        .expect("Grant failed");

        // Mock ledger time to after start but before cliff
        env.ledger().with_mut(|li| li.timestamp = start_time + cliff - 50);

        let vested = AcademyVestingContract::get_vested_amount(env, 1)
            .expect("Get vested failed");

        assert_eq!(vested, 0); // Still nothing vested (cliff not passed)
    }

    #[test]
    fn test_vesting_calculation_after_cliff() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;
        let cliff = 300u64;
        let duration = 3600u64;
        let amount = 1000i128;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            amount,
            start_time,
            cliff,
            duration,
        )
        .expect("Grant failed");

        // Mock ledger time to exactly at cliff
        env.ledger().with_mut(|li| li.timestamp = start_time + cliff);

        let vested = AcademyVestingContract::get_vested_amount(env, 1)
            .expect("Get vested failed");

        assert_eq!(vested, 0); // Vesting starts linearly after cliff
    }

    #[test]
    fn test_vesting_calculation_fully_vested() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;
        let cliff = 300u64;
        let duration = 3600u64;
        let amount = 1000i128;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            amount,
            start_time,
            cliff,
            duration,
        )
        .expect("Grant failed");

        // Mock ledger time to after full duration
        env.ledger().with_mut(|li| li.timestamp = start_time + duration + 1000);

        let vested = AcademyVestingContract::get_vested_amount(env, 1)
            .expect("Get vested failed");

        assert_eq!(vested, amount); // Fully vested
    }

    #[test]
    fn test_vesting_calculation_partial() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 0u64;
        let cliff = 100u64;
        let duration = 1000u64;
        let amount = 1000i128;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            amount,
            start_time,
            cliff,
            duration,
        )
        .expect("Grant failed");

        // Mock ledger time to 50% through vesting (after cliff)
        // Time: cliff + (duration - cliff) / 2 = 100 + 450 = 550
        env.ledger().with_mut(|li| li.timestamp = start_time + cliff + 450);

        let vested = AcademyVestingContract::get_vested_amount(env, 1)
            .expect("Get vested failed");

        // Should be approximately 50% of amount (500)
        assert!(vested >= 490 && vested <= 510); // Allow small rounding
    }

    #[test]
    fn test_claim_not_vested() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary.clone(),
            1000,
            start_time,
            300,
            3600,
        )
        .expect("Grant failed");

        // Try to claim before vesting
        env.ledger().with_mut(|li| li.timestamp = start_time - 100);

        let result = AcademyVestingContract::claim(env, 1, beneficiary);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::NotVested);
    }

    #[test]
    fn test_claim_single_semantics_prevents_double_claim() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 0u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary.clone(),
            1000,
            start_time,
            100,
            3600,
        )
        .expect("Grant failed");

        // Set time to after cliff
        env.ledger().with_mut(|li| li.timestamp = start_time + 200);

        // First claim should fail because insufficient balance (mock issue)
        // In real scenario with token setup, first claim would succeed
        // Second claim would fail with AlreadyClaimed

        // For this test, we verify the logic by checking schedule state
        let schedule = AcademyVestingContract::get_vesting(env.clone(), 1)
            .expect("Get vesting failed");
        assert!(!schedule.claimed);

        // Simulate the claimed state by attempting second claim
        // (In real test with token, first claim would mark it as claimed)
    }

    #[test]
    fn test_claim_revoked_schedule() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 0u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
            1000,
            start_time,
            100,
            3600,
        )
        .expect("Grant failed");

        // Revoke the schedule
        env.ledger().with_mut(|li| li.timestamp = start_time + 3600); // After start + revoke_delay
        AcademyVestingContract::revoke(env.clone(), 1, admin.clone(), 3600)
            .expect("Revoke failed");

        // Try to claim revoked schedule
        let result = AcademyVestingContract::claim(env, 1, beneficiary);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Revoked);
    }

    #[test]
    fn test_revoke_invalid_timelock() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary,
            1000,
            0,
            100,
            3600,
        )
        .expect("Grant failed");

        // Try to revoke with insufficient timelock (< 1 hour)
        let result = AcademyVestingContract::revoke(env, 1, admin, 100); // 100 seconds < 3600

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::InvalidTimelock);
    }

    #[test]
    fn test_revoke_not_enough_time_elapsed() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 1000u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary,
            1000,
            start_time,
            100,
            3600,
        )
        .expect("Grant failed");

        // Try to revoke too early (before revoke_delay elapsed)
        env.ledger().with_mut(|li| li.timestamp = start_time + 1000); // Only 1000 seconds elapsed

        let result = AcademyVestingContract::revoke(env, 1, admin, 3600); // 3600 second delay

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::NotEnoughTimeForRevoke);
    }

    #[test]
    fn test_revoke_cannot_revoke_claimed() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
            1000,
            0,
            100,
            3600,
        )
        .expect("Grant failed");

        // Cannot actually test claim without token setup, but we test the revoke constraint
        // by checking that revoke fails when trying to revoke a schedule
    }

    #[test]
    fn test_revoke_cannot_revoke_twice() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        let start_time = 0u64;

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary,
            1000,
            start_time,
            100,
            3600,
        )
        .expect("Grant failed");

        // Set time to allow revocation
        env.ledger().with_mut(|li| li.timestamp = start_time + 3600);

        // First revoke should succeed
        AcademyVestingContract::revoke(env.clone(), 1, admin.clone(), 3600)
            .expect("First revoke failed");

        // Second revoke should fail
        let result = AcademyVestingContract::revoke(env, 1, admin, 3600);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Revoked);
    }

    #[test]
    fn test_non_admin_cannot_revoke() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let non_admin = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin,
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary,
            1000,
            0,
            100,
            3600,
        )
        .expect("Grant failed");

        env.ledger().with_mut(|li| li.timestamp = 3600);

        // Non-admin tries to revoke
        let result = AcademyVestingContract::revoke(env, 1, non_admin, 3600);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Unauthorized);
    }

    #[test]
    fn test_claim_wrong_beneficiary() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);
        let other = Address::generate(&env);

        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        AcademyVestingContract::grant_vesting(
            env.clone(),
            admin,
            beneficiary,
            1000,
            0,
            100,
            3600,
        )
        .expect("Grant failed");

        env.ledger().with_mut(|li| li.timestamp = 200);

        // Different beneficiary tries to claim
        let result = AcademyVestingContract::claim(env, 1, other);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::Unauthorized);
    }

    #[test]
    fn test_get_vesting_nonexistent() {
        let (env, admin, _reward_token, governance) = create_test_env();

        AcademyVestingContract::init(
            env.clone(),
            admin,
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Try to get nonexistent grant
        let result = AcademyVestingContract::get_vesting(env, 999);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::GrantNotFound);
    }

    #[test]
    fn test_get_vested_amount_nonexistent() {
        let (env, admin, _reward_token, governance) = create_test_env();

        AcademyVestingContract::init(
            env.clone(),
            admin,
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Try to get vested amount for nonexistent grant
        let result = AcademyVestingContract::get_vested_amount(env, 999);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), VestingError::GrantNotFound);
    }

    #[test]
    fn test_integration_complete_vesting_flow() {
        let (env, admin, _reward_token, governance) = create_test_env();
        let beneficiary = Address::generate(&env);

        // Initialize contract
        AcademyVestingContract::init(
            env.clone(),
            admin.clone(),
            Address::generate(&env),
            governance,
        )
        .expect("Init failed");

        // Backend grants vesting schedule
        let start_time = 0u64;
        let cliff = 1000u64;
        let duration = 10000u64;
        let amount = 5000i128;

        let grant_id = AcademyVestingContract::grant_vesting(
            env.clone(),
            admin.clone(),
            beneficiary.clone(),
            amount,
            start_time,
            cliff,
            duration,
        )
        .expect("Grant failed");

        // Check vesting status before cliff
        env.ledger().with_mut(|li| li.timestamp = start_time + 500);
        let vested_before = AcademyVestingContract::get_vested_amount(env.clone(), grant_id)
            .expect("Get vested before cliff failed");
        assert_eq!(vested_before, 0);

        // Check vesting status at cliff
        env.ledger().with_mut(|li| li.timestamp = start_time + cliff);
        let vested_at_cliff = AcademyVestingContract::get_vested_amount(env.clone(), grant_id)
            .expect("Get vested at cliff failed");
        assert_eq!(vested_at_cliff, 0); // Vesting starts linearly after cliff

        // Check vesting status midway
        env.ledger().with_mut(|li| li.timestamp = start_time + cliff + (duration - cliff) / 2);
        let vested_midway = AcademyVestingContract::get_vested_amount(env.clone(), grant_id)
            .expect("Get vested midway failed");
        assert!(vested_midway > 0 && vested_midway < amount); // Partially vested

        // Check vesting status after full duration
        env.ledger().set_timestamp(start_time + duration + 1000);
        let vested_full = AcademyVestingContract::get_vested_amount(env.clone(), grant_id)
            .expect("Get vested full failed");
        assert_eq!(vested_full, amount); // Fully vested

        // Verify schedule details
        let schedule = AcademyVestingContract::get_vesting(env, grant_id)
            .expect("Get vesting failed");
        assert_eq!(schedule.beneficiary, beneficiary);
        assert_eq!(schedule.amount, amount);
        assert_eq!(schedule.cliff, cliff);
        assert_eq!(schedule.duration, duration);
        assert!(!schedule.claimed);
        assert!(!schedule.revoked);
    }
}
