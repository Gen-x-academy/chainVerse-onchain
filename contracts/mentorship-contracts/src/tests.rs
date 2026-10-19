extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, ContractStatus, DateRange, GetMentorshipContractQuery,
    ListMentorshipContractsQuery, MentorshipContractsContract,
    MentorshipContractsContractClient, Money, PageRequest, PublishMentorshipCommand, SeatState,
    WaitlistPageRequest,
};

fn setup() -> (Env, MentorshipContractsContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(MentorshipContractsContract, ());
    let client = MentorshipContractsContractClient::new(&env, &id);
    (env, client)
}

fn command(
    env: &Env,
    agreement_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    content_hash: u8,
) -> PublishMentorshipCommand {
    PublishMentorshipCommand {
        agreement_id: agreement_id.clone(),
        schema_version: 1,
        mentor: mentor.clone(),
        mentee: mentee.clone(),
        fee: Money {
            asset: Address::generate(env),
            amount: 25_000_000,
        },
        session_count: 4,
        period: DateRange {
            starts_at: 1_000,
            ends_at: 2_000,
        },
        terms_hash: BytesN::from_array(env, &[content_hash; 32]),
        evidence: None,
    }
}

#[test]
fn publish_creates_immutable_versions_and_queries_latest() {
    let (env, client) = setup();
    let agreement_id = BytesN::from_array(&env, &[1; 32]);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);

    assert_eq!(
        client.publish(&command(&env, &agreement_id, &mentor, &mentee, 3)),
        1
    );
    assert_eq!(
        client.publish(&command(&env, &agreement_id, &mentor, &mentee, 4)),
        2
    );

    let latest = client.get_contract_version(&GetMentorshipContractQuery {
        agreement_id: agreement_id.clone(),
        version: None,
    });
    assert_eq!(latest.version, 2);
    assert_eq!(latest.terms_hash, BytesN::from_array(&env, &[4; 32]));
    assert_eq!(latest.status, ContractStatus::Published);

    let first = client.get_contract_version(&GetMentorshipContractQuery {
        agreement_id,
        version: Some(1),
    });
    assert_eq!(first.terms_hash, BytesN::from_array(&env, &[3; 32]));
}

#[test]
fn version_history_uses_exclusive_cursor_pagination() {
    let (env, client) = setup();
    let agreement_id = BytesN::from_array(&env, &[2; 32]);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    for digest in 1..=3 {
        client.publish(&command(&env, &agreement_id, &mentor, &mentee, digest));
    }

    let first_page = client.list_contract_versions(&ListMentorshipContractsQuery {
        agreement_id: agreement_id.clone(),
        page: PageRequest {
            after_version: 0,
            limit: 2,
        },
    });
    assert_eq!(first_page.versions.len(), 2);
    assert_eq!(first_page.next_cursor, Some(2));

    let second_page = client.list_contract_versions(&ListMentorshipContractsQuery {
        agreement_id,
        page: PageRequest {
            after_version: first_page.next_cursor.unwrap(),
            limit: 2,
        },
    });
    assert_eq!(second_page.versions.len(), 1);
    assert_eq!(second_page.versions.get(0).unwrap().version, 3);
    assert_eq!(second_page.next_cursor, None);
}

#[test]
fn publish_rejects_invalid_schema_terms_and_party_changes() {
    let (env, client) = setup();
    let agreement_id = BytesN::from_array(&env, &[3; 32]);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);

    let mut unsupported = command(&env, &agreement_id, &mentor, &mentee, 1);
    unsupported.schema_version = 2;
    assert_eq!(
        client.try_publish(&unsupported).unwrap_err().unwrap(),
        ContractError::UnsupportedSchemaVersion
    );

    let mut invalid_dates = command(&env, &agreement_id, &mentor, &mentee, 1);
    invalid_dates.period.ends_at = invalid_dates.period.starts_at;
    assert_eq!(
        client.try_publish(&invalid_dates).unwrap_err().unwrap(),
        ContractError::InvalidTerms
    );

    client.publish(&command(&env, &agreement_id, &mentor, &mentee, 1));
    let replacement_mentee = Address::generate(&env);
    assert_eq!(
        client
            .try_publish(&command(
                &env,
                &agreement_id,
                &mentor,
                &replacement_mentee,
                2,
            ))
            .unwrap_err()
            .unwrap(),
        ContractError::PartiesImmutable
    );
}

#[test]
fn pagination_rejects_invalid_limits_and_cursors() {
    let (env, client) = setup();
    let agreement_id = BytesN::from_array(&env, &[4; 32]);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    client.publish(&command(&env, &agreement_id, &mentor, &mentee, 1));

    let invalid_page = client
        .try_list_contract_versions(&ListMentorshipContractsQuery {
            agreement_id: agreement_id.clone(),
            page: PageRequest {
                after_version: 0,
                limit: 0,
            },
        })
        .unwrap_err()
        .unwrap();
    assert_eq!(invalid_page, ContractError::InvalidPage);

    let invalid_cursor = client
        .try_list_contract_versions(&ListMentorshipContractsQuery {
            agreement_id,
            page: PageRequest {
                after_version: 2,
                limit: 1,
            },
        })
        .unwrap_err()
        .unwrap();
    assert_eq!(invalid_cursor, ContractError::InvalidCursor);
}

