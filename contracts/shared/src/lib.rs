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
