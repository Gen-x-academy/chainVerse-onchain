#![no_std]

//! Public, privacy-minimized mentor expertise taxonomy (MENTOR-010).
//!
//! Skill labels and aliases are public vocabulary. Skills have immutable
//! parents, so adding a node cannot create a cycle. Evidence requirements are
//! represented only by hashes and versioned; the evidence and requirement
//! text remain off-chain. Only the admin may change the taxonomy.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_SKILLS: u32 = 512;
const MAX_ALIASES: u32 = 2_048;
const MAX_HIERARCHY_DEPTH: u32 = 8;
const MAX_SKILLS_PAGE: u32 = 50;
pub const STORAGE_VERSION: u32 = 1;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    SkillNotFound = 4,
    SkillAlreadyExists = 5,
    InvalidParent = 6,
    SkillLimitExceeded = 7,
    HierarchyTooDeep = 8,
    AliasAlreadyExists = 9,
    AliasNotFound = 10,
    SkillDeprecated = 11,
    InvalidReplacement = 12,
    InvalidCommitment = 13,
    InvalidSchemaVersion = 14,
    EvidenceRequirementNotFound = 15,
    VersionOverflow = 16,
    AliasLimitExceeded = 17,
    InvalidLimit = 18,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Skill(BytesN<32>),
    SkillCount,
    SkillAt(u32),
    Alias(Symbol),
    AliasCount,
    EvidenceCurrent(BytesN<32>, ProficiencyLevel),
    EvidenceVersionCount(BytesN<32>, ProficiencyLevel),
    EvidenceVersion(BytesN<32>, ProficiencyLevel, u32),
}

/// Coarse, stable proficiency vocabulary; detailed rubrics stay off-chain.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProficiencyLevel {
    Foundation,
    Developing,
    Proficient,
    Expert,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillStatus {
    Active,
    Deprecated,
}

/// A public taxonomy node. `label` is public; private mentor profile data is
/// not part of this contract. Parent links are fixed at registration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    pub id: BytesN<32>,
    pub parent: Option<BytesN<32>>,
    pub label: Symbol,
    pub status: SkillStatus,
    pub replacement: Option<BytesN<32>>,
    pub created_at: u64,
    pub deprecated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AliasStatus {
    Active,
    Deprecated,
}

/// Alias text is public taxonomy vocabulary, not a mentor's private data.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillAlias {
    pub alias: Symbol,
    pub skill_id: BytesN<32>,
    pub status: AliasStatus,
    pub created_at: u64,
    pub deprecated_at: u64,
}

/// Integrity commitment to off-chain evidence criteria. No evidence or
/// private assessment content is stored on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRequirement {
    pub skill_id: BytesN<32>,
    pub level: ProficiencyLevel,
    pub version: u32,
    pub requirement_hash: BytesN<32>,
    pub schema_version: u32,
    pub published_at: u64,
    pub published_by: Address,
}

#[contract]
pub struct MentorExpertiseContract;

