//! Explicit dependency locking. Building from a lock never invokes this resolver.
pub mod lock;
pub mod repository;

pub mod artifacts;
pub mod build;

pub mod migration;

pub mod fat;

pub mod idea;

pub mod lock_codec;
pub mod matrix;
