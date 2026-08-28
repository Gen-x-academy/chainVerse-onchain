#![no_std]

pub mod vesting;

pub use shared::error::ContractError;
pub use shared::events::EventEmitter;
pub use shared::storage::{
    get_instance_storage, get_persistent_storage, remove_instance_storage,
    remove_persistent_storage, set_instance_storage, set_persistent_storage,
};
pub use vesting::{
    AcademyVestingContract, ClaimEvent, GrantEvent, RevokeEvent, VestingError, VestingSchedule,
};
