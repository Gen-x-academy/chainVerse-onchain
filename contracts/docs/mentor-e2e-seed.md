# Deterministic Mentorship Development Seed

- Status: Implemented as a host-side test fixture
- Related issue: MENTOR-007 (Foundation track)

## Use

`mentor-e2e` is a test-only crate, patterned after `scholarship-e2e`. Run
`cargo test -p mentor-e2e` from `contracts/`. `fixture::World::seeded()` builds
a fixed-time Soroban test environment, synthetic actors, deterministic opaque
IDs and commitments, and provisions the existing expertise, profile, and
delivery contracts. It has no deployable contract or testnet/mainnet seeding
command.

## Scenario contents

The fixture contains two organizations, three mentors, four mentees, two
published programs (remote and hybrid), four applications, two matches, three
sessions, three milestones, three payment records, and one responded dispute.
The lifecycle includes submitted, matched, completed, held, released, and
disputed states with small deterministic test-token amounts.

Applications, matches, sessions, milestones, payments, and disputes are
synthetic host-side records, not canonical on-chain records: their production
domain contracts are not part of the current workspace. The fixture is
structured so those future contracts can consume the same IDs, links, status
cases, hashes, and amounts without changing the scenario.

## Privacy and repeatability

Actor addresses are generated inside a fresh Soroban test environment; no
real account/key or personal data is used. The fixture contains no names,
biographies, application text, exact locations, schedules, evidence files,
or dispute narratives. Such content is represented only by deterministic
synthetic hashes. The fixed ledger time and tagged ID derivation make all
non-address fixture values reproducible across test runs.

Do not turn this host fixture into a production seed transaction. A public
ledger cannot be reset, and deterministic public identities/commitments are
appropriate only for isolated development environments.