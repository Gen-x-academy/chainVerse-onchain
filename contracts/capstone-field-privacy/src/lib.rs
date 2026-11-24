#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_RULES_PER_PROJECT: u32 = 256;
pub const MAX_REQUESTS_PER_ACTOR: u32 = 256;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const ROLE_TOPIC: Symbol = symbol_short!("CAPFLDROL");
const POLICY_TOPIC: Symbol = symbol_short!("CAPFLDPOL");
const RULE_TOPIC: Symbol = symbol_short!("CAPFLDRUL");
const REVOKE_TOPIC: Symbol = symbol_short!("CAPFLDREV");

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    PolicyLimit = 4,
    RuleNotFound = 5,
    PolicyNotFound = 6,
    StaleVersion = 7,
    RequestExists = 8,
    RequestConflict = 9,
    LimitReached = 10,
    Expired = 11,
    PolicyFrozen = 12,
    InvalidExpiry = 13,
    InvalidTransition = 14,
    RuleRevoked = 15,
    InvalidInput = 16,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    Public = 1,
    Participants = 2,
    Supervisors = 3,
    Reviewers = 4,
    Sponsors = 5,
    Administrators = 6,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectField {
    Profile = 1,
    Proposal = 2,
    Membership = 3,
    Notes = 4,
    Progress = 5,
    Outputs = 6,
    Location = 7,
    Funding = 8,
    Achievements = 9,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyState {
    Active = 1,
    Frozen = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleState {
    Active = 1,
    Revoked = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestAction {
    SetRule = 1,
    RevokeRule = 2,
    SetPolicy = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudienceVisibility {
    pub public: bool,
    pub participants: bool,
    pub supervisors: bool,
    pub reviewers: bool,
    pub sponsors: bool,
    pub administrators: bool,
}

impl AudienceVisibility {
    pub fn none() -> Self {
        Self {
            public: false,
            participants: false,
            supervisors: false,
            reviewers: false,
            sponsors: false,
            administrators: false,
        }
    }

    pub fn all() -> Self {
        Self {
            public: true,
            participants: true,
            supervisors: true,
            reviewers: true,
            sponsors: true,
            administrators: true,
        }
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldPolicy {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub field: ProjectField,
    pub allowed: AudienceVisibility,
    pub state: PolicyState,
    pub expected_version: u32,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldRule {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub subject: Address,
    pub field: ProjectField,
    pub allowed: AudienceVisibility,
    pub valid_until: u64,
    pub state: RuleState,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetFieldVisibilityInput {
    pub field: ProjectField,
    pub allowed: AudienceVisibility,
    pub valid_until: u64,
    pub request_id: BytesN<32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetFieldPolicyInput {
    pub field: ProjectField,
    pub allowed: AudienceVisibility,
    pub request_id: BytesN<32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevokeFieldVisibilityInput {
    pub field: ProjectField,
    pub request_id: BytesN<32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub action: RequestAction,
    pub field: ProjectField,
    pub allowed: AudienceVisibility,
    pub valid_until: u64,
    pub expected_version: u32,
    pub resulting_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Audience),
    Policy(BytesN<32>, BytesN<32>, ProjectField),
    Rule(BytesN<32>, BytesN<32>, Address, ProjectField),
    RuleCount(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, Address, BytesN<32>),
    RequestCount(BytesN<32>, BytesN<32>, Address),
}

#[contract]
pub struct CapstoneFieldPrivacyContract;

#[contractimpl]
impl CapstoneFieldPrivacyContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn set_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        audience: Audience,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(
            account.clone(),
            tenant_id.clone(),
            project_id.clone(),
            audience,
        );
        env.storage().persistent().set(&key, &active);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (ROLE_TOPIC,),
            (tenant_id, project_id, account, audience, active),
        );
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        audience: Audience,
    ) -> Result<(), ContractError> {
        Self::set_role(env, admin, account, tenant_id, project_id, audience, true)
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        audience: Audience,
    ) -> Result<(), ContractError> {
        Self::set_role(env, admin, account, tenant_id, project_id, audience, false)
    }

    pub fn has_role(
        env: Env,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        audience: Audience,
    ) -> bool {
        Self::is_role_active(&env, &account, &tenant_id, &project_id, audience)
    }

    pub fn set_policy(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: SetFieldPolicyInput,
    ) -> Result<FieldPolicy, ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone(), input.field);
        let existing: Option<FieldPolicy> = env.storage().persistent().get(&key);
        if let Some(resulting_version) = Self::check_receipt(
            &env,
            &tenant_id,
            &project_id,
            &admin,
            &input.request_id,
            RequestAction::SetPolicy,
            input.field,
            &input.allowed,
            0,
            input.expected_version,
        )? {
            if let Some(policy) = existing {
                if policy.expected_version == resulting_version {
                    return Ok(policy);
                }
            }
            return Err(ContractError::RequestConflict);
        }
        let now = env.ledger().timestamp();
        let policy = match existing {
            Some(mut policy) => {
                if input.expected_version == 0 && policy.allowed == input.allowed {
                    Self::record_request(
                        &env,
                        &tenant_id,
                        &project_id,
                        &admin,
                        &input.request_id,
                        RequestAction::SetPolicy,
                        input.field,
                        &input.allowed,
                        0,
                        input.expected_version,
                        policy.expected_version,
                    )?;
                    return Ok(policy);
                }
                if policy.state == PolicyState::Frozen {
                    return Err(ContractError::PolicyFrozen);
                }
                if input.expected_version != policy.expected_version {
                    return Err(ContractError::StaleVersion);
                }
                policy.allowed = input.allowed;
                policy.expected_version = policy
                    .expected_version
                    .checked_add(1)
                    .ok_or(ContractError::StaleVersion)?;
                policy.updated_at = now;
                policy
            }
            None => {
                if input.expected_version != 0 {
                    return Err(ContractError::StaleVersion);
                }
                FieldPolicy {
                    tenant_id: tenant_id.clone(),
                    project_id: project_id.clone(),
                    field: input.field,
                    allowed: input.allowed,
                    state: PolicyState::Active,
                    expected_version: 1,
                    updated_at: now,
                }
            }
        };
        env.storage().persistent().set(&key, &policy);
        Self::extend_persistent(&env, &key);
        Self::record_request(
            &env,
            &tenant_id,
            &project_id,
            &admin,
            &input.request_id,
            RequestAction::SetPolicy,
            input.field,
            &policy.allowed,
            0,
            input.expected_version,
            policy.expected_version,
        )?;
        env.events().publish(
            (POLICY_TOPIC,),
            (
                tenant_id,
                project_id,
                input.field,
                policy.allowed,
                policy.expected_version,
            ),
        );
        Ok(policy)
    }

    pub fn set_field_policy(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: SetFieldPolicyInput,
    ) -> Result<FieldPolicy, ContractError> {
        Self::set_policy(env, admin, tenant_id, project_id, input)
    }

    pub fn freeze_policy(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        field: ProjectField,
    ) -> Result<FieldPolicy, ContractError> {
        Self::transition_policy(
            &env,
            &admin,
            &tenant_id,
            &project_id,
            field,
            PolicyState::Active,
            PolicyState::Frozen,
        )
    }

    pub fn unfreeze_policy(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        field: ProjectField,
    ) -> Result<FieldPolicy, ContractError> {
        Self::transition_policy(
            &env,
            &admin,
            &tenant_id,
            &project_id,
            field,
            PolicyState::Frozen,
            PolicyState::Active,
        )
    }

    pub fn get_policy(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        field: ProjectField,
    ) -> Result<FieldPolicy, ContractError> {
        Ok(Self::load_policy_or_default(
            &env,
            &tenant_id,
            &project_id,
            field,
        ))
    }

    pub fn set_visibility(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: SetFieldVisibilityInput,
    ) -> Result<FieldRule, ContractError> {
        subject.require_auth();
        let now = env.ledger().timestamp();
        if input.valid_until != 0 && now >= input.valid_until {
            return Err(ContractError::InvalidExpiry);
        }
        let key = DataKey::Rule(
            tenant_id.clone(),
            project_id.clone(),
            subject.clone(),
            input.field,
        );
        let existing: Option<FieldRule> = env.storage().persistent().get(&key);
        if let Some(resulting_version) = Self::check_receipt(
            &env,
            &tenant_id,
            &project_id,
            &subject,
            &input.request_id,
            RequestAction::SetRule,
            input.field,
            &input.allowed,
            input.valid_until,
            input.expected_version,
        )? {
            if let Some(rule) = existing {
                if rule.expected_version == resulting_version {
                    return Ok(rule);
                }
            }
            return Err(ContractError::RequestConflict);
        }
        let policy = Self::load_policy_or_default(&env, &tenant_id, &project_id, input.field);
        if policy.state != PolicyState::Active {
            return Err(ContractError::PolicyFrozen);
        }
        if !Self::allowed_subset(&input.allowed, &policy.allowed) {
            return Err(ContractError::PolicyLimit);
        }
        let rule = match existing {
            Some(rule) => {
                if input.expected_version != rule.expected_version {
                    return Err(ContractError::StaleVersion);
                }
                FieldRule {
                    tenant_id: tenant_id.clone(),
                    project_id: project_id.clone(),
                    subject: subject.clone(),
                    field: input.field,
                    allowed: input.allowed,
                    valid_until: input.valid_until,
                    state: RuleState::Active,
                    expected_version: rule
                        .expected_version
                        .checked_add(1)
                        .ok_or(ContractError::StaleVersion)?,
                    created_at: rule.created_at,
                    updated_at: now,
                }
            }
            None => {
                if input.expected_version != 0 {
                    return Err(ContractError::StaleVersion);
                }
                let count_key = DataKey::RuleCount(tenant_id.clone(), project_id.clone());
                let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
                if count >= MAX_RULES_PER_PROJECT {
                    return Err(ContractError::LimitReached);
                }
                env.storage().persistent().set(
                    &count_key,
                    &count.checked_add(1).ok_or(ContractError::LimitReached)?,
                );
                Self::extend_persistent(&env, &count_key);
                FieldRule {
                    tenant_id: tenant_id.clone(),
                    project_id: project_id.clone(),
                    subject: subject.clone(),
                    field: input.field,
                    allowed: input.allowed,
                    valid_until: input.valid_until,
                    state: RuleState::Active,
                    expected_version: 1,
                    created_at: now,
                    updated_at: now,
                }
            }
        };
        env.storage().persistent().set(&key, &rule);
        Self::extend_persistent(&env, &key);
        Self::record_request(
            &env,
            &tenant_id,
            &project_id,
            &subject,
            &input.request_id,
            RequestAction::SetRule,
            input.field,
            &input.allowed,
            input.valid_until,
            input.expected_version,
            rule.expected_version,
        )?;
        env.events().publish(
            (RULE_TOPIC,),
            (
                tenant_id,
                project_id,
                subject,
                input.field,
                rule.allowed,
                rule.valid_until,
                rule.state,
                rule.expected_version,
            ),
        );
        Ok(rule)
    }

    pub fn set_field_visibility(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: SetFieldVisibilityInput,
    ) -> Result<FieldRule, ContractError> {
        Self::set_visibility(env, subject, tenant_id, project_id, input)
    }

    pub fn revoke_visibility(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: RevokeFieldVisibilityInput,
    ) -> Result<FieldRule, ContractError> {
        subject.require_auth();
        let key = DataKey::Rule(
            tenant_id.clone(),
            project_id.clone(),
            subject.clone(),
            input.field,
        );
        let mut rule: FieldRule = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::RuleNotFound)?;
        if let Some(resulting_version) = Self::check_receipt(
            &env,
            &tenant_id,
            &project_id,
            &subject,
            &input.request_id,
            RequestAction::RevokeRule,
            input.field,
            &AudienceVisibility::none(),
            0,
            input.expected_version,
        )? {
            if rule.expected_version == resulting_version {
                return Ok(rule);
            }
            return Err(ContractError::RequestConflict);
        }
        if rule.state == RuleState::Revoked {
            return Err(ContractError::RuleRevoked);
        }
        if input.expected_version != rule.expected_version {
            return Err(ContractError::StaleVersion);
        }
        rule.state = RuleState::Revoked;
        rule.expected_version = rule
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::StaleVersion)?;
        rule.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &rule);
        Self::extend_persistent(&env, &key);
        Self::record_request(
            &env,
            &tenant_id,
            &project_id,
            &subject,
            &input.request_id,
            RequestAction::RevokeRule,
            input.field,
            &AudienceVisibility::none(),
            0,
            input.expected_version,
            rule.expected_version,
        )?;
        env.events().publish(
            (REVOKE_TOPIC,),
            (
                tenant_id,
                project_id,
                subject,
                input.field,
                rule.state,
                rule.expected_version,
            ),
        );
        Ok(rule)
    }

    pub fn revoke_field_visibility(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: RevokeFieldVisibilityInput,
    ) -> Result<FieldRule, ContractError> {
        Self::revoke_visibility(env, subject, tenant_id, project_id, input)
    }

    pub fn get_rule(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        field: ProjectField,
    ) -> Result<FieldRule, ContractError> {
        let key = DataKey::Rule(tenant_id, project_id, subject, field);
        let rule: FieldRule = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::RuleNotFound)?;
        Self::extend_persistent(&env, &key);
        Ok(rule)
    }

    pub fn get_visibility(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        field: ProjectField,
    ) -> Option<FieldRule> {
        let key = DataKey::Rule(tenant_id, project_id, subject, field);
        let rule: Option<FieldRule> = env.storage().persistent().get(&key);
        if rule.is_some() {
            Self::extend_persistent(&env, &key);
        }
        rule
    }

    pub fn is_visible(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        field: ProjectField,
    ) -> bool {
        let key = DataKey::Rule(
            tenant_id.clone(),
            project_id.clone(),
            subject.clone(),
            field,
        );
        let Some(rule): Option<FieldRule> = env.storage().persistent().get(&key) else {
            return false;
        };
        Self::extend_persistent(&env, &key);
        if rule.state != RuleState::Active {
            return false;
        }
        if rule.valid_until != 0 && env.ledger().timestamp() >= rule.valid_until {
            return false;
        }
        if viewer == subject {
            return true;
        }
        let policy = Self::load_policy_or_default(&env, &tenant_id, &project_id, field);
        if policy.state != PolicyState::Active {
            return false;
        }
        if rule.allowed.public && policy.allowed.public {
            return true;
        }
        for audience in [
            Audience::Participants,
            Audience::Supervisors,
            Audience::Reviewers,
            Audience::Sponsors,
            Audience::Administrators,
        ] {
            if Self::audience_allowed(&rule.allowed, audience)
                && Self::audience_allowed(&policy.allowed, audience)
                && Self::is_role_active(&env, &viewer, &tenant_id, &project_id, audience)
            {
                return true;
            }
        }
        false
    }

    pub fn can_view(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        field: ProjectField,
    ) -> bool {
        Self::is_visible(env, viewer, tenant_id, project_id, subject, field)
    }

    pub fn is_field_visible(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        field: ProjectField,
    ) -> bool {
        Self::is_visible(env, viewer, tenant_id, project_id, subject, field)
    }

    pub fn version(_env: Env) -> u32 {
        SCHEMA_VERSION
    }

    pub fn supports_version(_env: Env, version: u32) -> bool {
        version == SCHEMA_VERSION
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &admin != caller {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn is_role_active(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        audience: Audience,
    ) -> bool {
        let key = DataKey::Role(
            account.clone(),
            tenant_id.clone(),
            project_id.clone(),
            audience,
        );
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend_persistent(env, &key);
        }
        active
    }

    fn transition_policy(
        env: &Env,
        admin: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        field: ProjectField,
        from: PolicyState,
        to: PolicyState,
    ) -> Result<FieldPolicy, ContractError> {
        Self::require_admin(env, admin)?;
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone(), field);
        let mut policy: FieldPolicy = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::PolicyNotFound)?;
        if policy.state != from {
            return Err(ContractError::InvalidTransition);
        }
        policy.state = to;
        policy.expected_version = policy
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::StaleVersion)?;
        policy.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &policy);
        Self::extend_persistent(env, &key);
        env.events().publish(
            (POLICY_TOPIC,),
            (
                tenant_id.clone(),
                project_id.clone(),
                field,
                policy.state,
                policy.expected_version,
            ),
        );
        Ok(policy)
    }

    fn load_policy_or_default(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        field: ProjectField,
    ) -> FieldPolicy {
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone(), field);
        let policy: Option<FieldPolicy> = env.storage().persistent().get(&key);
        if let Some(policy) = policy {
            Self::extend_persistent(env, &key);
            policy
        } else {
            FieldPolicy {
                tenant_id: tenant_id.clone(),
                project_id: project_id.clone(),
                field,
                allowed: AudienceVisibility::all(),
                state: PolicyState::Active,
                expected_version: 0,
                updated_at: 0,
            }
        }
    }

    fn audience_allowed(allowed: &AudienceVisibility, audience: Audience) -> bool {
        match audience {
            Audience::Public => allowed.public,
            Audience::Participants => allowed.participants,
            Audience::Supervisors => allowed.supervisors,
            Audience::Reviewers => allowed.reviewers,
            Audience::Sponsors => allowed.sponsors,
            Audience::Administrators => allowed.administrators,
        }
    }

    fn allowed_subset(requested: &AudienceVisibility, policy: &AudienceVisibility) -> bool {
        for audience in [
            Audience::Public,
            Audience::Participants,
            Audience::Supervisors,
            Audience::Reviewers,
            Audience::Sponsors,
            Audience::Administrators,
        ] {
            if Self::audience_allowed(requested, audience)
                && !Self::audience_allowed(policy, audience)
            {
                return false;
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn check_receipt(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        actor: &Address,
        request_id: &BytesN<32>,
        action: RequestAction,
        field: ProjectField,
        allowed: &AudienceVisibility,
        valid_until: u64,
        expected_version: u32,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            actor.clone(),
            request_id.clone(),
        );
        let receipt: Option<RequestReceipt> = env.storage().persistent().get(&key);
        match receipt {
            None => Ok(None),
            Some(receipt) => {
                if receipt.action == action
                    && receipt.field == field
                    && &receipt.allowed == allowed
                    && receipt.valid_until == valid_until
                    && receipt.expected_version == expected_version
                {
                    Self::extend_persistent(env, &key);
                    Ok(Some(receipt.resulting_version))
                } else {
                    Err(ContractError::RequestConflict)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        actor: &Address,
        request_id: &BytesN<32>,
        action: RequestAction,
        field: ProjectField,
        allowed: &AudienceVisibility,
        valid_until: u64,
        expected_version: u32,
        resulting_version: u32,
    ) -> Result<(), ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            actor.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::RequestExists);
        }
        let count_key = DataKey::RequestCount(tenant_id.clone(), project_id.clone(), actor.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_REQUESTS_PER_ACTOR {
            return Err(ContractError::LimitReached);
        }
        let receipt = RequestReceipt {
            action,
            field,
            allowed: *allowed,
            valid_until,
            expected_version,
            resulting_version,
        };
        env.storage().persistent().set(&key, &receipt);
        env.storage().persistent().set(
            &count_key,
            &count.checked_add(1).ok_or(ContractError::LimitReached)?,
        );
        Self::extend_persistent(env, &key);
        Self::extend_persistent(env, &count_key);
        Ok(())
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