#[test]
fn capacity_reserves_confirms_and_auto_promotes_fifo() {
    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[5; 32]);
    let organizer = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    client.create_program(&organizer, &program_id, &2, &100, &true);

    assert_eq!(client.request_seat(&first, &program_id), SeatState::Offered);
    assert_eq!(client.request_seat(&second, &program_id), SeatState::Offered);
    assert_eq!(client.request_seat(&third, &program_id), SeatState::Waitlisted);
    let capacity = client.get_program_capacity(&program_id);
    assert_eq!(capacity.reserved_seats, 2);
    assert_eq!(capacity.confirmed_seats, 0);

    let queue = client.get_waitlist_page(
        &program_id,
        &WaitlistPageRequest {
            after_sequence: None,
            limit: 10,
        },
    );
    assert_eq!(queue.participants.len(), 1);
    assert_eq!(queue.participants.get(0), Some(third.clone()));
    assert_eq!(queue.next_cursor, None);

    client.confirm_seat(&first, &program_id);
    client.confirm_seat(&second, &program_id);
    client.release_confirmed_seat(&first, &program_id);

    assert_eq!(
        client.get_seat_record(&program_id, &third).unwrap().state,
        SeatState::Offered
    );
    let capacity = client.get_program_capacity(&program_id);
    assert_eq!(capacity.reserved_seats, 1);
    assert_eq!(capacity.confirmed_seats, 1);
    assert_eq!(capacity.waitlist_head, capacity.waitlist_tail);
}

#[test]
fn expired_offer_releases_reservation_and_promotes_next_participant() {
    use soroban_sdk::testutils::Ledger as _;

    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[6; 32]);
    let organizer = Address::generate(&env);
    let offered = Address::generate(&env);
    let waiting = Address::generate(&env);
    client.create_program(&organizer, &program_id, &1, &10, &true);
    client.request_seat(&offered, &program_id);
    client.request_seat(&waiting, &program_id);
    env.ledger().set_timestamp(10);

    client.expire_offer(&program_id, &offered);
    let promoted = client.get_seat_record(&program_id, &waiting).unwrap();
    assert_eq!(promoted.state, SeatState::Offered);
    assert_eq!(promoted.offer_expires_at, Some(20));
    let capacity = client.get_program_capacity(&program_id);
    assert_eq!(capacity.reserved_seats, 1);
    assert_eq!(capacity.confirmed_seats, 0);
}

#[test]
fn disabled_auto_promotion_requires_organizer_and_preserves_order() {
    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[7; 32]);
    let organizer = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    client.create_program(&organizer, &program_id, &1, &100, &false);
    client.request_seat(&first, &program_id);
    client.request_seat(&second, &program_id);
    client.request_seat(&third, &program_id);
    client.confirm_seat(&first, &program_id);
    client.release_confirmed_seat(&first, &program_id);

    assert_eq!(
        client.get_seat_record(&program_id, &second).unwrap().state,
        SeatState::Waitlisted
    );
    let stranger = Address::generate(&env);
    assert_eq!(
        client
            .try_promote_next(&stranger, &program_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotProgramOrganizer
    );
    assert_eq!(client.promote_next(&organizer, &program_id), second);
    assert_eq!(
        client.get_seat_record(&program_id, &third).unwrap().state,
        SeatState::Waitlisted
    );
}

#[test]
fn waitlist_pages_follow_fifo_sequence() {
    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[9; 32]);
    let organizer = Address::generate(&env);
    let seat_holder = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    client.create_program(&organizer, &program_id, &1, &100, &false);
    client.request_seat(&seat_holder, &program_id);
    client.request_seat(&first, &program_id);
    client.request_seat(&second, &program_id);
    client.request_seat(&third, &program_id);

    let first_page = client.get_waitlist_page(
        &program_id,
        &WaitlistPageRequest {
            after_sequence: None,
            limit: 2,
        },
    );
    assert_eq!(first_page.participants.get(0), Some(first));
    assert_eq!(first_page.participants.get(1), Some(second));
    assert_eq!(first_page.next_cursor, Some(1));

    let next_page = client.get_waitlist_page(
        &program_id,
        &WaitlistPageRequest {
            after_sequence: first_page.next_cursor,
            limit: 2,
        },
    );
    assert_eq!(next_page.participants.get(0), Some(third));
    assert_eq!(next_page.next_cursor, None);
}

#[test]
fn increasing_capacity_automatically_offers_waitlisted_seats() {
    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[10; 32]);
    let organizer = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    let third = Address::generate(&env);
    client.create_program(&organizer, &program_id, &1, &100, &true);
    client.request_seat(&first, &program_id);
    client.request_seat(&second, &program_id);
    client.request_seat(&third, &program_id);

    client.configure_program(&organizer, &program_id, &3, &100, &true);
    assert_eq!(
        client.get_seat_record(&program_id, &second).unwrap().state,
        SeatState::Offered
    );
    assert_eq!(
        client.get_seat_record(&program_id, &third).unwrap().state,
        SeatState::Offered
    );
    let capacity = client.get_program_capacity(&program_id);
    assert_eq!(capacity.reserved_seats, 3);
    assert_eq!(capacity.waitlist_head, capacity.waitlist_tail);
}

#[test]
fn capacity_cannot_drop_below_reserved_or_confirmed_seats() {
    let (env, client) = setup();
    let program_id = BytesN::from_array(&env, &[8; 32]);
    let organizer = Address::generate(&env);
    let first = Address::generate(&env);
    let second = Address::generate(&env);
    client.create_program(&organizer, &program_id, &2, &100, &true);
    client.request_seat(&first, &program_id);
    client.request_seat(&second, &program_id);

    assert_eq!(
        client
            .try_configure_program(&organizer, &program_id, &0, &100, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidCapacity
    );
    assert_eq!(
        client
            .try_configure_program(&organizer, &program_id, &1, &100, &true)
            .unwrap_err()
            .unwrap(),
        ContractError::CapacityBelowCommitted
    );
}