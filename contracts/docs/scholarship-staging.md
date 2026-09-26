# Scholarship staging seed flow

How to provision, verify, and reverse a complete synthetic scholarship
scenario on testnet.

The implementation is [`scripts/seed-scholarship-staging.sh`](../../scripts/seed-scholarship-staging.sh).
The one genuinely off-chain step it depends on is
[`scripts/scholarship_wallet_signer.py`](../../scripts/scholarship_wallet_signer.py),
covered in [The signing boundary](#the-signing-boundary).

For what the contracts themselves do, see
[scholarship-guide.md](scholarship-guide.md). This document is about running
them.

## Quick start

```bash
./scripts/seed-scholarship-staging.sh plan      # print the scenario; sends nothing
./scripts/seed-scholarship-staging.sh apply     # provision; safe to re-run
./scripts/seed-scholarship-staging.sh verify    # read state back and assert
./scripts/seed-scholarship-staging.sh teardown  # reverse
```

`apply` refuses to run against a mainnet passphrase, and refuses to run at all
if the wallet signer cannot prove itself against the RFC 8032 test vectors.
Neither check has an override.

### Prerequisites

- Stellar CLI, `jq`, `python3`
- The seven scholarship contracts deployed, with IDs discoverable either as
  `SCHOLARSHIP_<NAME>_ID` environment variables or as `scholarship_<name>`
  keys in `deployments/testnet.json`

> The scholarship contracts are **not** yet written into
> `scripts/deploy-testnet.sh`, and do not appear in the committed manifest.
> `apply` fails with the specific missing name rather than a generic error.
> Adding them to the deploy script is the natural follow-up and is tracked as a
> known gap in [Known limits](#known-limits).

## What gets provisioned

Eleven steps, in dependency order:

| # | Contract | Actions |
| --- | --- | --- |
| 1 | `scholarship-registry` | `initialize`, `register_module` ×7 |
| 2 | `scholarship-registry` | `grant_role` for all five roles |
| 3 | all seven | `initialize` with the admin address |
| 4 | `scholarship-core` | `create_program`, `transition_program(Published)` |
| 5 | `scholarship-programs` | `set_program_window`, `configure_award_budget` |
| 6 | `scholarship-eligibility` | `publish_eligibility_rule`, `add_issuer`, `issue_attestation` |
| 7 | `scholarship-applications` | `register_program`, `set_program_active`, `publish_form_schema`, `publish_consent_terms` |
| 8 | `scholarship-applications` | `record_consent`, `submit_application` |
| 9 | `scholarship-applications` | `register_reviewer`, `set_review_mode`, `assign_reviewer` |
| 10 | `scholarship-milestones` | `create_milestone`, `submit_evidence`, `verify_evidence` |
| 11 | `scholarship-disbursements` | `add_creator`, wallet challenge, `create_intent` |

The scenario deliberately exercises a **real** decision point where one exists:
`verify_evidence` with `Approved` / `MeetsCriteria` is recorded on-chain and can
be read back.

It deliberately does **not** simulate an application review decision, because
no such function exists. `verify` says so explicitly rather than asserting a
field that was never written. See
[scholarship-guide.md](scholarship-guide.md#known-gaps).

## Synthetic identities

Seven keys are generated locally under the `sch-seed-` prefix:

| Label | Role in the scenario |
| --- | --- |
| `sch-seed-admin` | Contract admin, and the `Administrator` role |
| `sch-seed-sponsor` | Program owner, `Sponsor` |
| `sch-seed-applicant` | `Student`, applicant, and payout recipient |
| `sch-seed-reviewer` | `Reviewer`, evidence verifier |
| `sch-seed-finance` | `Finance`, disbursement creator |
| `sch-seed-wallet` | Payout wallet whose key proves control |
| `sch-seed-issuer` | Eligibility attestation issuer |

No real person, wallet, or pre-existing account is involved, and no existing
account is reused for an applicant role. Keys persist between runs, which is
what makes the scenario repeatable; remove them with
`stellar keys rm sch-seed-<label>`.

`sch-seed-wallet` exists as a separate identity from `sch-seed-applicant` on
purpose. Conflating the recipient and the proving wallet is the mistake that
would make a payout flow look verified when it only proves self-control.

## Determinism

Every identifier is derived as `SHA-256(SCHOLARSHIP_SEED:<label>)`, so the same
seed always produces the same `program_id`, form schema hash, consent terms
hash, and application commitment. Two consequences:

- Re-running `apply` targets the same program instead of littering the network
  with near-duplicates.
- Two developers running with the default seed are working on the *same*
  scenario. Set `SCHOLARSHIP_SEED` to something unique for an independent run.

```bash
SCHOLARSHIP_SEED=alice-trial-1 ./scripts/seed-scholarship-staging.sh apply
```

## The signing boundary

Payouts require the recipient to prove control of a payout wallet. That proof
is the only step in this flow that is not a contract call, and it is narrow on
purpose:

1. `open_wallet_challenge` returns a challenge id.
2. `wallet_challenge_payload` is a **public read** that returns the exact 32-byte
   digest the contract will verify.
3. The wallet signs that digest with its Ed25519 key, off-chain.
4. `confirm_wallet_challenge` submits the signature; the contract calls
   `verify_ed25519`.

The signer does not reconstruct the contract's signing domain and does not
encode XDR. It signs bytes the contract published, which is why it is a short
program rather than a client library.

`scholarship_wallet_signer.py` is a pure-Python RFC 8032 implementation because
staging environments cannot be relied on to have an Ed25519-capable OpenSSL
(macOS LibreSSL 3.3 does not) or a Python crypto module. It is constrained to
be safe:

- `--self-test` validates it against the RFC 8032 vectors plus a tamper check,
  and `apply` aborts if that fails.
- 31 tests cover the vectors, wrong messages, tampered bytes, malformed lengths,
  non-canonical scalars, and StrKey checksum rejection.
- A wrong signature cannot move money. The contract rejects a mismatch with
  `InvalidSignature`, so the worst outcome of a signer bug is a failed
  confirmation.
- The signer holds no network policy, because a Stellar secret does not encode
  its network — a testnet secret is indistinguishable from a mainnet one. The
  passphrase check lives in the seed script, which is the layer that knows it.

Run the self-test on its own at any time:

```bash
python3 scripts/scholarship_wallet_signer.py --self-test
```

## Repeatability

`apply` probes state before each mutating step, so a second run is a no-op
rather than an `AlreadyInitialized` failure. Steps that cannot be probed
cleanly are written to be naturally idempotent, and any step that genuinely
cannot be repeated is recorded in the ledger with a note saying so.

## Reversibility

`teardown` replays `deployments/scholarship-staging.json` backwards. Be precise
about what that means: **the chain cannot be rewound.** Reversal drives
contracts back toward their initial state. Every event ever published stays in
the ledger permanently.

`teardown` withdraws what can be withdrawn — cancels an open intent, releases a
reserved award, revokes consent, closes the program — and then states plainly
what it cannot undo:

- `initialize` on all seven contracts: one-shot, no uninitialize
- the attestation, the application, the verified evidence
- a settled disbursement, and the wallet binding
- the award budget, on purpose: see below

### Why teardown leaves the budget alone

`configure_award_budget` has no state guard. Calling it again overwrites the
stored budget and resets `awarded_count` and `committed_amount` to zero, which
would destroy the very history teardown is trying to wind down — and would let
the `total_budget` ceiling be circumvented. So teardown does not touch it.

Treat `configure_award_budget` as privileged. Gate it out of routine operations
tooling; the contract will not do it for you.

### For a genuinely clean slate

```bash
./scripts/reset-testnet.sh
```

That redeploys from scratch. It is heavier than `teardown` and destroys
everything else on the network, including non-scholarship contracts.

## Ownership

| Area | Owner |
| --- | --- |
| The seven contract sources | Platform team |
| `scholarship-api-baseline.json` and the compatibility gate | Platform team; changes need the override process |
| This seed flow and the signer | Platform team |
| Testnet RPC and funded accounts | whoever holds the testnet account funding |
| The synthetic keys | generated locally, nobody's responsibility after `teardown` |
| Production scholarship operations | Not established — see [scholarship-launch-readiness.md](scholarship-launch-readiness.md) |

## Privacy

The scenario contains no personal data of any kind. The `applicant` is a
locally generated key, and the `data_hash` is a SHA-256 of a fixed label, not of
anything resembling a real application.

Two things to keep in mind when reading staging results:

- **Participation is public even though content is not.** `SUBMIT`, `CONSENT`
  and `ATTEST` publish addresses. A staging ledger is a realistic demonstration
  that an application's answers stay private while the fact that a given
  address applied to a given program does not.
- **Staging keys are not private keys worth protecting, but treat them as
  disposable anyway.** They are unencrypted Stellar keys in your local keyring.
  Never reuse a `sch-seed-` key on mainnet, and never fund one with real assets.

## Migration

The scenario itself is versioned by `SCHOLARSHIP_SEED`, default
`scholarship-staging-v1`. Changing it produces a fully independent scenario
alongside the old one rather than mutating it, because contracts have no
"update the seeded record" path for most of what the flow writes.

For contract changes specifically:

- The interface gate runs against
  [`contracts/scholarship-api-baseline.json`](../scholarship-api-baseline.json).
  If a change here starts failing, the flow was written against an interface
  that moved. Fix the script and the guide in the same PR.
- Versioned on-chain data — the form schema, the consent terms, the
  eligibility rule — is versioned by the contracts themselves, and the flow
  always publishes version 1. It does not attempt a v2, so it cannot be used to
  test a re-consent or re-migration path.
- `scholarship-outbox` and `scholarship-migration` (from the integration
  work) are not part of this scenario. Seeding those is separate work.

## Operational impact

- Cost: seven contract deployments plus roughly 40 testnet transactions. Testnet
  XLM is free, but the fee account needs a small balance.
- Time: dominated by transaction finality, so a full `apply` is minutes rather
  than seconds.
- Blast radius: additive and confined to one program, one set of synthetic
  keys, and one budget. Nothing here touches mainnet.
- The ledger file `deployments/scholarship-staging.json` is the audit record of
  what was provisioned. Keep it until you are done, then delete it.
- The seed is a staging tool. It is not a migration tool, not a backup tool,
  and not safe to point at a network holding real funds.

## Known limits

1. **The scholarship contracts are not in `deploy-testnet.sh`.** They must be
   deployed and their IDs supplied separately. The flow reports exactly which
   one is missing.
2. **No application review outcome is seeded**, because no function records
   one. `verify` asserts the assignment instead and names the omission.
3. **Budget reconfiguration is not exercised**, on purpose, for the reason in
   [Reversibility](#why-teardown-leaves-the-budget-alone).
4. **The network leg has not been executed in the environment where this was
   written.** The Stellar CLI, a funded testnet account, and the seven
   deployed contract IDs were not available, so `plan`, the signer's RFC 8032
   self-test, the 59-test Python suite, the shell syntax, and the guards
   (missing CLI, unknown subcommand, mainnet refusal, determinism) are all
   verified, but no transaction has been submitted. Run `apply` then `verify` on
   a machine with the CLI before relying on the flow.
5. **Only one program, one applicant, one milestone.** It is a smoke test of the
   wiring, not a load test.

## Related documents

| Document | Covers |
| --- | --- |
| [scholarship-guide.md](scholarship-guide.md) | What each role can call, and the known gaps |
| [scholarship-launch-readiness.md](scholarship-launch-readiness.md) | Go-live criteria and rollback |
| [testnet-setup.md](../../docs/testnet-setup.md) | Testnet accounts and funding |
| [scholarship-api-baseline.json](../scholarship-api-baseline.json) | The interface this flow was written against |
