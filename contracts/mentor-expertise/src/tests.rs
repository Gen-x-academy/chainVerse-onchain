extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol};

use crate::{
    AliasStatus, ContractError, MentorExpertiseContract, MentorExpertiseContractClient,
    ProficiencyLevel, SkillStatus,
};

fn setup() -> (Env, Address, Address, MentorExpertiseContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorExpertiseContract, ());
    let admin = Address::generate(&env);
    let client = MentorExpertiseContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin, client)
}

fn id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn taxonomy_hierarchy_alias_and_bounded_listing_work() {
    let (env, _, admin, client) = setup();
    let root = id(&env, 1);
    let child = id(&env, 2);
    client.register_skill(&admin, &root, &None, &Symbol::new(&env, "programming"));
    client.register_skill(
        &admin,
        &child,
        &Some(root.clone()),
        &Symbol::new(&env, "rust"),
    );
    client.register_alias(&admin, &Symbol::new(&env, "rust_lang"), &child);

    assert_eq!(client.resolve_alias(&Symbol::new(&env, "rust_lang")), child);
    assert_eq!(client.get_skill(&child).parent, Some(root));
    assert_eq!(client.skill_count(), 2);
    assert_eq!(client.list_skills(&0, &50).len(), 2);
    assert_eq!(
        client.try_list_skills(&0, &51),
        Err(Ok(ContractError::InvalidLimit))
    );
}

#[test]
fn taxonomy_writes_are_admin_only_and_parent_must_exist() {
    let (env, _, admin, client) = setup();
    let stranger = Address::generate(&env);
    let new_id = id(&env, 1);
    assert_eq!(
        client.try_register_skill(
            &stranger,
            &new_id,
            &None,
            &Symbol::new(&env, "unauthorized")
        ),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_register_skill(
            &admin,
            &new_id,
            &Some(id(&env, 9)),
            &Symbol::new(&env, "orphan")
        ),
        Err(Ok(ContractError::InvalidParent))
    );
}

#[test]
fn hierarchy_depth_is_bounded_including_the_new_skill() {
    let (env, _, admin, client) = setup();
    let mut parent = None;
    for byte in 1..=8 {
        let skill_id = id(&env, byte);
        client.register_skill(
            &admin,
            &skill_id,
            &parent,
            &Symbol::new(&env, "nested_skill"),
        );
        parent = Some(skill_id);
    }

    assert_eq!(
        client.try_register_skill(
            &admin,
            &id(&env, 9),
            &parent,
            &Symbol::new(&env, "too_deep")
        ),
        Err(Ok(ContractError::HierarchyTooDeep))
    );
    assert_eq!(client.skill_count(), 8);
}

#[test]
fn evidence_requirements_are_hash_only_and_append_only() {
    let (env, _, admin, client) = setup();
    let skill_id = id(&env, 3);
    client.register_skill(
        &admin,
        &skill_id,
        &None,
        &Symbol::new(&env, "security"),
    );
    let first = client.publish_evidence_requirement(
        &admin,
        &skill_id,
        &ProficiencyLevel::Proficient,
        &id(&env, 4),
        &1,
    );
    let second = client.publish_evidence_requirement(
        &admin,
        &skill_id,
        &ProficiencyLevel::Proficient,
        &id(&env, 5),
        &2,
    );

    assert_eq!((first, second), (1, 2));
    assert_eq!(
        client
            .get_evidence_requirement_version(&skill_id, &ProficiencyLevel::Proficient, &1)
            .requirement_hash,
        id(&env, 4)
    );
    assert_eq!(
        client
            .get_evidence_requirement(&skill_id, &ProficiencyLevel::Proficient)
            .requirement_hash,
        id(&env, 5)
    );
    assert_eq!(
        client.try_publish_evidence_requirement(
            &admin,
            &skill_id,
            &ProficiencyLevel::Expert,
            &id(&env, 6),
            &0,
        ),
        Err(Ok(ContractError::InvalidSchemaVersion))
    );
}

#[test]
fn deprecation_preserves_terms_but_blocks_new_use() {
    let (env, _, admin, client) = setup();
    let old_id = id(&env, 7);
    let replacement_id = id(&env, 8);
    client.register_skill(
        &admin,
        &old_id,
        &None,
        &Symbol::new(&env, "old_skill"),
    );
    client.register_skill(
        &admin,
        &replacement_id,
        &None,
        &Symbol::new(&env, "new_skill"),
    );
    let alias = Symbol::new(&env, "legacy_skill");
    client.register_alias(&admin, &alias, &old_id);
    client.deprecate_skill(&admin, &old_id, &Some(replacement_id.clone()));

    let retired = client.get_skill(&old_id);
    assert_eq!(retired.status, SkillStatus::Deprecated);
    assert_eq!(retired.replacement, Some(replacement_id));
    assert_eq!(
        client.try_resolve_alias(&alias),
        Err(Ok(ContractError::SkillDeprecated))
    );
    assert_eq!(
        client.try_register_alias(&admin, &Symbol::new(&env, "new_alias"), &old_id),
        Err(Ok(ContractError::SkillDeprecated))
    );
}

#[test]
fn aliases_cannot_be_reused_after_deprecation() {
    let (env, _, admin, client) = setup();
    let skill_id = id(&env, 10);
    let alias = Symbol::new(&env, "old_name");
    client.register_skill(&admin, &skill_id, &None, &Symbol::new(&env, "current"));
    client.register_alias(&admin, &alias, &skill_id);
    client.deprecate_alias(&admin, &alias);

    assert_eq!(
        client.try_resolve_alias(&alias),
        Err(Ok(ContractError::AliasNotFound))
    );
    assert_eq!(client.get_alias(&alias).status, AliasStatus::Deprecated);
    assert_eq!(
        client.try_register_alias(&admin, &alias, &skill_id),
        Err(Ok(ContractError::AliasAlreadyExists))
    );
    assert_eq!(client.get_skill(&skill_id).status, SkillStatus::Active);
}