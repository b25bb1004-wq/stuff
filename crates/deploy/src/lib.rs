//! Deploy: service binaries, health probe, container images and compose file (T77).

#![deny(unsafe_code)]

pub mod cli;
pub mod probe;
mod services;
pub mod shutdown;
pub mod web;

pub use services::{run_aurum, run_iridium, run_registry, AurumConfig};
pub use web::run_web;
