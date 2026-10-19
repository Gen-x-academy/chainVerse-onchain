# MENTOR-061: Create Session Workspace

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Create session workspace.

## On-chain

One workspace per booked session, keyed by session id, holding the workspace version and the ids of the agenda items, goals, resources and follow-up items attached to it.

## Off-chain boundary

The workspace UI, resource uploads and any collaborative editing are served by the app.

## Rules

- A workspace is created exactly once per booked session; a second create for the same session is rejected.
- Only the two booked participants may read the workspace or attach items to it.
- Attachments are referenced by id and their content lives off-chain.
- The workspace id is the join key that every later session record keys against.
- A cancelled session's workspace is retained read-only so settlement can still resolve against it.

## Events and errors

Event `session_workspace_created(session, workspace)`. Errors: `NotAuthorized`, `NotBooked`, `AlreadyExists`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-061 work lives in the other ChainVerse repositories.