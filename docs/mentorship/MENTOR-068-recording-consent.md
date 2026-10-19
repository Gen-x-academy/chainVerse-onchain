# MENTOR-068: Add Session Recording Consent Controls

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Add session recording consent controls.

## On-chain

A recording policy per session holding whether recording is prohibited, optional or required, plus a per-participant consent state recorded at a specific revision of that policy.

## Off-chain boundary

The consent prompt, the recording provider integration and the recording playback experience are served by the app.

## Rules

- The recording policy is a closed set: prohibited, optional or required. A session cannot be booked into a policy it violates.
- Explicit consent is collected from every participant. Where recording is required, a participant who does not consent cannot be booked.
- Consent is bound to the policy revision it was given under. Changing the policy invalidates prior consent and requires it again.
- Consent can be withdrawn up to the point recording begins; after that it can only be recorded as withheld for future sessions.
- Only a provider reference is stored. No audio, video, transcript or participant biometric is written on-chain.
- Every participant must consent before recording starts. A partial consent set blocks the session from recording.

## Events and errors

Events `recording_policy_set(session, policy)` and `recording_consent_recorded(session, participant, consented)`. Errors: `NotAuthorized`, `NotBooked`, `PolicyChanged`, `ConsentIncomplete`, `RecordingProhibited`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-068 work lives in the other ChainVerse repositories.