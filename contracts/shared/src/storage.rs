use soroban_sdk::{Env, IntoVal, TryFromVal, Val};

// ========================
// Instance Storage Helpers
// ========================

/// Retrieves a value from instance storage by key.
///
/// Returns `None` if the key does not exist.
pub fn get_instance_storage<K, V>(env: &Env, key: &K) -> Option<V>
where
    K: IntoVal<Env, Val>,
    V: TryFromVal<Env, Val>,
{
    env.storage().instance().get(key)
}

/// Stores a key-value pair in instance storage.
///
/// Overwrites any existing value for the given key.
pub fn set_instance_storage<K, V>(env: &Env, key: &K, val: &V)
where
    K: IntoVal<Env, Val>,
    V: IntoVal<Env, Val>,
{
    env.storage().instance().set(key, val);
}

/// Removes a key and its associated value from instance storage.
///
/// No-op if the key does not exist.
pub fn remove_instance_storage<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage().instance().remove(key);
}

// ===========================
// Persistent Storage Helpers
// ===========================

/// Retrieves a value from persistent storage by key.
///
/// Returns `None` if the key does not exist.
pub fn get_persistent_storage<K, V>(env: &Env, key: &K) -> Option<V>
where
    K: IntoVal<Env, Val>,
    V: TryFromVal<Env, Val>,
{
    env.storage().persistent().get(key)
}

/// Stores a key-value pair in persistent storage.
///
/// Overwrites any existing value for the given key.
pub fn set_persistent_storage<K, V>(env: &Env, key: &K, val: &V)
where
    K: IntoVal<Env, Val>,
    V: IntoVal<Env, Val>,
{
    env.storage().persistent().set(key, val);
    env.storage()
        .persistent()
        .extend_ttl(key, crate::MIN_TTL, crate::MAX_TTL);
}

/// Removes a key and its associated value from persistent storage.
///
/// No-op if the key does not exist.
pub fn remove_persistent_storage<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage().persistent().remove(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{symbol_short, Env};

    // =====================
    // Instance Storage Tests
    // =====================

    #[test]
    fn test_instance_set_and_get() {
        let env = Env::default();
        let key = symbol_short!("MY_KEY");
        set_instance_storage(&env, &key, &42u32);
        let result: Option<u32> = get_instance_storage(&env, &key);
        assert_eq!(result, Some(42u32));
    }

    #[test]
    fn test_instance_get_nonexistent_returns_none() {
        let env = Env::default();
        let key = symbol_short!("MISSING");
        let result: Option<u32> = get_instance_storage(&env, &key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_instance_remove_clears_value() {
        let env = Env::default();
        let key = symbol_short!("DEL_KEY");
        set_instance_storage(&env, &key, &99u32);
        remove_instance_storage(&env, &key);
        let result: Option<u32> = get_instance_storage(&env, &key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_instance_set_overwrites_value() {
        let env = Env::default();
        let key = symbol_short!("OVR_KEY");
        set_instance_storage(&env, &key, &10u32);
        set_instance_storage(&env, &key, &20u32);
        let result: Option<u32> = get_instance_storage(&env, &key);
        assert_eq!(result, Some(20u32));
    }

    // ========================
    // Persistent Storage Tests
    // ========================

    #[test]
    fn test_persistent_set_and_get() {
        let env = Env::default();
        let key = symbol_short!("P_KEY");
        set_persistent_storage(&env, &key, &42u32);
        let result: Option<u32> = get_persistent_storage(&env, &key);
        assert_eq!(result, Some(42u32));
    }

    #[test]
    fn test_persistent_get_nonexistent_returns_none() {
        let env = Env::default();
        let key = symbol_short!("P_MISS");
        let result: Option<u32> = get_persistent_storage(&env, &key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_persistent_remove_clears_value() {
        let env = Env::default();
        let key = symbol_short!("P_DEL");
        set_persistent_storage(&env, &key, &99u32);
        remove_persistent_storage(&env, &key);
        let result: Option<u32> = get_persistent_storage(&env, &key);
        assert_eq!(result, None);
    }

    #[test]
    fn test_persistent_set_overwrites_value() {
        let env = Env::default();
        let key = symbol_short!("P_OVR");
        set_persistent_storage(&env, &key, &10u32);
        set_persistent_storage(&env, &key, &20u32);
        let result: Option<u32> = get_persistent_storage(&env, &key);
        assert_eq!(result, Some(20u32));
    }
}