#[contractimpl]
impl MentorExpertiseContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn skill(env: &Env, id: &BytesN<32>) -> Result<Skill, ContractError> {
        let key = DataKey::Skill(id.clone());
        let skill: Skill = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SkillNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(skill)
    }

    /// Register a canonical public taxonomy term. Parentage is immutable;
    /// the parent must exist and be active. A root has `parent = None`.
    pub fn register_skill(
        env: Env,
        admin: Address,
        skill_id: BytesN<32>,
        parent: Option<BytesN<32>>,
        label: Symbol,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if skill_id == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidCommitment);
        }
        let key = DataKey::Skill(skill_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::SkillAlreadyExists);
        }

        if let Some(parent_id) = parent.clone() {
            let parent_skill = Self::skill(&env, &parent_id)
                .map_err(|_| ContractError::InvalidParent)?;
            if parent_skill.status != SkillStatus::Active {
                return Err(ContractError::SkillDeprecated);
            }
            let mut depth = 1u32;
            let mut ancestor = Some(parent_id);
            while let Some(ancestor_id) = ancestor {
                depth += 1;
                if depth > MAX_HIERARCHY_DEPTH {
                    return Err(ContractError::HierarchyTooDeep);
                }
                ancestor = Self::skill(&env, &ancestor_id)?.parent;
            }
        }

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::SkillCount)
            .unwrap_or(0);
        if count >= MAX_SKILLS {
            return Err(ContractError::SkillLimitExceeded);
        }
        let next = count
            .checked_add(1)
            .ok_or(ContractError::SkillLimitExceeded)?;
        let now = env.ledger().timestamp();
        let skill = Skill {
            id: skill_id.clone(),
            parent,
            label: label.clone(),
            status: SkillStatus::Active,
            replacement: None,
            created_at: now,
            deprecated_at: 0,
        };

        env.storage().persistent().set(&key, &skill);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let index_key = DataKey::SkillAt(count);
        env.storage().persistent().set(&index_key, &skill_id);
        env.storage()
            .persistent()
            .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_counter(&env, DataKey::SkillCount, next);

        env.events()
            .publish((symbol_short!("SKNEW"),), (skill_id, parent, label, now));
        Ok(())
    }

    /// Bind a unique public alias to an active canonical skill.
    pub fn register_alias(
        env: Env,
        admin: Address,
        alias: Symbol,
        skill_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let target = Self::skill(&env, &skill_id)?;
        if target.status != SkillStatus::Active {
            return Err(ContractError::SkillDeprecated);
        }
        let key = DataKey::Alias(alias.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AliasAlreadyExists);
        }
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AliasCount)
            .unwrap_or(0);
        if count >= MAX_ALIASES {
            return Err(ContractError::AliasLimitExceeded);
        }
        let now = env.ledger().timestamp();
        let record = SkillAlias {
            alias: alias.clone(),
            skill_id: skill_id.clone(),
            status: AliasStatus::Active,
            created_at: now,
            deprecated_at: 0,
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_counter(&env, DataKey::AliasCount, count + 1);
        env.events()
            .publish((symbol_short!("SKALIAS"),), (alias, skill_id));
        Ok(())
    }

    /// Resolve an active alias to its canonical skill. Deprecated aliases
    /// and aliases of deprecated skills are not resolvable.
    pub fn resolve_alias(env: Env, alias: Symbol) -> Result<BytesN<32>, ContractError> {
        let key = DataKey::Alias(alias);
        let record: SkillAlias = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AliasNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        if record.status != AliasStatus::Active {
            return Err(ContractError::AliasNotFound);
        }
        let skill = Self::skill(&env, &record.skill_id)?;
        if skill.status != SkillStatus::Active {
            return Err(ContractError::SkillDeprecated);
        }
        Ok(record.skill_id)
    }

    /// Read an alias record, including its status after deprecation.
    pub fn get_alias(env: Env, alias: Symbol) -> Result<SkillAlias, ContractError> {
        let key = DataKey::Alias(alias);
        let record: SkillAlias = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AliasNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    /// Retire an alias without deleting its audit record or allowing reuse.
    pub fn deprecate_alias(env: Env, admin: Address, alias: Symbol) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Alias(alias.clone());
        let mut record: SkillAlias = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AliasNotFound)?;
        if record.status == AliasStatus::Deprecated {
            return Err(ContractError::AliasNotFound);
        }
        record.status = AliasStatus::Deprecated;
        record.deprecated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("ALIASDEP"),), (alias, record.skill_id));
        Ok(())
    }

    /// Deprecate a canonical term, optionally pointing to an active
    /// replacement. Deprecated terms remain readable for historical data.
    pub fn deprecate_skill(
        env: Env,
        admin: Address,
        skill_id: BytesN<32>,
        replacement: Option<BytesN<32>>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let mut record = Self::skill(&env, &skill_id)?;
        if record.status == SkillStatus::Deprecated {
            return Err(ContractError::SkillDeprecated);
        }
        if let Some(replacement_id) = replacement.clone() {
            if replacement_id == skill_id {
                return Err(ContractError::InvalidReplacement);
            }
            let target = Self::skill(&env, &replacement_id)
                .map_err(|_| ContractError::InvalidReplacement)?;
            if target.status != SkillStatus::Active {
                return Err(ContractError::InvalidReplacement);
            }
        }
        record.status = SkillStatus::Deprecated;
        record.replacement = replacement.clone();
        record.deprecated_at = env.ledger().timestamp();
        let key = DataKey::Skill(skill_id.clone());
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("SKDEPR"),),
            (skill_id, replacement, record.deprecated_at),
        );
        Ok(())
    }

    /// Publish or replace an off-chain evidence-rubric commitment. Each
    /// update creates a new immutable version for this skill and level.
    pub fn publish_evidence_requirement(
        env: Env,
        admin: Address,
        skill_id: BytesN<32>,
        level: ProficiencyLevel,
        requirement_hash: BytesN<32>,
        schema_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        let skill = Self::skill(&env, &skill_id)?;
        if skill.status != SkillStatus::Active {
            return Err(ContractError::SkillDeprecated);
        }
        if requirement_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidCommitment);
        }
        if schema_version == 0 {
            return Err(ContractError::InvalidSchemaVersion);
        }
        let count_key = DataKey::EvidenceVersionCount(skill_id.clone(), level);
        let current: u32 = env.storage().instance().get(&count_key).unwrap_or(0);
        let version = current
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let requirement = EvidenceRequirement {
            skill_id: skill_id.clone(),
            level,
            version,
            requirement_hash: requirement_hash.clone(),
            schema_version,
            published_at: env.ledger().timestamp(),
            published_by: admin.clone(),
        };
        let history_key = DataKey::EvidenceVersion(skill_id.clone(), level, version);
        env.storage().persistent().set(&history_key, &requirement);
        env.storage()
            .persistent()
            .extend_ttl(&history_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_counter(&env, count_key, version);
        let current_key = DataKey::EvidenceCurrent(skill_id.clone(), level);
        env.storage().persistent().set(&current_key, &requirement);
        env.storage()
            .persistent()
            .extend_ttl(&current_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("EVIDREQ"),),
            (skill_id, level, version, requirement_hash, schema_version),
        );
        Ok(version)
    }

    pub fn get_skill(env: Env, skill_id: BytesN<32>) -> Result<Skill, ContractError> {
        Self::skill(&env, &skill_id)
    }

    pub fn get_evidence_requirement(
        env: Env,
        skill_id: BytesN<32>,
        level: ProficiencyLevel,
    ) -> Result<EvidenceRequirement, ContractError> {
        Self::get_requirement(&env, &DataKey::EvidenceCurrent(skill_id, level))
    }

    pub fn get_evidence_requirement_version(
        env: Env,
        skill_id: BytesN<32>,
        level: ProficiencyLevel,
        version: u32,
    ) -> Result<EvidenceRequirement, ContractError> {
        if version == 0 {
            return Err(ContractError::EvidenceRequirementNotFound);
        }
        Self::get_requirement(
            &env,
            &DataKey::EvidenceVersion(skill_id, level, version),
        )
    }

    fn get_requirement(
        env: &Env,
        key: &DataKey,
    ) -> Result<EvidenceRequirement, ContractError> {
        let requirement: EvidenceRequirement = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::EvidenceRequirementNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(requirement)
    }

    pub fn skill_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::SkillCount)
            .unwrap_or(0)
    }

    /// Bounded, zero-based page over registered skills.
    pub fn list_skills(env: Env, cursor: u32, limit: u32) -> Result<Vec<Skill>, ContractError> {
        if limit == 0 || limit > MAX_SKILLS_PAGE {
            return Err(ContractError::InvalidLimit);
        }
        let count = Self::skill_count(env.clone());
        if cursor > count {
            return Err(ContractError::InvalidLimit);
        }
        let end = core::cmp::min(cursor.saturating_add(limit), count);
        let mut results = Vec::new(&env);
        for index in cursor..end {
            let index_key = DataKey::SkillAt(index);
            let id: BytesN<32> = env
                .storage()
                .persistent()
                .get(&index_key)
                .ok_or(ContractError::SkillNotFound)?;
            env.storage()
                .persistent()
                .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            results.push_back(Self::skill(&env, &id)?);
        }
        Ok(results)
    }

    fn write_counter(env: &Env, key: DataKey, value: u32) {
        env.storage().instance().set(&key, &value);
    }
}

#[cfg(test)]
mod tests;