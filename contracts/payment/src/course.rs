use soroban_sdk::{contracttype, Address, Symbol};

#[contracttype]
#[derive(Clone, Debug)]
pub struct Course {
    pub id: Symbol,
    pub instructor: Address,
    pub price: i128,
    pub is_active: bool,
}

pub fn register_course(course: Course) -> Course {
    course
}

pub fn deactivate_course(mut course: Course) -> Course {
    course.is_active = false;
    course
}
