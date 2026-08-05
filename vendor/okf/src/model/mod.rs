//! Core data model for OKF bundles.

mod bundle;
mod document;
mod metadata;

pub use bundle::Bundle;
pub use document::{Document, DocumentId, InvalidDocumentId};
pub use metadata::{Metadata, Reference};
