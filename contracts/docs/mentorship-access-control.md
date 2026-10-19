# Mentorship Access Control

Contract: `mentorship-access-control`  
Version: 1

## Purpose

Provides MENTOR-003's scoped role registry and least-privilege permission
matrix for mentorship contracts. This contract authorizes actions; it does not
store or serve private session content.

## Role matrix

| Role | Default permissions |
|---|---|
| Student | View engagement, create/update engagement, submit session evidence, raise safety concern |
| Tutor | View/update engagement, manage availability, submit session evidence, view payment status, raise safety concern |
| Mentor | View/update engagement, manage availability, submit session evidence, view payment status, raise safety concern |
| Guardian | View engagement, manage guardian consent, view payment status, raise safety concern |
| OrganizationStaff | View engagement, manage organization roster |
| Moderator | Review safety case, moderate participant |
| FinanceStaff | View payment status, initiate settlement |
| Administrator | Approve settlement, manage roles, audit access |

Unlisted role-permission pairs are denied. In particular, participants and
organization staff cannot review safety cases or approve settlements. Finance
staff may initiate but not approve settlement, and moderators cannot view
payment status. Administrators do not receive default access to engagement
content or safety cases.

## Permission identifiers

`ViewEngagement`, `CreateEngagement`, `UpdateEngagement`, `ManageAvailability`,
`SubmitSessionEvidence`, `ManageGuardianConsent`, `ViewPaymentStatus`,
`InitiateSettlement`, `ApproveSettlement`, `RaiseSafetyConcern`,
`ReviewSafetyCase`, `ModerateParticipant`, `ManageOrganizationRoster`,
`ManageRoles`, and `AuditAccess`.

## Scope and authorization

Role grants are keyed by `(account, scope, role)`, where `scope` is an opaque
32-byte mentorship or organization identifier. The zero value is reserved for
the global Administrator role; all other roles are scoped. The contract
administrator alone may grant/revoke roles and update matrix entries.
`authorize` requires the account's Soroban authorization, a matching scoped
role, and an allowed matrix entry. Revocation takes effect immediately.

## ABI and events

- `initialize(admin)`, `grant_role(admin, account, scope, role)`, and
  `revoke_role(admin, account, scope, role)` manage grants.
- `has_role(account, scope, role)`, `can(role, permission)`, and
  `authorize(account, scope, role, permission)` query or enforce access.
- `set_permission(admin, role, permission, allowed)` updates one matrix cell.
- `MROLE_GRT`, `MROLE_RVK`, and `MPERM_SET` events record policy changes.

## Privacy

On-chain state contains only public addresses, opaque 32-byte scope IDs, role
bindings, and permission bits. Keep session text, identity data, guardian
details, and safety-case content off-chain; use opaque references or hashes in
the calling workflow where commitments are required.