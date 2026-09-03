//! Core logic for ConstructCLI.
//!
//! This crate knows nothing about command-line arguments, stdout, or exit codes.
//! It returns typed values and typed errors, and it does not panic.

pub mod catalog;
pub mod config;
pub mod discovery;
pub mod error;
pub mod leveldat;
pub mod pack;
pub mod store;

pub use error::{CoreError, Result};
