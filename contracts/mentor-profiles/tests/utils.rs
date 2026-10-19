use soroban_sdk::{Address, Bytes, Env};

/// Helper to create a 32-byte hash for testing
pub fn make_test_hash(seed: u8) -> Bytes {
    let bytes = [seed; 32];
    Bytes::new(&Env::default(), &bytes)
}

/// Helper to create a test address
pub fn make_test_address(env: &Env, id: u8) -> Address {
    Address::generate(env)
}
