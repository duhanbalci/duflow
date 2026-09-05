//! duflow-core: akış grafı modeli, KDL parse/write-back, lint, sorgular, diff, UI export.

pub mod diff;
pub mod edit;
pub mod export;
pub mod expr;
pub mod graph;
pub mod lint;
pub mod model;
pub mod parse;
pub mod query;

pub use graph::Graph;
