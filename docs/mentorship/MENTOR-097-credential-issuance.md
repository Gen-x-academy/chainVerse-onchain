# MENTOR-097: Mentorship Completion Credentials

**Track:** Reputation | **Component:** on-chain | **Status:** design

Goal: Issue mentorship completion credentials.

## On-chain

A credential record: issuer, recipient reference hash, program, outcome code, issue ledger and revocation status.

## Off-chain boundary

The readable credential document and recipient details stay off-chain.

## Rules

- A credential can be issued only after the agreement is marked complete.
- Only an authorized issuer can issue one, and only once per agreement.
- Every credential can be revoked later.

## Events and errors

Event `credential_issued(credential, program)`. Errors: `NotComplete`, `AlreadyProcessed`, `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-097 work lives in the other ChainVerse repositories.
