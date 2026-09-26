//! A named cast of actors plus all five scholarship contracts on one ledger.

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env, Symbol};

use scholarship_applications::ScholarshipApplicationsContractClient;
use scholarship_core::ScholarshipCoreContractClient;
use scholarship_eligibility::ScholarshipEligibilityContractClient;
use scholarship_programs::ScholarshipProgramsContractClient;
use scholarship_registry::{Role, ScholarshipRegistryContractClient};

/// Module names as they are registered in `scholarship-registry`. Kept as
/// constants so a journey and a load test resolve the same names, and so a
/// rename shows up as one compile error rather than a silent lookup miss.
pub mod module {
    use soroban_sdk::Env;
    use soroban_sdk::Symbol;

    pub fn registry(env: &Env) -> Symbol {
        Symbol::new(env, "registry")
    }
    pub fn core(env: &Env) -> Symbol {
        Symbol::new(env, "core")
    }
    pub fn programs(env: &Env) -> Symbol {
        Symbol::new(env, "programs")
    }
    pub fn eligibility(env: &Env) -> Symbol {
        Symbol::new(env, "eligibility")
    }
    pub fn applications(env: &Env) -> Symbol {
        Symbol::new(env, "applications")
    }
}

/// The actors from ADR 0002's table, as a struct so a test can talk about
/// `world.sponsor` rather than `actors.1`, and so a missing actor is a
/// compile error instead of an `Address::generate(&env)` buried mid-test.
pub struct Cast {
    /// The address that called `initialize` on all five contracts. Today
    /// each contract distinguishes only "this admin" and "everyone else"
    /// (ADR 0002, Actors), so one address fills that role everywhere.
    pub admin: Address,
    /// Owns programs and holds the `Sponsor` role.
    pub sponsor: Address,
    /// Holds the `Reviewer` role.
    pub reviewer: Address,
    /// Holds the `Finance` role.
    pub finance: Address,
    /// Holds the `Student` role.
    pub student: Address,
    /// An issuer allowlisted to vouch for eligibility attestations.
    pub registrar: Address,
    /// Never granted any role. Used to prove that "not on the allowlist"
    /// is a rejection, not a silent no-op.
    pub stranger: Address,
    /// Deterministic addresses for load/property tests, so a failure can be
    /// reproduced from a seed without depending on generation order.
    pub students: soroban_sdk::Vec<Address>,
    pub reviewers: soroban_sdk::Vec<Address>,
}

/// Every contract in the epic, registered on one `Env`, plus the typed
/// clients a test drives them through.
pub struct World {
    pub env: Env,
    pub actors: Cast,
    pub core_id: Address,
    pub programs_id: Address,
    pub eligibility_id: Address,
    pub applications_id: Address,
    pub registry_id: Address,
    pub core: ScholarshipCoreContractClient<'static>,
    pub programs: ScholarshipProgramsContractClient<'static>,
    pub eligibility: ScholarshipEligibilityContractClient<'static>,
    pub applications: ScholarshipApplicationsContractClient<'static>,
    pub registry: ScholarshipRegistryContractClient<'static>,
}

impl World {
    /// Registers all five contracts, initializes them against one admin,
    /// registers each in the module registry, and grants the ADR 0002
    /// roles. `mock_all_auths` is on by default: authorization *trees* are
    /// asserted separately (see `tests/security.rs`), and blanket mocking
    /// here keeps journey tests about state transitions rather than about
    /// re-deriving the auth fixtures for every call.
    pub fn new() -> World {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);

        let core_id = env.register(scholarship_core::ScholarshipCoreContract, ());
        let programs_id = env.register(scholarship_programs::ScholarshipProgramsContract, ());
        let eligibility_id =
            env.register(scholarship_eligibility::ScholarshipEligibilityContract, ());
        let applications_id = env.register(
            scholarship_applications::ScholarshipApplicationsContract,
            (),
        );
        let registry_id = env.register(scholarship_registry::ScholarshipRegistryContract, ());

        let admin = Address::generate(&env);
        let sponsor = Address::generate(&env);
        let reviewer = Address::generate(&env);
        let finance = Address::generate(&env);
        let student = Address::generate(&env);
        let registrar = Address::generate(&env);
        let stranger = Address::generate(&env);

        let mut students = soroban_sdk::Vec::new(&env);
        for _ in 0..8 {
            students.push_back(Address::generate(&env));
        }
        let mut reviewers = soroban_sdk::Vec::new(&env);
        for _ in 0..4 {
            reviewers.push_back(Address::generate(&env));
        }

        // Clients own a clone of `Env`, so they are built before `env` is
        // moved into the struct.
        let core = ScholarshipCoreContractClient::new(&env, &core_id);
        let programs = ScholarshipProgramsContractClient::new(&env, &programs_id);
        let eligibility = ScholarshipEligibilityContractClient::new(&env, &eligibility_id);
        let applications = ScholarshipApplicationsContractClient::new(&env, &applications_id);
        let registry = ScholarshipRegistryContractClient::new(&env, &registry_id);

