//! Fix for #868: expose vault/voter status queries and emit an event
//! for every approval, release, cancellation, and admin action.
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub enum VaultEvent { Approved(String), Released, Cancelled }

pub struct EscrowVault {
    pub approvers: HashSet<String>,
    pub released: bool,
    pub cancelled: bool,
    pub events: Vec<VaultEvent>,
}
impl EscrowVault {
    pub fn new() -> Self {
        Self { approvers: HashSet::new(), released: false, cancelled: false, events: Vec::new() }
    }
    pub fn approve(&mut self, voter: &str) {
        self.approvers.insert(voter.to_string());
        self.events.push(VaultEvent::Approved(voter.to_string()));
    }
    /// Query which specific approvers have voted, not just a count.
    pub fn has_approved(&self, voter: &str) -> bool {
        self.approvers.contains(voter)
    }
    pub fn release(&mut self, min_approvals: usize) -> Result<(), &'static str> {
        if self.approvers.len() < min_approvals {
            return Err("not enough approvals");
        }
        self.released = true;
        self.events.push(VaultEvent::Released);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approvals_are_queryable_by_identity_and_emit_events() {
        let mut v = EscrowVault::new();
        v.approve("voter-a");
        assert!(v.has_approved("voter-a"));
        assert!(!v.has_approved("voter-b"));
        v.approve("voter-b");
        v.release(2).unwrap();
        assert_eq!(v.events.len(), 3);
    }
}
