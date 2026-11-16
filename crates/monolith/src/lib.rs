//! Monolith: a harness that gives a local AI model its own computer made of the Monolith tools.
//!
//! One AI computer is a folder (`$MONOLITH_HOME/computers/NAME/`): `root/` holds its files,
//! `journal.jsonl` records every step, `memory/` holds what it learned. The model drives it one
//! [`types::Action`] at a time. Design: `docs/ai-computer/plans/2026-11-16-harness-and-teaching.md`.

pub mod actions;
pub mod agent;
pub mod computer;
pub mod eval;
pub mod journal;
pub mod llm;
pub mod memory;
pub mod suite;
pub mod types;
