use soroban_sdk::{contractclient, Address, Env, Symbol};

/// Versioned external interface for authoritative course enrollment.
/// Implementations must return false for unknown or inactive enrollments.
#[contractclient(name = "CourseRegistryClient")]
pub trait CourseRegistryInterface {
    fn is_enrolled(env: Env, student: Address, course_id: Symbol) -> bool;
}
