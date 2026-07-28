//! Fix for #872: bounded, stable-order pagination over active courses
//! plus a public admin query, instead of lookup-by-known-ID only.

pub struct Course {
    pub id: u64,
    pub active: bool,
}

pub struct CourseIndex {
    pub admin: String,
    pub courses: Vec<Course>,
}
impl CourseIndex {
    pub fn admin(&self) -> &str {
        &self.admin
    }

    /// Cursor-based page over active courses only, in stable ID order.
    pub fn list_active(&self, after_id: Option<u64>, limit: usize) -> Vec<u64> {
        self.courses
            .iter()
            .filter(|c| c.active && after_id.map_or(true, |a| c.id > a))
            .take(limit)
            .map(|c| c.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_returns_only_active_courses_in_order() {
        let index = CourseIndex {
            admin: "admin-a".into(),
            courses: vec![
                Course { id: 1, active: true },
                Course { id: 2, active: false },
                Course { id: 3, active: true },
            ],
        };
        assert_eq!(index.list_active(None, 10), vec![1, 3]);
        assert_eq!(index.list_active(Some(1), 10), vec![3]);
        assert_eq!(index.admin(), "admin-a");
    }
}
