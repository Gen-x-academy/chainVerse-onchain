use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MentorshipError {
    // Authorization errors (100-199)
    Unauthorized = 100,
    NotAdmin = 101,
    NotParticipant = 102,
    NotGuardian = 103,

    // State errors (200-299)
    AlreadyInitialized = 200,
    NotInitialized = 201,
    EngagementNotFound = 202,
    InvalidStatus = 203,
    AlreadyPaused = 204,
    NotPaused = 205,
    AlreadyTerminated = 206,
    ConsentNotFound = 207,

    // Validation errors (300-399)
    InvalidAddress = 300,
    InvalidAmount = 301,
    InvalidTimestamp = 302,
    InvalidSessionCount = 303,
    ExpiredConsent = 304,
    ConsentRequired = 305,
    ConsentNotActive = 306,
    MaterialChangeRequiresReconsent = 307,

    // Pause/Resume errors (400-499)
    PauseRequestNotFound = 400,
    PauseNotApproved = 401,
    PauseAlreadyApproved = 402,
    PauseExpired = 403,
    NoActivePause = 404,

    // Feedback errors (500-599)
    FeedbackNotFound = 500,
    InvalidFeedbackType = 501,

    // General errors (900-999)
    InternalError = 900,
    StorageError = 901,
}
