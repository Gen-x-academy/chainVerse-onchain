/// Shared workspace-level error variants (#739).
///
/// Common error codes shared across multiple ChainVerse contracts.
/// Individual contracts may define additional domain-specific variants
/// that extend beyond this set, but should re-use these codes where
/// the semantics match to ensure consistent on-chain error values that
/// frontends and integrators can handle programmatically.
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    // -- Access control --
    Unauthorized             = 1,

    // -- Lifecycle --
    AlreadyInitialized       = 2,
    NotInitialized           = 3,

    // -- Operational state --
    ContractPaused           = 4,

    // -- Value validation --
    InvalidAmount            = 5,
    InvalidPayment           = 6,

    // -- Asset / token --
    InsufficientBalance      = 7,
    InsufficientAllowance    = 8,

    // -- Certificate / NFT --
    CertificateExists        = 9,
    CertificateNotFound      = 10,
    SoulboundTransferNotAllowed = 11,

    // -- Reward --
    AlreadyRewarded          = 12,
    AlreadyPurchased         = 13,

    // -- Escrow --
    EscrowNotFound           = 14,
    InvalidEscrowState       = 15,
}
