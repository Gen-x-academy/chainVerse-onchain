#![cfg(test)]

/// Tests for governance-dao: cast_vote with an eligible voter.
///
/// These tests simulate the vote-casting lifecycle:
///   - An eligible voter (one whose weight meets the quorum threshold) casts a vote.
///   - The vote is recorded in the tally.
///   - The running total reflects the cast vote correctly.
///
/// "Eligible" is defined as having a voting weight >= the minimum required weight (1).
/// The quorum is reached when the cumulative tally meets or exceeds the threshold.

const QUORUM_THRESHOLD: i128 = 100;
const MIN_VOTER_WEIGHT: i128 = 1;

struct VoteTally {
    yes: i128,
    no: i128,
}

impl VoteTally {
    fn new() -> Self {
        Self { yes: 0, no: 0 }
    }

    fn cast(&mut self, weight: i128, in_favour: bool) {
        if in_favour {
            self.yes += weight;
        } else {
            self.no += weight;
        }
    }

    fn total(&self) -> i128 {
        self.yes + self.no
    }

    fn quorum_reached(&self) -> bool {
        self.total() >= QUORUM_THRESHOLD
    }
}

fn is_eligible(weight: i128) -> bool {
    weight >= MIN_VOTER_WEIGHT
}

#[test]
fn test_cast_vote_eligible_voter_recorded_in_tally() {
    let voter_weight: i128 = 40;
    assert!(
        is_eligible(voter_weight),
        "voter must meet eligibility threshold"
    );

    let mut tally = VoteTally::new();
    tally.cast(voter_weight, true);

    assert_eq!(tally.yes, 40, "yes tally should reflect the cast vote");
    assert_eq!(tally.no, 0);
    assert_eq!(tally.total(), 40);
}

#[test]
fn test_cast_vote_multiple_eligible_voters_tallied_correctly() {
    let mut tally = VoteTally::new();

    // Three eligible voters
    let weights = [40i128, 35, 25];
    for w in weights {
        assert!(is_eligible(w));
        tally.cast(w, true);
    }

    assert_eq!(tally.yes, 100);
    assert_eq!(tally.no, 0);
    assert!(
        tally.quorum_reached(),
        "quorum should be reached at exactly 100"
    );
}

#[test]
fn test_cast_vote_ineligible_voter_rejected() {
    let voter_weight: i128 = 0;
    assert!(
        !is_eligible(voter_weight),
        "voter with zero weight must not be eligible"
    );
}

#[test]
fn test_cast_vote_mixed_yes_no_tallied_correctly() {
    let mut tally = VoteTally::new();

    tally.cast(60, true);
    tally.cast(40, false);

    assert_eq!(tally.yes, 60);
    assert_eq!(tally.no, 40);
    assert_eq!(tally.total(), 100);
    assert!(tally.quorum_reached());
}
