use mentor_types::ProgramSummary;
use soroban_sdk::{Address, Env, Symbol, Vec};

pub const MIN_TTL: u32 = 1_000_000;
pub const MAX_TTL: u32 = 17_280_000;

pub enum DataKey {
    Admin,
    Registry,
    ProgramCounter,
    Program(u32),
    AllPrograms,
}

impl DataKey {
    pub fn to_symbol(&self) -> Symbol {
        match self {
            DataKey::Admin => Symbol::new(&Env::new(), "admin"),
            DataKey::Registry => Symbol::new(&Env::new(), "registry"),
            DataKey::ProgramCounter => Symbol::new(&Env::new(), "prog_cnt"),
            DataKey::Program(id) => Symbol::new(&Env::new(), &format!("prog:{}", id)),
            DataKey::AllPrograms => Symbol::new(&Env::new(), "all_progs"),
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

pub fn get_registry(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Address>>(&DataKey::Registry.to_symbol())
}

pub fn set_registry(env: &Env, registry: &Address) {
    env.storage()
        .instance()
        .set::<Symbol, Option<Address>>(&DataKey::Registry.to_symbol(), &Some(registry.clone()));
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

pub fn get_program(env: &Env, program_id: u32) -> Option<ProgramSummary> {
    env.storage()
        .instance()
        .get::<Symbol, Option<ProgramSummary>>(&DataKey::Program(program_id).to_symbol())
        .flatten()
}

pub fn set_program(env: &Env, program_id: u32, program: ProgramSummary) {
    let mut all_progs = get_all_programs(env);

    // Check if updating existing
    let mut found = false;
    for i in 0..all_progs.len() {
        if let Some(p) = all_progs.get(i as u32) {
            if p.program_id == program_id {
                all_progs.set(i as u32, program.clone());
                found = true;
                break;
            }
        }
    }

    // If new, add to list
    if !found {
        all_progs.push_back(program.clone());
    }

    env.storage()
        .instance()
        .set::<Symbol, Option<Vec<ProgramSummary>>>(&DataKey::AllPrograms.to_symbol(), &Some(all_progs));

    env.storage()
        .instance()
        .set::<Symbol, Option<ProgramSummary>>(
            &DataKey::Program(program_id).to_symbol(),
            &Some(program),
        );
}

pub fn get_all_programs(env: &Env) -> Vec<ProgramSummary> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Vec<ProgramSummary>>>(&DataKey::AllPrograms.to_symbol())
        .flatten()
        .unwrap_or_else(|| Vec::new(env))
}
