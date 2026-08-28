//! Fix for #873: two-step admin transfer plus per-course owner/editor
//! roles, so one immutable global admin no longer controls every course.
use std::collections::HashMap;

pub struct CourseRegistry {
    pub admin: String,
    pending_admin: Option<String>,
    owners: HashMap<u64, String>,
}
impl CourseRegistry {
    pub fn new(admin: &str) -> Self {
        Self { admin: admin.to_string(), pending_admin: None, owners: HashMap::new() }
    }
    pub fn propose_admin(&mut self, caller: &str, candidate: &str) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("only current admin can propose");
        }
        self.pending_admin = Some(candidate.to_string());
        Ok(())
    }
    pub fn accept_admin(&mut self, caller: &str) -> Result<(), &'static str> {
        if self.pending_admin.as_deref() != Some(caller) {
            return Err("only the proposed admin can accept");
        }
        self.admin = caller.to_string();
        self.pending_admin = None;
        Ok(())
    }
    pub fn set_course_owner(&mut self, course_id: u64, owner: &str) {
        self.owners.insert(course_id, owner.to_string());
    }
    pub fn can_edit_course(&self, course_id: u64, caller: &str) -> bool {
        caller == self.admin || self.owners.get(&course_id).map(|o| o == caller).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_transfer_requires_acceptance() {
        let mut reg = CourseRegistry::new("admin-a");
        reg.propose_admin("admin-a", "admin-b").unwrap();
        assert!(reg.accept_admin("someone-else").is_err());
        reg.accept_admin("admin-b").unwrap();
        assert_eq!(reg.admin, "admin-b");
    }
}
