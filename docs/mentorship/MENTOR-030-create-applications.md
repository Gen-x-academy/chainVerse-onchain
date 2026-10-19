# MENTOR-030: Create Mentorship Applications

**Track:** Applications | **Component:** on-chain | **Status:** implemented

Goal: Create mentorship applications.

## On-chain

`contracts/mentorship-applications` exposes `submit_application`, distinct from
the draft entry points and atomic in one invocation. It records an immutable
`Application` containing the applicant, program, accepted `form_version`,
accepted `consent_version`, answer count, submission timestamp and a single
`data_hash` commitment over the off-chain application content (goals,
expectations, schedule, preferred mode, answers and attachments).

Submission validates, before any write:

- the program exists, is active, and the intake deadline has not passed;
- the accepted form version exists and every visible required question has an
  answer, with no answers to hidden questions;
- the applicant passed `consent = true` and holds a non-revoked consent record
  at the program's current terms version;
- no application already exists for `(applicant, program)`.

A failed check therefore reverts with no partial record, no consumed
application id and no queue row, so retries are safe. A successful submission
removes the applicant's draft for the same program, so an application can never
be mutated as a draft afterwards. `has_applied` and `get_application_id` let
clients check uniqueness cheaply.

## Off-chain boundary

The actual goals, expectations, schedule, preferred mode, free-text answers and
attachments never reach the chain. Clients canonicalize them, store them in the
application service, and pass the SHA-256 commitment as `data_hash`. Consent
terms text, form labels/help text and options are likewise commitments only
(`terms_hash`, `label_hash`, `help_hash`, `options_hash`). Rich content
validation (word limits, address/email format, option membership) and document
storage are application-service responsibilities; the contract enforces
presence of required answers and visibility conditions.

## Rules

- Drafts and submission are distinct entry points; submission never reads or
  flushes draft state implicitly.
- One active application per `(applicant, program)`; a replay or a second
  attempt fails with `DuplicateApplication` and leaves the first untouched.
- `autosave_draft` after a successful submission fails with
  `DuplicateApplication`, and `get_draft` returns `DraftNotFound`.
- Submission at the exact deadline timestamp is allowed; one second later fails
  with `DeadlinePassed`.
- Required answers are evaluated against the form version the applicant
  accepted, which is stored on the application for later rendering.
- Errors are stable codes, not panics: `ProgramNotFound`, `ProgramInactive`,
  `DeadlinePassed`, `FormVersionNotFound`, `MissingRequiredAnswer`,
  `AnswerNotVisible`, `AnswerNotInForm`, `DuplicateAnswer`,
  `NoConsentTermsPublished`, `ConsentNotFound`, `ConsentRevoked`,
  `ConsentOutOfDate`, `ConsentRequired`, `DuplicateApplication`,
  `InvalidDataHash`.

## Events and errors

Event `SUBMIT(application_id, applicant, program_id)`, emitted only after all
writes succeed. Errors are the `ContractError` variants listed above (codes
5–27 and 40).

## Dependencies and rollout

Depends on `publish_form` (MENTOR-032) and `publish_consent_terms`/
`record_consent` (shared consent primitive in this contract). Clients in the
other ChainVerse repositories own the application wizard, consent screens,
attachment handling and submission receipt UI. Ships behind the mentorship
feature flag as part of the MENTOR-120 pilot rollout. Full contract notes:
`contracts/docs/mentorship-applications.md`.
