//! Exact Soroban authorization-tree assertions (#1012).
//!
//! Every other suite in this crate calls `env.mock_all_auths()`, which
//! authorizes *whatever* the contract asks for. That proves a call succeeds; it
//! proves nothing about **who** had to sign, with **what arguments**, or that a
//! signature scoped to one call cannot be replayed against another.
//!
//! These tests never call `mock_all_auths`. Each one supplies exactly one
//! [`MockAuth`] and then asserts the tree the host actually recorded via
//! `env.auths()` — address, contract, function name, and the argument vector
//! the proof is bound to.
//!
//! ## Coverage per privileged entrypoint
//!
//! | Case          | What it proves                                              |
//! |---------------|-------------------------------------------------------------|
//! | success       | The exact `(address, contract, fn, args)` tree is required   |
//! | missing auth  | The call fails when no proof is supplied                     |
//! | wrong actor   | Another identity's proof does not satisfy the requirement    |
//! | cross-scope   | A proof for one function or argument set does not carry over |
//!
//! ## A note on ordering
//!
//! `require_admin`, `require_capability`, and the inline licensee check in
//! `allocate_seat`/`release_seat` all compare identity **before** calling
//! `require_auth()`. A wrong actor is therefore rejected without any auth
//! requirement being recorded at all — asserted below, because it means an
//! attacker cannot induce the contract to demand a signature from a third
//! party.
//!
//! ## Impact
//!
//! Test-only. No ABI, storage, event, privacy, deployment, or migration impact.

use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, Ledger as _, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, String, Symbol,
};

use crate::{AccessMode, LibraryLicensing, LibraryLicensingClient, LicenseError};

// ── Helpers ────────────────────────────────────────────────────────────────

/// A fresh environment with **no** blanket auth mocking.
fn fresh() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryLicensing, ());
    (env, contract_id)
}

fn work_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[9u8; 32])
}

/// Asserts the host recorded exactly one requirement, from `address`, for
/// `contract::fn_name(args)`, with no sub-invocations.
fn assert_single_auth(
    env: &Env,
    address: &Address,
    contract: &Address,
    fn_name: &str,
    args: soroban_sdk::Vec<soroban_sdk::Val>,
) {
    let auths = env.auths();
    assert_eq!(
        auths.len(),
        1,
        "expected exactly one authorization requirement, got {}",
        auths.len()
    );

    let (recorded_address, invocation) = &auths[0];
    assert_eq!(recorded_address, address, "authorization came from the wrong address");
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((contract.clone(), Symbol::new(env, fn_name), args)),
        "authorization tree does not match the expected contract/function/args"
    );
    assert!(
        invocation.sub_invocations.is_empty(),
        "unexpected sub-invocations in the authorization tree"
    );
}

/// Asserts the host recorded no authorization requirement at all.
fn assert_no_auth(env: &Env) {
    assert!(
        env.auths().is_empty(),
        "the call demanded authorization when it should have been rejected on identity alone"
    );
}

/// Initialises the admin under an exact proof and returns it.
fn init_admin(env: &Env, contract_id: &Address) -> Address {
    let admin = Address::generate(env);
    let client = LibraryLicensingClient::new(env, contract_id);
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: contract_id,
            fn_name: "set_admin",
            args: (admin.clone(),).into_val(env),
            sub_invokes: &[],
        },
    }]);
    client.set_admin(&admin);
    admin
}

/// Grants a license to `licensee` under an exact admin proof.
fn grant(
    env: &Env,
    contract_id: &Address,
    admin: &Address,
    licensee: &Address,
    total_seats: u32,
) -> BytesN<32> {
    let client = LibraryLicensingClient::new(env, contract_id);
    let rights = String::from_str(env, "read");
    let args = (
        admin.clone(),
        work_id(env),
        licensee.clone(),
        rights.clone(),
        1000u64,
        2000u64,
        total_seats,
    );
    env.mock_auths(&[MockAuth {
        address: admin,
        invoke: &MockAuthInvoke {
            contract: contract_id,
            fn_name: "grant_license",
            args: args.clone().into_val(env),
            sub_invokes: &[],
        },
    }]);
    client.grant_license(admin, &work_id(env), licensee, &rights, &1000, &2000, &total_seats)
}

// ═══════════════════════════════════════════════════════════════════════════
// set_admin
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn set_admin_requires_exactly_the_new_admins_signature() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);

    assert_single_auth(
        &env,
        &admin,
        &contract_id,
        "set_admin",
        (admin.clone(),).into_val(&env),
    );
}

#[test]
fn set_admin_fails_without_any_authorization() {
    let (env, contract_id) = fresh();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    env.mock_auths(&[]);

    assert!(
        client.try_set_admin(&admin).is_err(),
        "set_admin succeeded with no authorization supplied"
    );
}

