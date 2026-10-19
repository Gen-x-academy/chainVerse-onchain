use crate::{ContractError, MentorshipLifecycle, MentorshipStatus};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup(env: &Env) -> (Address, Address, Address, BytesN<32>, Address) {
    let contract_id = env.register(MentorshipLifecycle, ());
    let admin = Address::generate(env);
    let mentor = Address::generate(env);
    let mentee = Address::generate(env);
    let id = BytesN::from_array(env, &[7; 32]);
    let commitment = BytesN::from_array(env, &[9; 32]);
    let client = crate::MentorshipLifecycleClient::new(env, &contract_id);
    client.initialize(&admin).unwrap();
    client
        .create_mentorship(&mentor, &id, &commitment)
        .unwrap();
    (admin, mentor, mentee, id, contract_id)
}

#[test]
fn full_lifecycle_is_guarded_and_records_public_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, mentor, mentee, id, contract_id) = setup(&env);
    let client = crate::MentorshipLifecycleClient::new(&env, &contract_id);

    client.transition(&mentor, &id, &MentorshipStatus::Published);
    client.transition(&mentor, &id, &MentorshipStatus::Accepting);
    client.match_mentorship(&mentor, &id, &mentee);
    client.transition(&mentor, &id, &MentorshipStatus::Active);
    client.transition(&mentee, &id, &MentorshipStatus::Paused);
    client.transition(&mentor, &id, &MentorshipStatus::Active);
    client.transition(&mentee, &id, &MentorshipStatus::Disputed);
    client.transition(&admin, &id, &MentorshipStatus::Completed);
    client.transition(&admin, &id, &MentorshipStatus::Archived);

    let record = client.get_mentorship(&id).unwrap();
    assert_eq!(record.status, MentorshipStatus::Archived);
    assert_eq!(record.mentee, Some(mentee));
    assert_eq!(record.commitment_hash, BytesN::from_array(&env, &[9; 32]));
}

#[test]
fn illegal_transition_does_not_change_state() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, mentor, _mentee, id, contract_id) = setup(&env);
    let client = crate::MentorshipLifecycleClient::new(&env, &contract_id);

    let result = client.try_transition(&mentor, &id, &MentorshipStatus::Active);
    assert_eq!(result, Err(Ok(ContractError::InvalidTransition)));
    assert_eq!(client.get_mentorship(&id).unwrap().status, MentorshipStatus::Draft);
}

#[test]
fn non_participant_cannot_publish() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _mentor, _mentee, id, contract_id) = setup(&env);
    let client = crate::MentorshipLifecycleClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    let result = client.try_transition(&stranger, &id, &MentorshipStatus::Published);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}