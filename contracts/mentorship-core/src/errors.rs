use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// A mentorship with this ID already exists.
    MentorshipAlreadyExists = 4,
    MentorshipNotFound = 5,
    /// The proposed mentor-mentee pair already has an active or pending mentorship.
    ConflictingMentorship = 6,
    /// Caller is not the mentor of this match.
    NotMentor = 7,
    /// Caller is not the mentee of this match.
    NotMentee = 8,
    /// The mentorship amount must be strictly positive.
    InvalidAmount = 9,
    /// Start and end dates are invalid (start must be in future, end after start).
    InvalidDates = 10,
    /// Title is empty or exceeds MAX_TITLE_LEN.
    InvalidTitle = 11,
    /// The requested state change is not a legal transition.
    InvalidStatus = 12,
    /// The caller is not authorized for this operation.
    Unauthorized = 13,
    /// A mentorship match not found when loaded.
    NotFound = 14,
}
