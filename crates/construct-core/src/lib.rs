//! Core logic for ConstructCLI.
//!
//! This crate knows nothing about command-line arguments, stdout, or exit codes.
//! It returns typed values and typed errors, and it does not panic.

pub mod error;

pub use error::{CoreError, Result};
