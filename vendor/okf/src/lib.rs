//! Vendor-neutral APIs for Open Knowledge Format bundles and Libraries.

#![forbid(unsafe_code)]

pub mod error;
pub mod graph;
pub mod library;
pub mod library_manifest;
pub mod model;
pub mod parser;
pub mod providers;
pub mod retrieval;
pub mod validator;

pub use error::{Error, Result};
pub use graph::KnowledgeGraph;
pub use library::{
    CatalogEntry, KnowledgeNode, KnowledgeNodeKind, KnowledgeUri, LibraryCapability,
    LibraryCatalog, LibraryError, LibraryId, LibraryInstance, LibraryManifest, LibraryProvider,
    LibraryQuery, LibraryQueryHit, LibraryQueryResult, LibraryRegistry, LibraryResult,
    LibrarySource, QueryStrategy,
};
pub use library_manifest::{
    LIBRARY_MANIFEST_FILENAME, LibraryCatalogDeclaration, LibraryManifestError,
    LibraryPackageManifest, LibraryQueryDeclaration,
};
pub use model::{Bundle, Document, DocumentId, InvalidDocumentId, Metadata, Reference};
pub use parser::{BundleParser, ParserOptions, parse_document};
pub use providers::{BundleLibraryProvider, VirtualLibraryProvider};
pub use retrieval::{MatchField, SearchHit, SearchQuery};
pub use validator::{Severity, ValidationIssue, ValidationOptions, ValidationReport, Validator};
