use mentor_types::CompletionRecord;
use soroban_sdk::{Address, Env, String, Symbol, Vec};

pub const MIN_TTL: u32 = 1_000_000;
pub const MAX_TTL: u32 = 17_280_000; // ~200 days

pub enum DataKey {
    Admin,
    ProgramCounter,
    Mentor(Address),
    CompletionCount(Address),
    Completions(Address),
    ActiveCount,
    AuthorizedPrograms,
}

impl DataKey {
    pub fn to_symbol(&self) -> Symbol {
        match self {
            DataKey::Admin => Symbol::new(&Env::new(), "admin"),
            DataKey::ProgramCounter => Symbol::new(&Env::new(), "prog_cnt"),
            DataKey::Mentor(addr) => Symbol::new(&Env::new(), &format!("mentor:{}", addr)),
            DataKey::CompletionCount(addr) => {
                Symbol::new(&Env::new(), &format!("comp_cnt:{}", addr))
            }
            DataKey::Completions(addr) => {
                Symbol::new(&Env::new(), &format!("comps:{}", addr))
            }
            DataKey::ActiveCount => Symbol::new(&Env::new(), "active_cnt"),
            DataKey::AuthorizedPrograms => Symbol::new(&Env::new(), "auth_progs"),
        }
    }
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Address>>(&DataKey::Admin.to_symbol())
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set::<Symbol, Option<Address>>(&DataKey::Admin.to_symbol(), &Some(admin.clone()));
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), soroban_sdk::contracterror::ContractError> {
    let admin = get_admin(env).ok_or_else(|| {
        soroban_sdk::contracterror::ContractError::from_type_and_code(1, 1)
    })?;

    if admin != *caller {
        return Err(soroban_sdk::contracterror::ContractError::from_type_and_code(1, 3));
    }

    Ok(())
}

pub fn get_program_counter(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<Symbol, Option<u32>>(&DataKey::ProgramCounter.to_symbol())
        .flatten()
        .unwrap_or(0)
}

pub fn set_program_counter(env: &Env, counter: u32) {
    env.storage()
        .instance()
        .set::<Symbol, Option<u32>>(&DataKey::ProgramCounter.to_symbol(), &Some(counter));
}

pub fn increment_program_counter(env: &Env) -> u32 {
    let counter = get_program_counter(env) + 1;
    set_program_counter(env, counter);
    counter
}

pub fn mentor_exists(env: &Env, mentor: &Address) -> bool {
    env.storage()
        .instance()
        .has::<Symbol>(&DataKey::Mentor(mentor.clone()).to_symbol())
}

pub fn get_mentor(env: &Env, mentor: &Address) -> Option<mentor_types::MentorProfile> {
    env.storage()
        .instance()
        .get::<Symbol, Option<mentor_types::MentorProfile>>(
            &DataKey::Mentor(mentor.clone()).to_symbol(),
        )
        .flatten()
}

pub fn set_mentor(env: &Env, mentor: &Address, profile: mentor_types::MentorProfile) {
    env.storage()
        .instance()
        .set::<Symbol, Option<mentor_types::MentorProfile>>(
            &DataKey::Mentor(mentor.clone()).to_symbol(),
            &Some(profile),
        );
}

pub fn get_active_count(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<Symbol, Option<u32>>(&DataKey::ActiveCount.to_symbol())
        .flatten()
        .unwrap_or(0)
}

pub fn set_active_count(env: &Env, count: u32) {
    env.storage()
        .instance()
        .set::<Symbol, Option<u32>>(&DataKey::ActiveCount.to_symbol(), &Some(count));
}

pub fn increment_active_count(env: &Env) {
    let count = get_active_count(env) + 1;
    set_active_count(env, count);
}

pub fn decrement_active_count(env: &Env) {
    let count = get_active_count(env).saturating_sub(1);
    set_active_count(env, count);
}

pub fn add_completion(env: &Env, mentor: &Address, completion: CompletionRecord) {
    let mut completions = get_completions(env, mentor);
    completions.push_back(completion);

    env.storage()
        .instance()
        .set::<Symbol, Option<Vec<CompletionRecord>>>(
            &DataKey::Completions(mentor.clone()).to_symbol(),
            &Some(completions),
        );
}

pub fn get_completions(env: &Env, mentor: &Address) -> Vec<CompletionRecord> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Vec<CompletionRecord>>>(
            &DataKey::Completions(mentor.clone()).to_symbol(),
        )
        .flatten()
        .unwrap_or_else(|| Vec::new(env))
}

pub fn list_active_mentors(
    env: &Env,
    _offset: u32,
    _limit: u32,
) -> Result<Vec<Address>, soroban_sdk::contracterror::ContractError> {
    // This is a simplified version. In production, you'd iterate through
    // stored mentors and apply pagination. For now, return empty.
    Ok(Vec::new(env))
}

pub fn add_authorized_program(env: &Env, program: Address) {
    let mut programs = get_authorized_programs(env);
    programs.push_back(program);
    env.storage()
        .instance()
        .set::<Symbol, Option<Vec<Address>>>(&DataKey::AuthorizedPrograms.to_symbol(), &Some(programs));
}

pub fn remove_authorized_program(env: &Env, program: Address) {
    let programs = get_authorized_programs(env);
    let filtered: Vec<Address> = programs
        .iter()
        .filter(|p| *p != program)
        .cloned()
        .collect();

    env.storage()
        .instance()
        .set::<Symbol, Option<Vec<Address>>>(&DataKey::AuthorizedPrograms.to_symbol(), &Some(filtered));
}

pub fn get_authorized_programs(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Vec<Address>>>(&DataKey::AuthorizedPrograms.to_symbol())
        .flatten()
        .unwrap_or_else(|| Vec::new(env))
}

pub fn require_admin_or_authorized(
    env: &Env,
    caller: &Address,
) -> Result<(), soroban_sdk::contracterror::ContractError> {
    // Check if caller is admin
    if let Ok(()) = require_admin(env, caller) {
        return Ok(());
    }

    // Check if caller is authorized program
    let programs = get_authorized_programs(env);
    if programs.iter().any(|p| p == caller) {
        return Ok(());
    }

    Err(soroban_sdk::contracterror::ContractError::from_type_and_code(1, 3))
}
