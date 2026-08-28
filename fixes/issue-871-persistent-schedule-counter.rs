//! Fix for #871: a schedule counter that survives upgrades plus
//! authorized cancellation, so execute-vs-cancel races have one outcome.
use std::collections::HashMap;
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScheduleState { Pending, Executed, Cancelled }
pub struct PayoutSchedules {
    pub admin: String,
    next_id: u64,
    schedules: HashMap<u64, ScheduleState>,
}
impl PayoutSchedules {
    pub fn new(admin: &str, persisted_next_id: u64) -> Self {
        Self { admin: admin.to_string(), next_id: persisted_next_id, schedules: HashMap::new() }
    }
    pub fn create(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.schedules.insert(id, ScheduleState::Pending);
        id
    }
    pub fn cancel(&mut self, caller: &str, id: u64) -> Result<(), &'static str> {
        if caller != self.admin {
            return Err("only admin can cancel");
        }
        match self.schedules.get_mut(&id) {
            Some(s) if *s == ScheduleState::Pending => { *s = ScheduleState::Cancelled; Ok(()) }
            Some(_) => Err("already executed or cancelled"),
            None => Err("unknown schedule"),
        }
    }
    pub fn execute(&mut self, id: u64) -> Result<(), &'static str> {
        match self.schedules.get_mut(&id) {
            Some(s) if *s == ScheduleState::Pending => { *s = ScheduleState::Executed; Ok(()) }
            _ => Err("schedule not pending"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_schedule_cannot_execute() {
        let mut s = PayoutSchedules::new("admin", 100);
        let id = s.create();
        assert_eq!(id, 100);
        s.cancel("admin", id).unwrap();
        assert!(s.execute(id).is_err());
    }
}
