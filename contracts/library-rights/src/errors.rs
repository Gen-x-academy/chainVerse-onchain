use soroban_sdk::contracterror;

/// Typed errors for the library-rights contract.
///
/// Kept local to this crate rather than re-using `shared::ContractError`:
/// every existing workspace contract (`course_registry`, `staking`,
/// `token`, `payout-automation`, `escrow-vault`, ...) defines its own
/// local error enum despite `docs/contracts.md` describing a shared-enum
/// convention, so this follows the convention actually in force across
/// the codebase.
///
/// Discriminants 1..=9 are assigned to the foundation and registry
/// work (#924..#927, and #931/#934 which live in concurrently-open PRs);
/// the registry features in this crate (#928, #929, #932, #933) start at
/// 10 so concurrent additions never silently collide on an ABI value.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// `bootstrap` was called after the contract was already bootstrapped.
    AlreadyInitialized = 1,
    /// A role-gated call was made before `bootstrap` ever succeeded.
    NotInitialized = 2,
    /// The caller does not hold the role required for this call.
    NotAdmin = 3,
    /// Two or more of the four roles were given the same address.
    DuplicateRole = 4,
    /// No work record exists for the given work id.
    WorkNotFound = 5,
    /// A registry identifier (entry id) was malformed (all-zero bytes).
    InvalidIdentifier = 10,
    /// A commitment hash (metadata manifest hash or content digest) was
    /// malformed (all-zero bytes); a commitment must carry a real value.
    InvalidHash = 11,
    /// An entry id is already registered; overwrites are not allowed.
    AlreadyRegistered = 12,
    /// No catalog entry exists for the given id.
    EntryNotFound = 13,
    /// The parent entry does not exist or cannot legally carry this child
    /// kind (editions only under works, renditions only under editions).
    InvalidParent = 14,
    /// The operation is not valid for the entry's kind (e.g. anchoring a
    /// content hash on a work, which has no rendition content).
    InvalidKind = 15,
    /// The metadata URI is empty, exceeds the length bound, or uses a
    /// scheme that is not on the allowlist.
    InvalidMetadataUri = 16,
    /// The requested version snapshot does not exist for this entry.
    VersionNotFound = 17,
    /// A children query used an invalid limit or an out-of-range cursor.
    InvalidLimit = 18,
    /// A monotonic counter overflowed; the operation failed deterministically
    /// instead of silently wrapping.
    Overflow = 19,
}
