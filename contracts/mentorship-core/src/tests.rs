#[cfg(test)]
mod tests {
    use crate::*;
    use soroban_sdk::{testutils::Address as _, BytesN, Env, String as SorobanString, Symbol};

    fn setup() -> (Env, Address, Address, Address) {
        let env = Env::default();
        let admin = Address::generate(&env);
        let mentor = Address::generate(&env);
        let mentee = Address::generate(&env);
        (env, admin, mentor, mentee)
    }

    fn program_id(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn match_id(env: &Env, byte: u8) -> BytesN<32> {
        let mut arr = [0u8; 32];
        arr[0] = byte;
        BytesN::from_array(env, &arr)
    }

    fn terms_hash(env: &Env, byte: u8) -> BytesN<32> {
        let mut arr = [0u8; 32];
        arr[1] = byte;
        BytesN::from_array(env, &arr)
    }

    #[test]
    fn test_initialize() {
        let (env, admin, _, _) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));

        assert!(client.initialize(&admin).is_ok());
        // Double initialize should fail
        assert_eq!(client.initialize(&admin), Err(Ok(ContractError::AlreadyInitialized)));
    }

    #[test]
    fn test_propose_requires_admin() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let non_admin = Address::generate(&env);
        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test Mentorship");

        let result = client.propose_mentorship(
            &non_admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        );

        assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
    }

    #[test]
    fn test_propose_mentorship_basic() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Mentoring Session");

        let result = client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            5000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        );

        assert!(result.is_ok());

        // Verify the mentorship was created
        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.mentor, mentor);
        assert_eq!(mentorship.mentee, mentee);
        assert_eq!(mentorship.amount, 5000);
        assert_eq!(mentorship.status, MentorshipStatus::Proposed);
        assert_eq!(mentorship.reserved, true);
    }

    #[test]
    fn test_propose_rejects_invalid_amount() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        let result = client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            0, // Invalid: zero amount
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        );

        assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
    }

    #[test]
    fn test_propose_rejects_invalid_dates() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // Start date in the past
        let result = client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now - 1000,
            now + 86400,
            &title,
        );

        assert_eq!(result, Err(Ok(ContractError::InvalidDates)));
    }

    #[test]
    fn test_propose_rejects_duplicate_id() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // First proposal succeeds
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Second proposal with same ID should fail
        let mentor2 = Address::generate(&env);
        let result = client.propose_mentorship(
            &admin,
            &m_id,
            &mentor2,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        );

        assert_eq!(result, Err(Ok(ContractError::MentorshipAlreadyExists)));
    }

    #[test]
    fn test_propose_rejects_conflicting_mentorship() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id1 = match_id(&env, 1);
        let m_id2 = match_id(&env, 2);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // First proposal with mentor-mentee pair succeeds
        assert!(client.propose_mentorship(
            &admin,
            &m_id1,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Second proposal with same mentor-mentee pair should fail
        let result = client.propose_mentorship(
            &admin,
            &m_id2,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        );

        assert_eq!(result, Err(Ok(ContractError::ConflictingMentorship)));
    }

    #[test]
    fn test_accept_as_mentee_requires_auth() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // Create proposal
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Try to accept without auth (mentee not authorizing)
        let wrong_address = Address::generate(&env);
        let decl_hash = terms_hash(&env, 2);
        let result = client.try_accept_as_mentee(&wrong_address, &m_id, &decl_hash);

        // Should fail due to lack of authorization
        assert!(result.is_err());
    }

    #[test]
    fn test_full_acceptance_workflow() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let decl_hash = terms_hash(&env, 2);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test Mentorship");

        // Propose
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            5000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::Proposed);

        // Mentee accepts
        assert!(client.accept_as_mentee(&mentee, &m_id, &decl_hash).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::ProposedMenteeAccepted);
        assert!(mentorship.mentee_accepted_at > 0);

        // Mentor accepts
        assert!(client.accept_as_mentor(&mentor, &m_id, &decl_hash).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::ProposedMentorAccepted);
        assert!(mentorship.mentor_accepted_at > 0);

        // Admin activates
        assert!(client.activate_mentorship(&admin, &m_id).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::Active);
        assert!(mentorship.activated_at > 0);
    }

    #[test]
    fn test_mentee_acceptance_records() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let decl_hash = terms_hash(&env, 2);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // Propose and accept
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        assert!(client.accept_as_mentee(&mentee, &m_id, &decl_hash).is_ok());

        // Verify acceptance record
        let acceptance = client.get_mentee_acceptance(&m_id).unwrap();
        assert_eq!(acceptance.signer, mentee);
        assert_eq!(acceptance.terms_version, 1);
        assert_eq!(acceptance.terms_hash, t_hash);
        assert_eq!(acceptance.declarations_hash, decl_hash);
    }

    #[test]
    fn test_reject_before_both_accept() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // Propose
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Mentee rejects
        assert!(client.reject_mentorship(&mentee, &m_id, 1).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::Rejected);
        assert_eq!(mentorship.reserved, false);

        // Verify active mentorship pointer was cleared
        assert!(!client.has_active_mentorship(&mentor, &mentee));
    }

    #[test]
    fn test_complete_mentorship() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id = match_id(&env, 1);
        let t_hash = terms_hash(&env, 1);
        let decl_hash = terms_hash(&env, 2);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        // Full workflow to Active
        assert!(client.propose_mentorship(
            &admin,
            &m_id,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        assert!(client.accept_as_mentee(&mentee, &m_id, &decl_hash).is_ok());
        assert!(client.accept_as_mentor(&mentor, &m_id, &decl_hash).is_ok());
        assert!(client.activate_mentorship(&admin, &m_id).is_ok());

        // Complete
        assert!(client.complete_mentorship(&admin, &m_id).is_ok());

        let mentorship = client.get_mentorship(&m_id).unwrap();
        assert_eq!(mentorship.status, MentorshipStatus::Completed);
        assert_eq!(mentorship.reserved, false);
    }

    #[test]
    fn test_mentor_and_mentee_indices() {
        let (env, admin, mentor, mentee) = setup();
        let client = MentorshipCoreContractClient::new(&env, &env.register_contract(None, MentorshipCoreContract));
        
        client.initialize(&admin).unwrap();

        let prog_id = program_id(&env);
        let m_id1 = match_id(&env, 1);
        let m_id2 = match_id(&env, 2);
        let t_hash = terms_hash(&env, 1);
        let now = env.ledger().timestamp();
        let title = SorobanString::from_slice(&env, b"Test");

        let mentor2 = Address::generate(&env);

        // Create first mentorship: mentor-mentee
        assert!(client.propose_mentorship(
            &admin,
            &m_id1,
            &mentor,
            &mentee,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Create second mentorship: mentor2-mentee
        let mentee2 = Address::generate(&env);
        assert!(client.propose_mentorship(
            &admin,
            &m_id2,
            &mentor2,
            &mentee2,
            &prog_id,
            1000,
            Symbol::new(&env, "CHV"),
            1,
            &t_hash,
            now + 86400,
            now + 86400 * 365,
            &title,
        ).is_ok());

        // Check indices
        let mentor_matches = client.get_mentor_matches(&mentor);
        assert_eq!(mentor_matches.len(), 1);
        assert_eq!(mentor_matches.get(0).unwrap(), m_id1);

        let mentor2_matches = client.get_mentor_matches(&mentor2);
        assert_eq!(mentor2_matches.len(), 1);
        assert_eq!(mentor2_matches.get(0).unwrap(), m_id2);

        let mentee_matches = client.get_mentee_matches(&mentee);
        assert_eq!(mentee_matches.len(), 1);
        assert_eq!(mentee_matches.get(0).unwrap(), m_id1);
    }
}
