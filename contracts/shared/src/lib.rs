#![no_std]

pub mod error;
pub mod events;
pub mod storage;

pub use error::ContractError;
pub use events::EventEmitter;
pub use storage::{
    get_instance_storage, get_persistent_storage, remove_instance_storage,
    remove_persistent_storage, set_instance_storage, set_persistent_storage,
};

/// Minimum TTL (in ledgers) for persistent storage entries (~1 day at 5s/ledger).
pub const MIN_TTL: u32 = 17_280;
/// Maximum TTL (in ledgers) for persistent storage entries (~30 days at 5s/ledger).
pub const MAX_TTL: u32 = 518_400;
