use soroban_sdk::{Env, Symbol, TryFromVal, IntoVal, Val};

/// Retrieves a value from instance storage by key.
/// Returns `Option<T>` for missing values, avoiding panic.
pub fn get_storage<T: TryFromVal<Env, Val>>(
    env: &Env, 
    key: &Symbol
) -> Option<T> {
    env.storage().instance().get(key)
}

/// Stores a value in instance storage associated with a key.
/// Uses `IntoVal` trait for type-safe conversion to Soroban `Val`.
pub fn set_storage<T: IntoVal<Env, Val>>(
    env: &Env, 
    key: &Symbol, 
    value: &T
) {
    env.storage().instance().set(key, value);
}

/// Removes a key and its associated value from instance storage.
pub fn remove_storage(env: &Env, key: &Symbol) {
    env.storage().instance().remove(key);
}

/// Optional advanced helper to check if a key exists in instance storage.
pub fn has_storage(env: &Env, key: &Symbol) -> bool {
    env.storage().instance().has(key)
}
