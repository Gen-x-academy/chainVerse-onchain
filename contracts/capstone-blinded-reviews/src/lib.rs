#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_ASSIGNMENTS: u32 = 64;
const MIN_ASSIGNMENTS: u32 = 2;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const REQUEST_MIN_TTL: u32 = 1_555_200;
const REQUEST_MAX_TTL: u32 = 3_110_400;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidTransition = 6,
    StaleVersion = 7,
    VersionOverflow = 8,
    RequestConflict = 9,
    CollectionFull = 10,
    InvalidHash = 11,
    InvalidCommitment = 12,
    MissingPrerequisite = 13,
    ArithmeticOverflow = 14,
    DuplicateAssignment = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Config(BytesN<32>, BytesN<32>),
    Assignments(BytesN<32>, BytesN<32>),
    Assignment(BytesN<32>, BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    BlindCoordinator = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlindMode {
    Open = 1,
    SingleBlind = 2,
    DoubleBlind = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlindState {
    Configured = 1,
    Sealed = 2,
    Revealed = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Hidden = 1,
    Pseudonymous = 2,
    Visible = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindConfig {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub version: u32,
    pub state: BlindState,
    pub mode: BlindMode,
    pub assignment_count: u32,
    pub participant_commitment: BytesN<32>,
    pub reviewer_commitment: BytesN<32>,
    pub configured_at: u64,
    pub sealed_at: u64,
    pub revealed_at: u64,
    pub request_id: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assignment {
    pub blinded_ref: BytesN<32>,
    pub reviewer_commitment: BytesN<32>,
    pub participant_commitment: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub payload_hash: BytesN<32>,
    pub result_id: BytesN<32>,
    pub result_version: u32,
    pub recorded_at: u64,
}

#[contract]
pub struct CapstoneBlindedReviewsContract;

#[contractimpl]
impl CapstoneBlindedReviewsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("BLGRANT"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().remove(&key);
        env.events()
            .publish((symbol_short!("BLRVOK"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn configure(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        mode: BlindMode,
        participant_commitment: BytesN<32>,
        reviewer_commitment: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::BlindCoordinator,
        )?;
        if participant_commitment == Self::zero_id(&env)
            || reviewer_commitment == Self::zero_id(&env)
        {
            return Err(ContractError::InvalidCommitment);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Config(tenant_id.clone(), project_id.clone());
        let now = env.ledger().timestamp();
        let (version, configured_at, assignment_count) =
            match env.storage().persistent().get::<_, BlindConfig>(&key) {
                None => {
                    if expected_version != 0 {
                        return Err(ContractError::StaleVersion);
                    }
                    (1u32, now, 0u32)
                }
                Some(existing) => {
                    if existing.state != BlindState::Configured {
                        return Err(ContractError::InvalidTransition);
                    }
                    if existing.version != expected_version {
                        return Err(ContractError::StaleVersion);
                    }
                    (
                        existing
                            .version
                            .checked_add(1)
                            .ok_or(ContractError::VersionOverflow)?,
                        existing.configured_at,
                        existing.assignment_count,
                    )
                }
            };
        let config = BlindConfig {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            version,
            state: BlindState::Configured,
            mode,
            assignment_count,
            participant_commitment,
            reviewer_commitment,
            configured_at,
            sealed_at: 0,
            revealed_at: 0,
            request_id: request_id.clone(),
        };
        Self::store_config(&env, &key, &config);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("BLNEW"),),
            (tenant_id, project_id, mode, version),
        );
        Ok(version)
    }

    pub fn register_assignment(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        blinded_ref: BytesN<32>,
        reviewer_commitment: BytesN<32>,
        participant_commitment: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::BlindCoordinator,
        )?;
        if blinded_ref == Self::zero_id(&env)
            || reviewer_commitment == Self::zero_id(&env)
            || participant_commitment == Self::zero_id(&env)
        {
            return Err(ContractError::InvalidCommitment);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Config(tenant_id.clone(), project_id.clone());
        let mut config: BlindConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if config.state != BlindState::Configured {
            return Err(ContractError::InvalidTransition);
        }
        if config.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let ids_key = DataKey::Assignments(tenant_id.clone(), project_id.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));
        if ids.len() >= MAX_ASSIGNMENTS {
            return Err(ContractError::CollectionFull);
        }
        if Self::contains_id(&ids, &blinded_ref) {
            return Err(ContractError::DuplicateAssignment);
        }
        ids.push_back(blinded_ref.clone());
        let version = config
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let count = config
            .assignment_count
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if count != ids.len() {
            return Err(ContractError::ArithmeticOverflow);
        }
        let now = env.ledger().timestamp();
        config.version = version;
        config.assignment_count = count;
        Self::store_config(&env, &key, &config);
        let assignment_key = DataKey::Assignment(
            tenant_id.clone(),
            project_id.clone(),
            blinded_ref.clone(),
        );
        let assignment = Assignment {
            blinded_ref: blinded_ref.clone(),
            reviewer_commitment,
            participant_commitment,
            version: 1,
        };
        env.storage().persistent().set(&assignment_key, &assignment);
        env.storage()
            .persistent()
            .extend_ttl(&assignment_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.storage().persistent().set(&ids_key, &ids);
        env.storage()
            .persistent()
            .extend_ttl(&ids_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("BLASGN"),),
            (tenant_id, project_id, blinded_ref, version),
        );
        Ok(version)
    }

    pub fn seal(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        sealed_commitment: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::BlindCoordinator,
        )?;
        if sealed_commitment == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Config(tenant_id.clone(), project_id.clone());
        let mut config: BlindConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if config.state != BlindState::Configured {
            return Err(ContractError::InvalidTransition);
        }
        if config.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if config.assignment_count < MIN_ASSIGNMENTS {
            return Err(ContractError::MissingPrerequisite);
        }
        let version = config
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        config.version = version;
        config.state = BlindState::Sealed;
        config.sealed_at = now;
        Self::store_config(&env, &key, &config);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("BLSEAL"),),
            (tenant_id, project_id, sealed_commitment, version),
        );
        Ok(version)
    }

    pub fn reveal(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        reveal_commitment: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &admin, &tenant_id, &project_id, Role::Admin)?;
        if reveal_commitment == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Config(tenant_id.clone(), project_id.clone());
        let mut config: BlindConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if config.state != BlindState::Sealed {
            return Err(ContractError::InvalidTransition);
        }
        if config.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let version = config
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        config.version = version;
        config.state = BlindState::Revealed;
        config.revealed_at = now;
        Self::store_config(&env, &key, &config);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &project_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("BLREVL"),),
            (tenant_id, project_id, reveal_commitment, version),
        );
        Ok(version)
    }

    pub fn get_config(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Option<BlindConfig> {
        env.storage()
            .persistent()
            .get(&DataKey::Config(tenant_id, project_id))
    }

    pub fn get_assignment(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        blinded_ref: BytesN<32>,
    ) -> Option<Assignment> {
        env.storage().persistent().get(&DataKey::Assignment(
            tenant_id,
            project_id,
            blinded_ref,
        ))
    }

    pub fn get_assignments(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::Assignments(tenant_id, project_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn projection(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> Visibility {
        let config: BlindConfig = match env
            .storage()
            .persistent()
            .get(&DataKey::Config(tenant_id, project_id))
        {
            Some(value) => value,
            None => return Visibility::Hidden,
        };
        match config.mode {
            BlindMode::Open => Visibility::Visible,
            BlindMode::SingleBlind => {
                if config.state == BlindState::Revealed {
                    Visibility::Visible
                } else {
                    Visibility::Pseudonymous
                }
            }
            BlindMode::DoubleBlind => {
                if config.state == BlindState::Revealed {
                    Visibility::Pseudonymous
                } else {
                    Visibility::Hidden
                }
            }
        }
    }

    pub fn has_role(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        role: Role,
    ) -> bool {
        Self::has_stored_role(&env, &subject, &tenant_id, &project_id, role)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &configured != admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if let Some(configured) = env.storage().instance().get::<_, Address>(&DataKey::Admin) {
            if &configured == caller {
                return Ok(());
            }
        }
        let zero = Self::zero_id(env);
        if Self::has_stored_role(env, caller, tenant_id, &zero, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, role)
        {
            return Ok(());
        }
        Err(ContractError::Unauthorized)
    }

    fn has_stored_role(
        env: &Env,
        subject: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> bool {
        let key = DataKey::Role(subject.clone(), tenant_id.clone(), project_id.clone(), role);
        env.storage()
            .persistent()
            .get::<_, bool>(&key)
            .unwrap_or(false)
    }

    fn zero_id(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn replay_version(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(receipt) = env.storage().temporary().get::<_, RequestReceipt>(&key) {
            if &receipt.payload_hash != payload_hash || &receipt.result_id != result_id {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .temporary()
                .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
            return Ok(Some(receipt.result_version));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
        version: u32,
        now: u64,
    ) {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let receipt = RequestReceipt {
            payload_hash: payload_hash.clone(),
            result_id: result_id.clone(),
            result_version: version,
            recorded_at: now,
        };
        env.storage().temporary().set(&key, &receipt);
        env.storage()
            .temporary()
            .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
    }

    fn store_config(env: &Env, key: &DataKey, config: &BlindConfig) {
        env.storage().persistent().set(key, config);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn contains_id(ids: &Vec<BytesN<32>>, id: &BytesN<32>) -> bool {
        let mut i = 0;
        while i < ids.len() {
            if ids.get(i).as_ref() == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }
}
