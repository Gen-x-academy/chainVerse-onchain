use soroban_sdk::{Address, Env, Symbol};

pub const MIN_TTL: u32 = 1_000_000;
pub const MAX_TTL: u32 = 17_280_000;

pub enum DataKey {
    Admin,
    Registry,
    Discovery,
}

impl DataKey {
    pub fn to_symbol(&self) -> Symbol {
        match self {
            DataKey::Admin => Symbol::new(&Env::new(), "admin"),
            DataKey::Registry => Symbol::new(&Env::new(), "registry"),
            DataKey::Discovery => Symbol::new(&Env::new(), "discovery"),
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

pub fn get_discovery(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Address>>(&DataKey::Discovery.to_symbol())
}

pub fn set_discovery(env: &Env, discovery: &Address) {
    env.storage()
        .instance()
        .set::<Symbol, Option<Address>>(&DataKey::Discovery.to_symbol(), &Some(discovery.clone()));
}
