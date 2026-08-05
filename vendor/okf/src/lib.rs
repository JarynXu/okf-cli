//! Vendor-neutral APIs for Open Knowledge Format bundles.
//!
//! The crate loads Markdown-based bundles, validates their structure, exposes a directed
//! knowledge graph, and provides deterministic lexical retrieval.

#![forbid(unsafe_code)]

pub mod error;
pub mod graph;
pub mod model;
pub mod parser;
pub mod retrieval;
pub mod validator;

pub use error::{Error, Result};
pub use graph::KnowledgeGraph;
pub use model::{Bundle, Document, DocumentId, InvalidDocumentId, Metadata, Reference};
pub use parser::{BundleParser, ParserOptions, parse_document};
pub use retrieval::{MatchField, SearchHit, SearchQuery};
pub use validator::{Severity, ValidationIssue, ValidationOptions, ValidationReport, Validator};