#[test]
fn set_admin_is_not_satisfied_by_another_addresss_signature() {
    let (env, contract_id) = fresh();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    // A well-formed proof — for the wrong address.
    env.mock_auths(&[MockAuth {
        address: &impostor,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_admin",
            args: (admin.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_set_admin(&admin).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// grant_license — admin / Cataloger capability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn grant_license_requires_the_admins_signature_over_every_argument() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);

    grant(&env, &contract_id, &admin, &licensee, 3);

    assert_single_auth(
        &env,
        &admin,
        &contract_id,
        "grant_license",
        (
            admin.clone(),
            work_id(&env),
            licensee.clone(),
            String::from_str(&env, "read"),
            1000u64,
            2000u64,
            3u32,
        )
            .into_val(&env),
    );
}

#[test]
fn grant_license_fails_without_any_authorization() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);

    env.mock_auths(&[]);

    assert!(client
        .try_grant_license(
            &admin,
            &work_id(&env),
            &licensee,
            &String::from_str(&env, "read"),
            &1000,
            &2000,
            &3
        )
        .is_err());
}

#[test]
fn grant_license_by_a_non_admin_is_rejected_before_any_signature_is_demanded() {
    let (env, contract_id) = fresh();
    let _admin = init_admin(&env, &contract_id);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);

    // The stranger is perfectly willing to sign — and it does not help.
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "grant_license",
            args: (
                stranger.clone(),
                work_id(&env),
                licensee.clone(),
                String::from_str(&env, "read"),
                1000u64,
                2000u64,
                3u32,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client
        .try_grant_license(
            &stranger,
            &work_id(&env),
            &licensee,
            &String::from_str(&env, "read"),
            &1000,
            &2000,
            &3
        )
        .is_err());
    assert_no_auth(&env);
}

// ═══════════════════════════════════════════════════════════════════════════
// allocate_seat / release_seat — licensee only
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn allocate_seat_requires_the_licensees_signature_bound_to_the_license() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (licensee.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.allocate_seat(&licensee, &license_id);

    assert_single_auth(
        &env,
        &licensee,
        &contract_id,
        "allocate_seat",
        (licensee.clone(), license_id.clone()).into_val(&env),
    );
}

#[test]
fn allocate_seat_fails_without_any_authorization() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    assert!(client.try_allocate_seat(&licensee, &license_id).is_err());
    assert_eq!(client.license(&license_id).allocated_seats, 0);
}

#[test]
fn allocate_seat_by_a_non_licensee_is_rejected_before_any_signature_is_demanded() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (stranger.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert_eq!(
        client.try_allocate_seat(&stranger, &license_id),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_no_auth(&env);
    assert_eq!(client.license(&license_id).allocated_seats, 0);
}

#[test]
fn even_the_admin_cannot_allocate_a_seat_on_someone_elses_license() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (admin.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert_eq!(
        client.try_allocate_seat(&admin, &license_id),
        Err(Ok(LicenseError::Unauthorized))
    );
}

#[test]
fn release_seat_requires_the_licensees_signature_bound_to_the_license() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (licensee.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.allocate_seat(&licensee, &license_id);

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "release_seat",
            args: (licensee.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.release_seat(&licensee, &license_id);

    assert_single_auth(
        &env,
        &licensee,
        &contract_id,
        "release_seat",
        (licensee.clone(), license_id.clone()).into_val(&env),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Cross-scope — a proof must not carry beyond the call it was issued for
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_proof_for_allocate_seat_does_not_authorize_release_seat() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (licensee.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.allocate_seat(&licensee, &license_id);

    // The same proof is still the only one on offer; release must not accept it.
    assert!(client.try_release_seat(&licensee, &license_id).is_err());
    assert_eq!(client.license(&license_id).allocated_seats, 1);
}

#[test]
fn a_proof_bound_to_one_license_does_not_authorize_another() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let first = grant(&env, &contract_id, &admin, &licensee, 3);

    // A second license for the same licensee, on a different work.
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let other_work = BytesN::from_array(&env, &[7u8; 32]);
    let rights = String::from_str(&env, "read");
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "grant_license",
            args: (
                admin.clone(),
                other_work.clone(),
                licensee.clone(),
                rights.clone(),
                1000u64,
                2000u64,
                3u32,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let second =
        client.grant_license(&admin, &other_work, &licensee, &rights, &1000, &2000, &3);

    // Proof names the first license; the call targets the second.
    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "allocate_seat",
            args: (licensee.clone(), first.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_allocate_seat(&licensee, &second).is_err());
    assert_eq!(client.license(&second).allocated_seats, 0);
}

#[test]
fn a_proof_for_a_different_contract_does_not_authorize_this_one() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    // A second, unrelated deployment of the same contract.
    let other_deployment = env.register(LibraryLicensing, ());

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &other_deployment,
            fn_name: "allocate_seat",
            args: (licensee.clone(), license_id.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_allocate_seat(&licensee, &license_id).is_err());
    assert_eq!(client.license(&license_id).allocated_seats, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// grant_entitlement — admin only
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn grant_entitlement_requires_the_admins_signature_over_every_argument() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let rendition = Symbol::new(&env, "epub");

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "grant_entitlement",
            args: (
                admin.clone(),
                license_id.clone(),
                rendition.clone(),
                AccessMode::Borrow,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.grant_entitlement(&admin, &license_id, &rendition, &AccessMode::Borrow);

    assert_single_auth(
        &env,
        &admin,
        &contract_id,
        "grant_entitlement",
        (
            admin.clone(),
            license_id.clone(),
            rendition.clone(),
            AccessMode::Borrow,
        )
            .into_val(&env),
    );
}

#[test]
fn grant_entitlement_by_the_licensee_is_rejected_before_any_signature_is_demanded() {
    let (env, contract_id) = fresh();
    let admin = init_admin(&env, &contract_id);
    let licensee = Address::generate(&env);
    env.ledger().set_timestamp(1500);
    let license_id = grant(&env, &contract_id, &admin, &licensee, 3);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let rendition = Symbol::new(&env, "epub");

    env.mock_auths(&[MockAuth {
        address: &licensee,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "grant_entitlement",
            args: (
                licensee.clone(),
                license_id.clone(),
                rendition.clone(),
                AccessMode::Borrow,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert_eq!(
        client.try_grant_entitlement(&licensee, &license_id, &rendition, &AccessMode::Borrow),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_no_auth(&env);
}
