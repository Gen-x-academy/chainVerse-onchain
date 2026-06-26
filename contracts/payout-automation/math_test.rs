#![cfg(test)]

use soroban_sdk::Env;

#[test]
fn test_u64_overflow() {
    let env = Env::default();
    env.mock_all_auths();

    let max = u64::MAX;

    // Attempt overflow
    let result = max.checked_add(1);

    // Should NOT panic, should return None
    assert_eq!(result, None);
}

#[test]
fn test_u64_underflow() {
    let env = Env::default();
    env.mock_all_auths();

    let zero = 0u64;

    // Attempt underflow
    let result = zero.checked_sub(1);

    // Should return None instead of crashing
    assert_eq!(result, None);
}

#[test]
fn test_i128_overflow() {
    let env = Env::default();
    env.mock_all_auths();

    let max = i128::MAX;

    let result = max.checked_add(1);

    assert_eq!(result, None);
}

#[test]
fn test_i128_underflow() {
    let env = Env::default();
    env.mock_all_auths();

    let min = i128::MIN;

    let result = min.checked_sub(1);

    assert_eq!(result, None);
}
