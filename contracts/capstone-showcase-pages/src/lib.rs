#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val, Vec,
};

const RECORD_MIN_TTL: u32 = 100_000;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_VERIFICATION_LINKS: u32 = 16;
const MAX_REVISIONS: u32 = 32;
const MAX_VALIDITY_SECS: u64 = 31_536_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    WrongTenant = 4,
    RequestAlreadyUsed = 5,
    IntentAlreadyUsed = 6,
    StaleVersion = 7,
    PageNotFound = 8,
    InvalidState = 9,
    TooManyLinks = 10,
    InvalidValidity = 11,
    Overflow = 12,
    NotExpired = 13,
    RevisionLimit = 14,
    AlreadyExpired = 15,
    AlreadyHasRole = 16,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Administrator,
    Operator,
    Curator,
    Publisher,
    Auditor,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageState {
    Draft,
    Published,
    Updated,
    Unpublished,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentMarker {
    pub fingerprint: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShowcasePage {
    pub page_id: BytesN<32>,
    pub version: u32,
    pub approval_hash: BytesN<32>,
    pub content_hash: BytesN<32>,
    pub contributor_hash: BytesN<32>,
    pub output_hash: BytesN<32>,
    pub demonstration_hash: BytesN<32>,
    pub citation_hash: BytesN<32>,
    pub license_hash: BytesN<32>,
    pub funding_hash: BytesN<32>,
    pub verification_hash: BytesN<32>,
    pub verification_links: Vec<BytesN<32>>,
    pub state: PageState,
    pub published_at: u64,
    pub expires_at: u64,
    pub revisions: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    TenantAdmin(BytesN<32>),
    Role(BytesN<32>, Address, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Intent(BytesN<32>, BytesN<32>, BytesN<32>),
    Page(BytesN<32>, BytesN<32>, BytesN<32>),
    Latest(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneShowcasePagesContract;

#[contractimpl]
impl CapstoneShowcasePagesContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        let admin_key = DataKey::TenantAdmin(tenant_id.clone());
        if env.storage().persistent().has(&admin_key) {
            return Err(ContractError::AlreadyInitialized);
        }
        Self::write(&env, &admin_key, &admin);
        let role_key = DataKey::Role(tenant_id.clone(), admin, Role::Administrator);
        Self::write(&env, &role_key, &true);
        env.events().publish((symbol_short!("CAPTNT"),), (tenant_id,));
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &admin, &tenant_id, Role::Administrator)?;
        let key = DataKey::Role(tenant_id.clone(), account, role);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyHasRole);
        }
        Self::write(&env, &key, &true);
        env.events()
            .publish((symbol_short!("CAPROL"),), (tenant_id, role));
        Ok(())
    }

    pub fn create_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        version: u32,
        approval_hash: BytesN<32>,
        content_hash: BytesN<32>,
        contributor_hash: BytesN<32>,
        output_hash: BytesN<32>,
        demonstration_hash: BytesN<32>,
        citation_hash: BytesN<32>,
        license_hash: BytesN<32>,
        funding_hash: BytesN<32>,
        verification_hash: BytesN<32>,
        verification_links: Vec<BytesN<32>>,
        valid_for_secs: u64,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Curator)?;
        if verification_links.len() > MAX_VERIFICATION_LINKS {
            return Err(ContractError::TooManyLinks);
        }
        if valid_for_secs == 0 || valid_for_secs > MAX_VALIDITY_SECS {
            return Err(ContractError::InvalidValidity);
        }
        let payload_hash = Self::payload_hash(
            &env,
            &approval_hash,
            &content_hash,
            &contributor_hash,
            &output_hash,
            &demonstration_hash,
            &citation_hash,
            &license_hash,
            &funding_hash,
            &verification_hash,
            &verification_links,
            valid_for_secs,
        );
        let fingerprint = Self::request_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &actor,
            &payload_hash,
            1,
            version,
        );
        if Self::check_request(&env, &tenant_id, &project_id, &request_id, &fingerprint)? {
            return Ok(request_id);
        }
        let page_key = DataKey::Page(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&page_key) {
            return Err(ContractError::RequestAlreadyUsed);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(valid_for_secs)
            .ok_or(ContractError::Overflow)?;
        let page = ShowcasePage {
            page_id: request_id.clone(),
            version,
            approval_hash,
            content_hash,
            contributor_hash,
            output_hash,
            demonstration_hash,
            citation_hash,
            license_hash,
            funding_hash,
            verification_hash,
            verification_links,
            state: PageState::Draft,
            published_at: 0,
            expires_at,
            revisions: 0,
        };
        Self::write(&env, &page_key, &page);
        let latest_key = DataKey::Latest(tenant_id.clone(), project_id.clone());
        Self::write(&env, &latest_key, &request_id);
        env.events().publish(
            (symbol_short!("SHWNEW"),),
            (tenant_id, project_id, request_id, version, approval_hash),
        );
        Ok(request_id)
    }

    pub fn publish_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        page_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Publisher)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &page_id,
            &actor,
            2,
            expected_version,
            &page_id,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            return Ok(());
        }
        let (page_key, mut page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        if page.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if env.ledger().timestamp() > page.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if page.state != PageState::Draft {
            return Err(ContractError::InvalidState);
        }
        page.state = PageState::Published;
        page.published_at = env.ledger().timestamp();
        Self::write(&env, &page_key, &page);
        env.events().publish(
            (symbol_short!("SHWPUB"),),
            (tenant_id, project_id, page_id, expected_version, page.published_at),
        );
        Ok(())
    }

    pub fn update_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        page_id: BytesN<32>,
        expected_version: u32,
        content_hash: BytesN<32>,
        verification_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Curator)?;
        let mut change = Bytes::new(&env);
        change.extend_from_array(&content_hash.to_array());
        change.extend_from_array(&verification_hash.to_array());
        let change_hash = env.crypto().sha256(&change);
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &page_id,
            &actor,
            3,
            expected_version,
            &change_hash,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            let (_, existing) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
            return Ok(existing.version);
        }
        let (page_key, mut page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        if page.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if env.ledger().timestamp() > page.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if page.state != PageState::Published && page.state != PageState::Updated {
            return Err(ContractError::InvalidState);
        }
        if page.revisions >= MAX_REVISIONS {
            return Err(ContractError::RevisionLimit);
        }
        let version = expected_version
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        let revisions = page.revisions.checked_add(1).ok_or(ContractError::Overflow)?;
        page.version = version;
        page.revisions = revisions;
        page.content_hash = content_hash;
        page.verification_hash = verification_hash;
        page.state = PageState::Updated;
        Self::write(&env, &page_key, &page);
        env.events().publish(
            (symbol_short!("SHWUPD"),),
            (tenant_id, project_id, page_id, version, revisions),
        );
        Ok(version)
    }

    pub fn unpublish_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        page_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Publisher)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &page_id,
            &actor,
            4,
            expected_version,
            &page_id,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            return Ok(());
        }
        let (page_key, mut page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        if page.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if page.state != PageState::Published && page.state != PageState::Updated {
            return Err(ContractError::InvalidState);
        }
        page.state = PageState::Unpublished;
        Self::write(&env, &page_key, &page);
        env.events().publish(
            (symbol_short!("SHWUNP"),),
            (tenant_id, project_id, page_id, expected_version),
        );
        Ok(())
    }

    pub fn expire_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        page_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Operator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &page_id,
            &actor,
            5,
            expected_version,
            &page_id,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            return Ok(());
        }
        let (page_key, mut page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        if page.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if env.ledger().timestamp() <= page.expires_at {
            return Err(ContractError::NotExpired);
        }
        if page.state == PageState::Expired {
            return Err(ContractError::InvalidState);
        }
        page.state = PageState::Expired;
        Self::write(&env, &page_key, &page);
        env.events().publish(
            (symbol_short!("SHWEXP"),),
            (tenant_id, project_id, page_id, expected_version),
        );
        Ok(())
    }

    pub fn get_page(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        page_id: BytesN<32>,
    ) -> Result<ShowcasePage, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let (_, page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        Ok(page)
    }

    pub fn get_latest_page(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<ShowcasePage, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let key = DataKey::Latest(tenant_id.clone(), project_id.clone());
        let page_id = env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&key)
            .ok_or(ContractError::PageNotFound)?;
        Self::extend(&env, &key);
        let (_, page) = Self::load_page(&env, &tenant_id, &project_id, &page_id)?;
        Ok(page)
    }

    fn require_tenant(env: &Env, tenant_id: &BytesN<32>) -> Result<(), ContractError> {
        let key = DataKey::TenantAdmin(tenant_id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::WrongTenant);
        }
        Self::extend(env, &key);
        Ok(())
    }

    fn require_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_tenant(env, tenant_id)?;
        actor.require_auth();
        let admin_key = DataKey::Role(tenant_id.clone(), actor.clone(), Role::Administrator);
        if env.storage().persistent().get::<_, bool>(&admin_key).unwrap_or(false) {
            Self::extend(env, &admin_key);
            return Ok(());
        }
        let key = DataKey::Role(tenant_id.clone(), actor.clone(), role);
        if env.storage().persistent().get::<_, bool>(&key).unwrap_or(false) {
            Self::extend(env, &key);
            return Ok(());
        }
        Err(ContractError::NotAuthorized)
    }

    fn check_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        fingerprint: &BytesN<32>,
    ) -> Result<bool, ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(stored) = env.storage().persistent().get::<_, BytesN<32>>(&key) {
            Self::extend(env, &key);
            if stored == *fingerprint {
                return Ok(true);
            }
            return Err(ContractError::RequestAlreadyUsed);
        }
        Self::write(env, &key, fingerprint);
        Ok(false)
    }

    fn check_intent(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
        fingerprint: &BytesN<32>,
        version: u32,
    ) -> Result<bool, ContractError> {
        let key = DataKey::Intent(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        );
        if let Some(marker) = env.storage().persistent().get::<_, IntentMarker>(&key) {
            Self::extend(env, &key);
            if marker.fingerprint == *fingerprint && marker.version == version {
                return Ok(true);
            }
            return Err(ContractError::IntentAlreadyUsed);
        }
        Self::write(
            env,
            &key,
            &IntentMarker {
                fingerprint: fingerprint.clone(),
                version,
            },
        );
        Ok(false)
    }

    fn load_page(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        page_id: &BytesN<32>,
    ) -> Result<(DataKey, ShowcasePage), ContractError> {
        let key = DataKey::Page(tenant_id.clone(), project_id.clone(), page_id.clone());
        let page = env
            .storage()
            .persistent()
            .get::<_, ShowcasePage>(&key)
            .ok_or(ContractError::PageNotFound)?;
        Self::extend(env, &key);
        Ok((key, page))
    }

    fn payload_hash(
        env: &Env,
        approval_hash: &BytesN<32>,
        content_hash: &BytesN<32>,
        contributor_hash: &BytesN<32>,
        output_hash: &BytesN<32>,
        demonstration_hash: &BytesN<32>,
        citation_hash: &BytesN<32>,
        license_hash: &BytesN<32>,
        funding_hash: &BytesN<32>,
        verification_hash: &BytesN<32>,
        verification_links: &Vec<BytesN<32>>,
        valid_for_secs: u64,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&valid_for_secs.to_be_bytes());
        preimage.extend_from_array(&approval_hash.to_array());
        preimage.extend_from_array(&content_hash.to_array());
        preimage.extend_from_array(&contributor_hash.to_array());
        preimage.extend_from_array(&output_hash.to_array());
        preimage.extend_from_array(&demonstration_hash.to_array());
        preimage.extend_from_array(&citation_hash.to_array());
        preimage.extend_from_array(&license_hash.to_array());
        preimage.extend_from_array(&funding_hash.to_array());
        preimage.extend_from_array(&verification_hash.to_array());
        preimage.extend_from_array(&verification_links.len().to_be_bytes());
        for i in 0..verification_links.len() {
            if let Some(link) = verification_links.get(i) {
                preimage.extend_from_array(&link.to_array());
            }
        }
        env.crypto().sha256(&preimage)
    }

    fn request_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        actor: &Address,
        payload_hash: &BytesN<32>,
        tag: u32,
        version: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&request_id.to_array());
        preimage.extend_from_array(&payload_hash.to_array());
        preimage.append(&actor.clone().to_xdr(env));
        env.crypto().sha256(&preimage)
    }

    fn intent_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
        target_id: &BytesN<32>,
        actor: &Address,
        tag: u32,
        version: u32,
        payload_hash: &BytesN<32>,
        code: u32,
        aux: u64,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&code.to_be_bytes());
        preimage.extend_from_array(&aux.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&intent_id.to_array());
        preimage.extend_from_array(&target_id.to_array());
        preimage.extend_from_array(&payload_hash.to_array());
        preimage.append(&actor.clone().to_xdr(env));
        env.crypto().sha256(&preimage)
    }

    fn write<T>(env: &Env, key: &DataKey, value: &T)
    where
        T: IntoVal<Env, Val>,
    {
        env.storage().persistent().set(key, value);
        Self::extend(env, key);
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
