# MENTOR-063: Record Privacy-aware Attendance

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Record privacy-aware attendance.

## On-chain

An attendance record per session and participant holding join and leave ledger timestamps, accumulated duration, the verification method used and the state of any correction request.

## Off-chain boundary

The check-in experience, duration display and the review queue for corrections are served by the app.

## Rules

- Only the booked participant may record their own attendance.
- Join, leave and check-in are recorded by the participant who performed them; a coordinator may only submit a correction.
- Duration is accumulated from ledger time, never from a client-supplied number.
- Verification method is stored as an enum value. No biometric, device or location data is written.
- A correction request opens a window; the other party may respond before the correction is applied.
- A correction that is refused leaves the original record unchanged and is itself recorded.

## Events and errors

Events `attendance_recorded(session, participant, verified_by)` and `attendance_correction_requested(session, participant, reason)`. Errors: `NotAuthorized`, `NotBooked`, `SessionNotStarted`, `CorrectionWindowClosed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-063 work lives in the other ChainVerse repositories.