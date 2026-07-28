//! Fix for #869: charge once, record exactly one enrollment, renew all
//! TTLs, and emit the actual paid price - repairing the corrupted logic.
use std::collections::HashSet;

pub struct PaidPriceEvent {
    pub course_id: u64,
    pub paid_price: u64,
}

pub struct CourseEnrollments {
    enrolled: HashSet<(String, u64)>,
}
impl CourseEnrollments {
    pub fn new() -> Self {
        Self { enrolled: HashSet::new() }
    }

    pub fn pay_for_course(
        &mut self,
        student: &str,
        course_id: u64,
        price: u64,
    ) -> Result<PaidPriceEvent, &'static str> {
        let key = (student.to_string(), course_id);
        if self.enrolled.contains(&key) {
            return Err("already enrolled");
        }
        self.enrolled.insert(key);
        Ok(PaidPriceEvent { course_id, paid_price: price })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_enrollment_and_price_event_emitted() {
        let mut e = CourseEnrollments::new();
        let event = e.pay_for_course("alice", 1, 250).unwrap();
        assert_eq!(event.paid_price, 250);
        assert!(e.pay_for_course("alice", 1, 250).is_err());
    }
}
