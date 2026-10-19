# MENTOR-062: Build Collaborative Session Agendas

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Build collaborative session agendas.

## On-chain

An agenda per session holding the ordered list of item ids, the revision number, and per item the proposer, assignee, resolution state and the ledger time of the last change.

## Off-chain boundary

The agenda editor, drag-to-reorder interaction and item text are served by the app.

## Rules

- Either booked participant may propose an item, and only the proposer or a coordinator may withdraw one.
- Every accepted change bumps the revision, so the history is reconstructable without storing prior revisions in full.
- Ordering is explicit; a reorder is a change like any other and cannot be applied silently.
- Items are frozen at the pre-session cutoff. After the cutoff only the resolution state may change.
- A withdrawn item stays in the revision history rather than being deleted.

## Events and errors

Events `agenda_item_proposed(session, item, proposer)` and `agenda_revision(session, revision, actor)`. Errors: `NotAuthorized`, `ItemNotFound`, `AgendaFrozen`, `RevisionConflict`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-062 work lives in the other ChainVerse repositories.