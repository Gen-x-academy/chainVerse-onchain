use crate::WorkRecord;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

// -- Positive --

#[test]
fn test_work_record_holds_only_hash_and_custodian() {
    let env = Env::default();
    let custodian = Address::generate(&env);
    let hash = BytesN::from_array(&env, &[7u8; 32]);

    let record = WorkRecord {
        work_hash: hash.clone(),
        custodian: custodian.clone(),
    };

    // Destructuring against the full field set is the structural
    // guarantee here: this stops compiling the moment a third field is
    // added to `WorkRecord` without updating this test to justify it
    // against the classification doc in `types.rs`.
    let WorkRecord {
        work_hash,
        custodian: record_custodian,
    } = record;
    assert_eq!(work_hash, hash);
    assert_eq!(record_custodian, custodian);
}

// -- Boundary --

// A full end-to-end round trip through contract storage (hash + address
// only, nothing else persisted) is covered by
// `tests::storage::test_put_and_get_work_round_trips`, which exercises
// this same privacy boundary via the actual `put_work`/`get_work` ABI
// rather than duplicating it here.
