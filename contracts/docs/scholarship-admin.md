# Scholarship rollout flags and administration configuration

Implements issues [#1144](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1144) and
[#1145](https://github.com/Gen-x-academy/chainVerse-onchain/issues/1145).

One contract, `scholarship-admin`, because flags and configuration share an
authority, an audit trail, and a version history, and splitting them would mean
two contracts that have to agree about who may change what.

## What this is for

Two decisions an operator has to make:

- **Which surfaces are exposed** (#1144) — discovery, applications, reviews,
  awards, and payouts, independently, per environment and per cohort.
- **The parameters those surfaces run under** (#1145) — limits, deadlines,
  providers, assets, templates, and risk thresholds.

## Flags are not permissions

This is the most important sentence in this document.

A flag is **not** an authorization decision. `Flag::Awards = On` means "the
awards surface is exposed"; it does not mean any particular address may create
an award. The authoritative check stays server-side, in the backend that
already authorizes against the scholarship contracts.

The contract is built so that it *cannot* be mistaken for one:

- No function takes a flag and returns a capability.
- No function here moves value or is called by a value-moving path.
- `is_enabled` is documented as a read-only hint at every call site.

If a future change made a flag gate a payout, this contract would have become
an authorization bug with a rollout UI attached. That is the failure mode to
watch for in review.

## Flags fail closed

- An unconfigured flag reads as `Off`, never `On`.
- `is_enabled` answers `false` for an unconfigured flag or environment rather
  than erroring, because the caller needs a boolean to gate a menu with, and
  the safe answer to "is this configured?" is no.
- A missing configuration can never be the reason a feature becomes available.

Flags are three-state rather than boolean, because "on for two cohorts" is a
real rollout shape:

| State | Meaning |
| --- | --- |
| `Off` | nothing is exposed |
| `On` | everything is exposed |
| `CohortsOnly([...])` | only the named cohorts are exposed |

`CohortsOnly` with an empty list is rejected, because it would be `Off` under
another name while an operator reading the state was told an allowlist exists.

Environments (`Testnet`, `Staging`, `Production`) are separate keys, so a
testnet allowlist cannot leak into production.

## Rollback does not corrupt in-flight work

A rollback is *publishing a previous version*, never editing the present one:

- `rollback_config` appends a new version rather than rewinding the counter, so
  the rollback is itself auditable and history stays append-only.
  (Deriving the new version from the restored one would overwrite a version
  that already exists — the test `test_rollback_does_not_overwrite_an_existing_version`
  pins this.)
- `flag_version` / `config_version` expose retained history, so "what was
  exposed when" is answerable without replaying a deploy log.
- A workflow that read a flag at its start **keeps that decision**.
  `pin` records it, and `pinned_state` returns it afterwards. A rollback
  changes what happens next, never what already happened.

Without pins, rolling a flag back would leave in-flight work referring to a
configuration that no longer exists anywhere. `pinned_state` is the answer to
"why did this award go through when the flag says payouts are off?" — it
started when the flag said otherwise.

## Ownership

- One administrator, set once at `initialize`. `require_auth` plus an exact
  address comparison; there is no role hierarchy and no second signer.
- Every write emits an event carrying the version it replaced, so the exposure
  and configuration history is reconstructable from the event stream alone.
- This contract holds no funds and moves no tokens.

## Privacy

- **No secret is ever stored, accepted, or returned.** A provider's API
  credential lives in an off-chain secret manager; `ref_hash` holds a
  *reference or commitment* to it. A view that returned a credential would put
  it into every `simulate` and indexer.
- Cohort membership is a `Symbol` chosen by the operator, not a student
  identifier. Flags are read by cohort, so this contract does not need to know
  who is in a cohort.
- `is_enabled` takes a cohort symbol and returns a boolean. It stores nothing
  about the caller and creates no per-user record, so enabling a surface does
  not produce a trace of who looked.
- An unconfigured flag's `updated_by` reports the contract admin rather than a
  fabricated address: attributing a read to an address nobody chose would be a
  worse lie than attributing it to the authority that could have.

## Migration

No migration is required. `scholarship-admin` is new and self-contained; it
calls no other contract, per ADR 0002, and no existing contract reads it.

Adopting it is additive:

1. Deploy and `initialize` with the administrator.
2. Configure flags — they default to `Off`, so nothing is exposed by deploying.
3. Move the exposure decision into the backend's rollout check, keeping the
   server-side authorization check as the authority.
4. Configure parameters, using `preview_config` to validate before writing.

Existing workflows are unaffected at every step, because the default is
`Off` and no in-flight workflow has a pin until one is written.

## Operational impact

- **Bounded storage.** At most 32 retained versions per flag and per
  configuration key, at most 32 cohorts per flag, at most 2,000 configuration
  keys, at most 5,000 pins. Counts are held in *instance* storage so they
  cannot expire and silently reset a bound to zero.
- **TTL.** Records carry a ~6–14 month minimum-threshold extension, because
  version history *is* the audit trail and an audit log that expires is not
  one.
- **Version floors are reported.** `flag_version_floor` and
  `config_version_floor` tell an auditor where the retained history begins
  instead of leaving them to assume version 1 is still there. Evicted versions
  return `VersionNotFound`.
- **Rejection is loud.** Exceeding a bound is a typed error
  (`TooManyCohorts`, `TooManyConfigKeys`, `TooManyPins`), never a silent
  truncation — a rollout that quietly dropped half its cohort list would be
  worse than one that refused.
- **Preview before write.** `preview_config` runs the same validation as
  `set_config` and touches no state, so what an operator is shown cannot
  disagree with what the transaction will do. A test drives both paths from
  one table of invalid inputs to keep them honest.

## Tests

49 tests. Beyond the per-criterion cases, the ones worth knowing about:

| Test | What it protects |
| --- | --- |
| `test_every_surface_fails_closed_before_configuration` | all 5 flags × 3 environments start disabled |
| `test_preview_agrees_with_the_write_path_exactly` | preview and write share one validation table |
| `test_rollback_does_not_overwrite_an_existing_version` | a rollback cannot destroy history |
| `test_rollback_leaves_an_in_flight_workflow_intact` | pins survive a flag change and a rollback |
| `test_flag_discriminants_are_fixed` | reordering the enum cannot reinterpret stored flags |
| `test_config_key_table_is_bounded` / `test_pin_table_is_bounded` | the storage bounds actually hold |

## Known limitations

- One administrator, with no multisig or timelock. A compromised admin key can
  turn any surface on immediately. Mitigations live off-chain (deploy under a
  multisig, gate the backend) and a timelock is the natural next increment.
- Pins are never pruned. They are bounded by count, and an operator pruning
  them is destroying the evidence that makes a rollback explainable.
- `is_enabled` is a read-only helper. If the backend ever treats it as
  authoritative, the separation described above is gone; that is the property
  to re-check on every change to this contract.
