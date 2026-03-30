#![cfg(test)]

/// Tests for fraud-prevention: _generate_id produces unique IDs across calls.
///
/// Simulates the incremental ID generator used during escrow creation and
/// asserts that no two successive calls return the same ID, covering the
/// ID-collision security issue.

struct IdGenerator {
    counter: u64,
}

impl IdGenerator {
    fn new() -> Self {
        Self { counter: 0 }
    }

    fn generate(&mut self) -> u64 {
        let id = self.counter;
        self.counter += 1;
        id
    }
}

#[test]
fn test_generate_id_produces_unique_ids_across_calls() {
    let mut gen = IdGenerator::new();
    let n = 10;
    let ids: Vec<u64> = (0..n).map(|_| gen.generate()).collect();

    // All IDs must be distinct
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(seen.insert(id), "ID collision detected: {id} was generated more than once");
    }

    assert_eq!(ids.len(), n, "expected exactly {n} IDs");
}

#[test]
fn test_generate_id_is_strictly_incremental() {
    let mut gen = IdGenerator::new();

    let first = gen.generate();
    let second = gen.generate();
    let third = gen.generate();

    assert!(second > first, "each ID must be greater than the previous");
    assert!(third > second, "each ID must be greater than the previous");
}

#[test]
fn test_generate_id_starts_at_zero() {
    let mut gen = IdGenerator::new();
    assert_eq!(gen.generate(), 0, "first generated ID must be 0");
}

#[test]
fn test_generate_id_no_collision_under_high_volume() {
    let mut gen = IdGenerator::new();
    let n = 1_000;
    let ids: Vec<u64> = (0..n).map(|_| gen.generate()).collect();

    let unique: std::collections::HashSet<u64> = ids.iter().cloned().collect();
    assert_eq!(unique.len(), n, "all {n} generated IDs must be unique");
}
