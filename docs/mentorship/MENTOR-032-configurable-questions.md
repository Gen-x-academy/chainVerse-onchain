# MENTOR-032: Configurable Application Questions

**Track:** Applications | **Component:** on-chain | **Status:** implemented

Goal: Support configurable application questions.

## On-chain

`publish_form(admin, program_id, schema_hash, questions)` publishes an
immutable, versioned mentorship application form. Publishing again always
creates version N+1; an earlier version is never overwritten, so an application
already accepted against version N can always be re-rendered against its exact
question set.

Each `Question` carries:

- `id` (stable, non-zero) and `order` (strictly increasing within a version);
- `kind` — `ShortText`, `LongText`, `Email`, `Number`, `Choice`,
  `MultiChoice`, `Date`;
- `required`;
- `label_hash`, `help_hash` and, for choice types, `options_hash` — integrity
  commitments over off-chain text/options;
- `visible_if: VisibilityCondition { depends_on, required_answer_hash }`, where
  `depends_on = 0` means always visible.

`validate_answers` exposes the same validation used by submission as a
read-only view, so clients can preview errors without submitting.

### Validation and determinism

`publish_form` rejects empty forms, more than 32 questions, id 0, duplicate ids,
non-increasing `order`, zero commitments, an options commitment on a non-choice
type, a choice type without options, an expected answer without a dependency,
and any dependency that is not an earlier question. The "earlier question" rule
makes cycles and forward references unrepresentable, so visibility is evaluated
in a single deterministic forward pass.

A question is visible iff its dependency is answered and, when
`required_answer_hash` is set, the dependency's answer commitment equals it.
Because only commitments are compared, value-dependent branching is possible
without ever putting an answer value on chain. Required questions are required
only while visible; answers to hidden questions are rejected
(`AnswerNotVisible`), so the answer set uniquely determines the rendered form.

### Unsafe markup

The contract accepts no strings at all. Labels, help text and options are
`BytesN<32>` commitments over content sanitised off-chain. There is therefore no
storage boundary through which markup — unsafe or otherwise — can enter on-chain
state.

## Off-chain boundary

Form structure, labels, help text, option lists and conditional copy are owned
by the application service and rendered by clients. The contract stores only
version, ordering, types, required flags, visibility rules and commitments.
Clients must canonicalise the off-chain form document and hash it into
`schema_hash` so the published commitment is verifiable. Rendering historical
responses uses `get_form(program_id, version)` plus the application's stored
`form_version`.

## Rules

- Published versions are immutable; version numbers never decrease or repeat.
- Unsafe markup is rejected by construction (commitments only) and zero
  commitments are rejected.
- Conditions are deterministic: earlier-question-only dependencies, one forward
  pass, commitment comparison only.
- Historical responses remain renderable: every application records its
  accepted `form_version`, and older form versions remain readable.
- Answers referencing unknown questions, duplicate answers and more than 32
  answers fail with `AnswerNotInForm`, `DuplicateAnswer` and `TooManyAnswers`.

## Events and errors

Event `FORMPUB(program_id, version)` per published version. Errors:
`NoFormPublished`, `FormVersionNotFound`, `InvalidForm`, `NotAdmin`,
`ProgramNotFound`, `VersionOverflow`, plus submission-side
`MissingRequiredAnswer`, `AnswerNotVisible`, `AnswerNotInForm`,
`DuplicateAnswer`, `InvalidAnswerHash`.

## Dependencies and rollout

Depends on `register_program` and on the consent-terms primitive in the same
contract. The matching MENTOR-032 work in the other repositories owns the form
builder, markup sanitisation, canonical hashing, renderer for historical
versions and conditional-visibility UI. Ship behind the mentorship feature flag;
forms are published by program owners after `register_program` and before
opening intake. Full contract notes:
`contracts/docs/mentorship-applications.md`.
