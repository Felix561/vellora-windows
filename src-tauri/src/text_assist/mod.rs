//! Text Assist: polish or rewrite the selected text in any Windows editor.
//! Design: docs/TEXT_ASSIST_PLAN.md.

// Wired into the app in M2/M3; until then some items are unused.
#![allow(dead_code)]

#[cfg(test)]
pub mod benchmark;
pub mod commands;
pub mod geometry;
pub mod guard;
pub mod job;
pub mod panel;
pub mod platform;
pub mod prompts;
pub mod runtime;
