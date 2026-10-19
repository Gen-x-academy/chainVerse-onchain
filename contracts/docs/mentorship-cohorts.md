# Mentorship Cohorts

The `mentorship-cohorts` Soroban contract maintains cohort scheduling,
capacity, coordinator authorization, and privacy-minimized enrollment state.

## ABI

- `initialize(admin)`: one-time administrator setup.
- `create_cohort(...)`: admin creates a cohort with its program/cohort IDs,
  enrollment window, cohort dates, capacity, coordinator addresses, and
  calendar/settings commitments.
- `update_cohort(...)`: admin updates a cohort's dates, capacity, coordinators,
  and reference commitments. Active enrollments are preserved, and capacity
  cannot be reduced below the active count. Each update increments a version.
- `enroll(coordinator, cohort_id, participant_ref_hash)`: listed coordinator
  enrolls a pseudonymous reference while the enrollment window is open and
  capacity remains.
- `withdraw(coordinator, cohort_id, participant_ref_hash)`: listed
  coordinator withdraws an enrollment; its history is retained and active
  capacity is released. A withdrawn reference cannot be enrolled again in the
  same cohort.
- `get_cohort(cohort_id)` and `get_enrollment(cohort_id, participant_ref_hash)`:
  retrieve cohort configuration or a participant's enrollment metadata.

Enrollment opens/closes and cohort start/end values are UTC ledger timestamps.
Enrollment is accepted inclusively from `enrollment_opens_at` through
`enrollment_closes_at`; the close must be no later than cohort start.

## Privacy and authorization

Participant identity is represented only by a high-entropy 32-byte
`participant_ref_hash`; callers must not provide a raw database key or a plain
hash of low-entropy personal data. Profiles, messages, calendar details,
meeting links, and setting contents remain off-chain. `calendar_ref_hash` and
`settings_hash` are opaque nonzero commitments, and events never contain
participant references.

Only the admin creates or changes cohort configuration. Only addresses in a
cohort's configured coordinator list may enroll or withdraw. Cohort and
enrollment data are public Soroban state, so reference commitments should be
randomized and must not enable guessing or personal-data recovery.

## Events

- `COHNEW(program_id, cohort_id, version)` and
  `COHUPD(program_id, cohort_id, version)` publish configuration changes.
- `COHENR(cohort_id, active_count, ledger_time)` and
  `COHWDR(cohort_id, active_count, ledger_time)` publish aggregate enrollment
  changes without participant references.

Persistent data uses bounded Soroban TTL extension; production services must
periodically touch active cohort and enrollment records and retain their
off-chain reference mappings.
