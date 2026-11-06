//! Bounded Maven 3.9.11 / Resolver 1.9.24 compatibility engine.
//!
//! Repository I/O is injected; resolution never reads Maven settings or the host cache.
//! See COMPATIBILITY.md before using the output to construct a build classpath.
mod activation;
mod graph;
mod model;
mod repository;
mod types;

pub use graph::{ContextualModels, resolve, resolve_with_contextual_models, resolve_with_models};
pub use model::{EffectiveModel, inspect_model, inspect_model_with_context};
pub use repository::{DirectoryRepository, RepositoryProvider};
pub use types::*;

pub const COMPATIBILITY_TARGET: &str = "maven-3.9.11/resolver-1.9.24";
