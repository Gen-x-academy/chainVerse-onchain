use mentor_e2e::fixture::{
    ApplicationStatus, DisputeStatus, MilestoneStatus, PaymentStatus, ProgramStatus,
    SessionStatus, World,
};

#[test]
fn deterministic_fixture_covers_a_realistic_mentorship_lifecycle() {
    let world = World::seeded();
    let scenario = &world.scenario;

    assert_eq!(scenario.organizations.len(), 2);
    assert_eq!(scenario.mentors.len(), 3);
    assert_eq!(scenario.mentees.len(), 4);
    assert_eq!(scenario.programs.len(), 2);
    assert_eq!(scenario.applications.len(), 4);
    assert_eq!(scenario.matches.len(), 2);
    assert_eq!(scenario.sessions.len(), 3);
    assert_eq!(scenario.milestones.len(), 3);
    assert_eq!(scenario.payments.len(), 3);
    assert_eq!(scenario.disputes.len(), 1);

    assert_eq!(scenario.programs[0].status, ProgramStatus::Published);
    assert_eq!(scenario.applications[0].status, ApplicationStatus::Matched);
    assert_eq!(scenario.applications[2].status, ApplicationStatus::Submitted);
    assert_eq!(scenario.sessions[0].status, SessionStatus::Completed);
    assert_eq!(scenario.milestones[0].status, MilestoneStatus::Paid);
    assert_eq!(scenario.payments[0].status, PaymentStatus::Released);
    assert_eq!(scenario.payments[1].status, PaymentStatus::Held);
    assert_eq!(scenario.disputes[0].status, DisputeStatus::Responded);
    assert!(scenario.milestones.iter().all(|milestone| milestone.amount > 0));
    assert!(scenario.disputes[0].disputed_amount > 0);
    assert!(scenario.programs.iter().all(|program| program.opens_at < program.closes_at));
    assert!(scenario.sessions[0].scheduled_at < scenario.sessions[1].scheduled_at);
    assert!(scenario.disputes[0].opened_at < scenario.disputes[0].response_deadline);

    for application in &scenario.applications {
        assert!(scenario.programs.iter().any(|p| p.id == application.program_id));
        assert!(scenario.mentees.iter().any(|m| m.id == application.mentee_id));
    }
    for mentorship_match in &scenario.matches {
        assert!(scenario.applications.iter().any(|a| a.id == mentorship_match.application_id));
        assert!(scenario.mentors.iter().any(|m| m.id == mentorship_match.mentor_id));
        assert!(scenario.mentees.iter().any(|m| m.id == mentorship_match.mentee_id));
    }
    for session in &scenario.sessions {
        assert!(scenario.matches.iter().any(|m| m.id == session.match_id));
    }
    for milestone in &scenario.milestones {
        assert!(scenario.matches.iter().any(|m| m.id == milestone.match_id));
    }
    for payment in &scenario.payments {
        let milestone = scenario
            .milestones
            .iter()
            .find(|m| m.id == payment.milestone_id)
            .expect("payments must refer to a seeded milestone");
        assert!(payment.amount <= milestone.amount);
    }
    let disputed_match = scenario
        .matches
        .iter()
        .find(|m| m.id == scenario.disputes[0].match_id)
        .expect("disputes must refer to a seeded match");
    assert!(scenario
        .milestones
        .iter()
        .filter(|m| m.match_id == disputed_match.id)
        .any(|m| scenario.disputes[0].disputed_amount <= m.amount));
}

#[test]
fn fixture_ids_and_contract_state_are_stable_for_the_fixed_seed() {
    let world = World::seeded();
    let first_program = &world.scenario.programs[0];
    let second_program = &world.scenario.programs[1];

    assert_eq!(world.expertise.skill_count(), 4);
    assert_eq!(world.profiles.get_profile(&world.scenario.mentors[0].account).version, 1);
    assert_eq!(
        world.delivery.get_config(&first_program.id).mode,
        first_program.mode
    );
    assert_eq!(
        world.delivery.get_config(&second_program.id).mode,
        second_program.mode
    );
    assert_eq!(first_program.id, world.scenario.applications[0].program_id);
    assert_eq!(
        world.scenario.matches[0].application_id,
        world.scenario.applications[0].id
    );
    assert_eq!(
        world.scenario.payments[0].milestone_id,
        world.scenario.milestones[0].id
    );
    let repeat = mentor_e2e::fixture::Scenario::seed(&world.env, &world.actors);
    assert_eq!(world.scenario, repeat);
}