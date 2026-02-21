use soroban_sdk::{contracttype, Address};

#[contracttype]
pub struct Course {
    pub course_id: u32,
    pub price_xlm: i128,
    pub price_chv: i128,
}

#[contracttype]
pub enum DataKey {
    Course(u32),
    Purchase(Address, u32),
    Treasury,
    CHVToken,
}


#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Purchase(Address, u64), // (buyer, course_id)
}