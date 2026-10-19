# Mentorship Capability Configuration

Contract: `mentorship-config`  
Version: 1

## Environment isolation and defaults

Each configuration is keyed by `Development`, `Staging`, or `Production`.
An environment with no explicit configuration reads as disabled, has no
supported currencies or session durations, limits each participant to one
active engagement and each engagement to one session, and uses a 24-hour
cancellation window. Default evidence rules require one opaque reference and
tutor attestation. Defaults have version `0` and no updater.

## Configuration fields

| Field | Validation |
|---|---|
| `enabled` | Enabling requires at least one supported currency, one allowed session duration, and tutor attestation |
| `max_active_engagements_per_participant` | 1-100 |
| `max_sessions_per_engagement` | 1-100 |
| `supported_currencies` | Unique Soroban token contract addresses; maximum 8 |
| `session_durations_minutes` | Unique durations from 15 to 240 minutes; maximum 8 |
| `cancellation_window_seconds` | 0-30 days |
| `evidence_rules.minimum_references` | 0-8; at least 1 while enabled |
| `evidence_rules.require_tutor_attestation` | Must be true while enabled |
| `evidence_rules.require_student_confirmation` | Explicitly configured per environment |

## ABI and events

- `initialize(admin)` establishes the sole configuration administrator.
- `set_config(admin, environment, settings)` validates and publishes a new
  environment-specific version.
- `get_config(environment)` returns the active snapshot or safe default;
  `is_enabled(environment)` is a convenience rollout check, not an access
  control decision.
- `CFGSET` emits `(environment, snapshot)` after each successful update.

## Privacy and trust boundary

Only public token contract addresses, numerical policy, version metadata, and
admin addresses are stored. Evidence bodies and personal session content stay
off-chain; workflows may use opaque hashes/references and public attestations.
Integrating contracts must enforce participant authorization independently:
an enabled feature flag does not grant any role or permission. Cancellation
timing and evidence-count enforcement are policy inputs for the booking and
settlement contracts; this registry does not implement those workflows.