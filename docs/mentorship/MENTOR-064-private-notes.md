# MENTOR-064: Add Private Participant Notes

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Add private participant notes.

## On-chain

A note reference per session and owner holding the ciphertext hash, the creation ledger time and the visibility scope. The note text is never stored on-chain.

## Off-chain boundary

Note composition, client-side encryption and decryption, and the key exchange between the two participants are served by the app.

## Rules

- A note is readable only by its owner. The counterpart, any reviewer and the public profile never see it, even by default.
- Note text is encrypted client-side; the ledger holds only the ciphertext hash, so the contract cannot decrypt or disclose it.
- Only the owner may create, amend or delete their own note.
- A note is never deleted implicitly. Deletion removes the reference and the ledger retains that it existed.
- Where a session is subject to a legal hold, a note reference is marked held and cannot be deleted, though it stays private.

## Events and errors

Event `private_note_set(session, owner, note_hash)`. Errors: `NotAuthorized`, `NotBooked`, `NoteHeld`, `NotFound`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-064 work lives in the other ChainVerse repositories.