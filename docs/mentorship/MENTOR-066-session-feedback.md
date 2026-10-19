# MENTOR-066: Capture Session Feedback

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Capture session feedback.

## On-chain

A feedback record per session and reviewer holding the role the reviewer held, the structured ratings, the reveal time and the hash of any protected comment.

## Off-chain boundary

The feedback form, the rating widgets and the free-text comment box are served by the app.

## Rules

- Feedback is role-appropriate: a mentor and a mentee are scored against different criteria.
- Only a booked participant may submit feedback for a session they attended.
- Each participant may submit once. A repeat submission updates the existing record rather than adding a second.
- Protected comments are committed as hashes and stay hidden until the configured reveal time.
- Feedback is locked once the dispute window opens, so it cannot be changed after settlement has begun.
- Neither side sees the other's feedback before the reveal time, and neither may read it early.

## Events and errors

Events `session_feedback_submitted(session, reviewer, role)` and `session_feedback_revealed(session)`. Errors: `NotAuthorized`, `NotBooked`, `FeedbackLocked`, `AlreadySubmitted`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-066 work lives in the other ChainVerse repositories.