        let world = World {
            core_id,
            programs_id,
            eligibility_id,
            applications_id,
            registry_id,
            core,
            programs,
            eligibility,
            applications,
            registry,
            env,
            actors: Cast {
                admin,
                sponsor,
                reviewer,
                finance,
                student,
                registrar,
                stranger,
                students,
                reviewers,
            },
        };
        world.initialize();
        world
    }

    fn initialize(&self) {
        let admin = &self.actors.admin;
        self.core.initialize(admin);
        self.programs.initialize(admin);
        self.eligibility.initialize(admin);
        self.applications.initialize(admin);
        self.registry.initialize(admin);

        for (name, id) in [
            (module::registry(&self.env), &self.registry_id),
            (module::core(&self.env), &self.core_id),
            (module::programs(&self.env), &self.programs_id),
            (module::eligibility(&self.env), &self.eligibility_id),
            (module::applications(&self.env), &self.applications_id),
        ] {
            self.registry.register_module(admin, &name, id);
        }

        for (account, role) in [
            (&self.actors.sponsor, Role::Sponsor),
            (&self.actors.reviewer, Role::Reviewer),
            (&self.actors.finance, Role::Finance),
            (&self.actors.student, Role::Student),
            (&self.actors.admin, Role::Administrator),
        ] {
            self.registry.grant_role(admin, account, &role);
        }

        // The registrar is the only non-admin actor trusted to vouch for
        // eligibility; the stranger deliberately is not.
        self.eligibility.add_issuer(admin, &self.actors.registrar);
    }

    /// A deterministic 32-byte program identifier derived from a `u8`
    /// seed, so a load test can name program #17 and get the same bytes on
    /// every run.
    pub fn program_id(seed: u8) -> BytesN<32> {
        BytesN::from_array(&soroban_sdk::Env::default(), &[seed; 32])
    }

    /// Program identifiers must be built against *this* world's `Env`, not
    /// a throwaway one — `BytesN` values carry no environment, but building
    /// them from a different `Env` is a trap worth naming once.
    pub fn seeded_id(env: &Env, seed: u8) -> BytesN<32> {
        let _ = env;
        BytesN::from_array(env, &[seed; 32])
    }

    /// Publishes a sponsor-owned program and brings it all the way to
    /// `Published`, with a published eligibility rule and a reserved award
    /// budget. Returns the program id.
    pub fn publish_program(&self, seed: u8, title: &str) -> BytesN<32> {
        self.publish_program_at(seed, title, self.env.ledger().timestamp())
    }

    /// As [`World::publish_program`], but with an explicit window so tests
    /// can place submissions relative to a deadline rather than to "now".
    pub fn publish_program_at(&self, seed: u8, title: &str, opens_at: u64) -> BytesN<32> {
        let env = &self.env;
        let admin = &self.actors.admin;
        let pid = BytesN::from_array(env, &[seed; 32]);

        self.core.create_program(
            &self.actors.sponsor,
            &pid,
            &BytesN::from_array(env, &[seed.wrapping_add(200); 32]),
            &soroban_sdk::String::from_str(env, title),
            &soroban_sdk::String::from_str(env, "Seeded by the scholarship test harness."),
            &Symbol::new(env, "USDC"),
            &scholarship_core::FundingModel::FixedAward,
        );
        self.core.transition_program(
            &self.actors.sponsor,
            &pid,
            &scholarship_core::ProgramStatus::Published,
        );

        let closes_at = opens_at + 100_000;
        self.programs
            .set_program_window(admin, &pid, &opens_at, &closes_at, &0, &0);
        self.programs
            .configure_award_budget(admin, &pid, &4, &1_000, &4_000);
        self.applications.register_program(admin, &pid, &closes_at);
        self.applications
            .publish_form_schema(admin, &pid, &BytesN::from_array(env, &[seed; 32]));
        self.applications.publish_consent_terms(
            admin,
            &pid,
            &BytesN::from_array(env, &[seed.wrapping_add(9); 32]),
        );

        let mut required = soroban_sdk::Vec::new(env);
        required.push_back(self.attestation_type());
        self.eligibility
            .publish_eligibility_rule(admin, &pid, &required);

        pid
    }

    /// A synthetic attestation type name. Attestation types are `Symbol`s
    /// chosen by the program, not a closed enum in the contract, so tests
    /// use one stable name.
    /// Turn authorization mocking off for this world, *after* setup has
    /// run. `set_auths(&[])` disables recording, so from this point on
    /// every `require_auth` in a contract is enforced for real and a call
    /// on behalf of an address that did not sign fails.
    ///
    /// This is what makes the wallet-substitution and
    /// privilege-escalation abuse cases in `tests/security.rs` meaningful:
    /// under `mock_all_auths` every one of those attacks would succeed.
    pub fn seal(&self) {
        self.env.set_auths(&[]);
    }

    pub fn attestation_type(&self) -> Symbol {
        Symbol::new(&self.env, "enrolled")
    }

    /// Vouches for `subject` under the program-scoped attestation type, with
    /// `valid_for` seconds of remaining validity.
    pub fn attest(&self, subject: &Address, pid: &BytesN<32>, valid_for: u64) {
        let expiry = self.env.ledger().timestamp() + valid_for;
        self.eligibility.issue_attestation(
            &self.actors.registrar,
            subject,
            pid,
            &self.attestation_type(),
            &expiry,
        );
    }

    /// The happy-path preconditions for a submission: eligible, consented,
    /// inside the window.
    pub fn prepare_student(&self, student: &Address, pid: &BytesN<32>, valid_for: u64) {
        self.attest(student, pid, valid_for);
        self.applications.record_consent(student, pid);
    }

    /// Every contract's reported schema version, as a map a test can assert
    /// against. Used by the journey tests to prove a deployment under test
    /// is the one the suite was written for.
    pub fn versions(&self) -> Vec<(&'static str, u32)> {
        vec![
            ("registry", self.registry.version()),
            ("core", self.core.version()),
            ("programs", self.programs.version()),
            ("eligibility", self.eligibility.version()),
            ("applications", self.applications.version()),
        ]
    }
}

impl Default for World {
    fn default() -> Self {
        World::new()
    }
}
