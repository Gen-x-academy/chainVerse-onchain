#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 518_400;
const PERSISTENT_BUMP_AMOUNT: u32 = 2_592_000;
const MAX_HISTORY_ENTRIES: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RegistryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidTenant = 4,
    IncompatibleVersion = 5,
    DuplicateIntent = 6,
    ExpiredIntent = 7,
    InstanceNotFound = 8,
    InstanceSuperseded = 9,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum InstanceStatus {
    Active = 1,
    Superseded = 2,
    Deprecated = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractRegistration {
    pub contract_address: Address,
    pub interface_version: u32,
    pub capability_flags: u64,
    pub network_id: BytesN<32>,
    pub commitment_hash: BytesN<32>,
    pub status: InstanceStatus,
    pub superseded_by: Option<Address>,
    pub activated_at_ledger: u32,
    pub expires_at_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationHistoryEntry {
    pub contract_address: Address,
    pub interface_version: u32,
    pub capability_flags: u64,
    pub status: InstanceStatus,
    pub ledger: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TenantGovernor(BytesN<32>, Address),
    MinInterfaceVersion(BytesN<32>, Symbol),
    ActiveContract(BytesN<32>, Symbol),
    ContractRecord(BytesN<32>, Address),
    History(BytesN<32>, Symbol),
    ExecutedIntent(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CanonicalCapstoneRegistry;

#[contractimpl]
impl CanonicalCapstoneRegistry {
    pub fn initialize(env: Env, admin: Address) -> Result<(), RegistryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RegistryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn set_tenant_governor(
        env: Env,
        tenant_id: BytesN<32>,
        governor: Address,
        approved: bool,
    ) -> Result<(), RegistryError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(RegistryError::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::TenantGovernor(tenant_id.clone(), governor.clone());
        env.storage().persistent().set(&key, &approved);
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.events().publish(
            (symbol_short!("gov_set"), tenant_id, governor),
            approved,
        );
        Ok(())
    }

    pub fn set_min_interface_version(
        env: Env,
        tenant_id: BytesN<32>,
        governor: Address,
        service_key: Symbol,
        min_version: u32,
    ) -> Result<(), RegistryError> {
        Self::require_governor(&env, &tenant_id, &governor)?;

        let key = DataKey::MinInterfaceVersion(tenant_id.clone(), service_key.clone());
        env.storage().persistent().set(&key, &min_version);
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.events().publish(
            (symbol_short!("ver_min"), tenant_id, service_key),
            min_version,
        );
        Ok(())
    }

    /// Registers or upgrades an active canonical capstone contract under tenant governance.
    /// Marks any prior active instance as `Superseded` and appends a bounded history record.
    pub fn register_contract_instance(
        env: Env,
        tenant_id: BytesN<32>,
        governor: Address,
        intent_nonce: BytesN<32>,
        service_key: Symbol,
        contract_address: Address,
        interface_version: u32,
        capability_flags: u64,
        network_id: BytesN<32>,
        commitment_hash: BytesN<32>,
        expires_at_ledger: u32,
    ) -> Result<(), RegistryError> {
        Self::require_governor(&env, &tenant_id, &governor)?;

        let current_ledger = env.ledger().sequence();
        if expires_at_ledger <= current_ledger {
            return Err(RegistryError::ExpiredIntent);
        }

        // Domain-separated replay protection
        let intent_key = DataKey::ExecutedIntent(tenant_id.clone(), intent_nonce.clone());
        if env.storage().persistent().has(&intent_key) {
            return Err(RegistryError::DuplicateIntent);
        }

        let min_ver_key = DataKey::MinInterfaceVersion(tenant_id.clone(), service_key.clone());
        let min_version: u32 = env
            .storage()
            .persistent()
            .get(&min_ver_key)
            .unwrap_or(1);
        if interface_version < min_version {
            return Err(RegistryError::IncompatibleVersion);
        }

        // Supersede any previously active contract instance for this (tenant_id, service_key)
        let active_key = DataKey::ActiveContract(tenant_id.clone(), service_key.clone());
        if let Some(prev_addr) = env
            .storage()
            .persistent()
            .get::<_, Address>(&active_key)
        {
            if prev_addr != contract_address {
                let prev_rec_key = DataKey::ContractRecord(tenant_id.clone(), prev_addr.clone());
                if let Some(mut prev_record) = env
                    .storage()
                    .persistent()
                    .get::<_, ContractRegistration>(&prev_rec_key)
                {
                    prev_record.status = InstanceStatus::Superseded;
                    prev_record.superseded_by = Some(contract_address.clone());
                    env.storage().persistent().set(&prev_rec_key, &prev_record);
                    env.storage().persistent().extend_ttl(
                        &prev_rec_key,
                        PERSISTENT_LIFETIME_THRESHOLD,
                        PERSISTENT_BUMP_AMOUNT,
                    );
                }

                env.events().publish(
                    (symbol_short!("inst_sup"), tenant_id.clone(), service_key.clone()),
                    (prev_addr, contract_address.clone(), current_ledger),
                );
            }
        }

        let registration = ContractRegistration {
            contract_address: contract_address.clone(),
            interface_version,
            capability_flags,
            network_id: network_id.clone(),
            commitment_hash: commitment_hash.clone(),
            status: InstanceStatus::Active,
            superseded_by: None,
            activated_at_ledger: current_ledger,
            expires_at_ledger,
        };

        let rec_key = DataKey::ContractRecord(tenant_id.clone(), contract_address.clone());
        env.storage().persistent().set(&rec_key, &registration);
        env.storage().persistent().extend_ttl(
            &rec_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.storage().persistent().set(&active_key, &contract_address);
        env.storage().persistent().extend_ttl(
            &active_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.storage().persistent().set(&intent_key, &true);
        env.storage().persistent().extend_ttl(
            &intent_key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::append_history(
            &env,
            &tenant_id,
            &service_key,
            RegistrationHistoryEntry {
                contract_address: contract_address.clone(),
                interface_version,
                capability_flags,
                status: InstanceStatus::Active,
                ledger: current_ledger,
            },
        );

        env.events().publish(
            (symbol_short!("inst_act"), tenant_id, service_key),
            (
                contract_address,
                interface_version,
                capability_flags,
                network_id,
                commitment_hash,
                current_ledger,
            ),
        );

        Ok(())
    }

    /// Resolves only an approved, active, and non-expired canonical contract instance.
    pub fn resolve_active_contract(
        env: Env,
        tenant_id: BytesN<32>,
        service_key: Symbol,
        required_capabilities: u64,
    ) -> Result<ContractRegistration, RegistryError> {
        let active_key = DataKey::ActiveContract(tenant_id.clone(), service_key);
        let addr: Address = env
            .storage()
            .persistent()
            .get(&active_key)
            .ok_or(RegistryError::InstanceNotFound)?;

        let rec_key = DataKey::ContractRecord(tenant_id, addr);
        let record: ContractRegistration = env
            .storage()
            .persistent()
            .get(&rec_key)
            .ok_or(RegistryError::InstanceNotFound)?;

        if record.status != InstanceStatus::Active {
            return Err(RegistryError::InstanceSuperseded);
        }

        if env.ledger().sequence() >= record.expires_at_ledger {
            return Err(RegistryError::ExpiredIntent);
        }

        if (record.capability_flags & required_capabilities) != required_capabilities {
            return Err(RegistryError::IncompatibleVersion);
        }

        Ok(record)
    }

    pub fn get_registration_history(
        env: Env,
        tenant_id: BytesN<32>,
        service_key: Symbol,
    ) -> Vec<RegistrationHistoryEntry> {
        let key = DataKey::History(tenant_id, service_key);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    fn require_governor(
        env: &Env,
        tenant_id: &BytesN<32>,
        governor: &Address,
    ) -> Result<(), RegistryError> {
        governor.require_auth();
        let key = DataKey::TenantGovernor(tenant_id.clone(), governor.clone());
        let ok: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !ok {
            return Err(RegistryError::Unauthorized);
        }
        Ok(())
    }

    fn append_history(
        env: &Env,
        tenant_id: &BytesN<32>,
        service_key: &Symbol,
        entry: RegistrationHistoryEntry,
    ) {
        let key = DataKey::History(tenant_id.clone(), service_key.clone());
        let mut history: Vec<RegistrationHistoryEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));

        if history.len() >= MAX_HISTORY_ENTRIES {
            history.remove(0);
        }
        history.push_back(entry);

        env.storage().persistent().set(&key, &history);
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

    #[test]
    fn test_register_supersede_and_resolve_canonical_contracts() {
        let env = Env::default();
        env.mock_all_auths();

        let registry_id = env.register(CanonicalCapstoneRegistry, ());
        let client = CanonicalCapstoneRegistryClient::new(&env, &registry_id);

        let admin = Address::generate(&env);
        let governor = Address::generate(&env);
        let tenant = BytesN::from_array(&env, &[7u8; 32]);
        let network = BytesN::from_array(&env, &[1u8; 32]);
        let service = symbol_short!("escrow");

        client.initialize(&admin);
        client.set_tenant_governor(&tenant, &governor, &true);
        client.set_min_interface_version(&tenant, &governor, &service, &1);

        env.ledger().with_mut(|li| {
            li.sequence_number = 50;
        });

        let v1_addr = Address::generate(&env);
        let nonce_1 = BytesN::from_array(&env, &[11u8; 32]);
        let commit_1 = BytesN::from_array(&env, &[21u8; 32]);

        client.register_contract_instance(
            &tenant,
            &governor,
            &nonce_1,
            &service,
            &v1_addr,
            &1,
            &0b0011,
            &network,
            &commit_1,
            &500,
        );

        let resolved_v1 = client.resolve_active_contract(&tenant, &service, &0b0001);
        assert_eq!(resolved_v1.contract_address, v1_addr);
        assert_eq!(resolved_v1.status, InstanceStatus::Active);

        // Replay with same nonce is rejected
        let replay = client.try_register_contract_instance(
            &tenant,
            &governor,
            &nonce_1,
            &service,
            &v1_addr,
            &1,
            &0b0011,
            &network,
            &commit_1,
            &500,
        );
        assert_eq!(replay, Err(Ok(RegistryError::DuplicateIntent)));

        // Upgrade to v2 supersedes v1 and records verifiable history
        let v2_addr = Address::generate(&env);
        let nonce_2 = BytesN::from_array(&env, &[12u8; 32]);
        let commit_2 = BytesN::from_array(&env, &[22u8; 32]);

        client.register_contract_instance(
            &tenant,
            &governor,
            &nonce_2,
            &service,
            &v2_addr,
            &2,
            &0b0111,
            &network,
            &commit_2,
            &1_000,
        );

        let resolved_v2 = client.resolve_active_contract(&tenant, &service, &0b0101);
        assert_eq!(resolved_v2.contract_address, v2_addr);
        assert_eq!(resolved_v2.interface_version, 2);

        let history = client.get_registration_history(&tenant, &service);
        assert_eq!(history.len(), 2);
        assert_eq!(history.get(0).unwrap().contract_address, v1_addr);
        assert_eq!(history.get(1).unwrap().contract_address, v2_addr);
    }
}