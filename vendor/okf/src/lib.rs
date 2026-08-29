//! Vendored build snapshot of the vendor-neutral OKF SDK.

#![forbid(unsafe_code)]

pub mod error;
pub mod graph;
#[cfg(feature = "http-provider")]
pub mod http_provider;
pub mod library;
pub mod library_manifest;
pub mod model;
pub mod parser;
pub mod process_provider;
pub mod provider_protocol;
pub mod provider_stack;
pub mod providers;
pub mod retrieval;
pub mod validator;

pub use error::{Error, Result};
pub use graph::KnowledgeGraph;
#[cfg(feature = "http-provider")]
pub use http_provider::HttpLibraryProvider;
pub use library::{
    CatalogEntry, KnowledgeNode, KnowledgeNodeKind, KnowledgeUri, LibraryCapability,
    LibraryCatalog, LibraryError, LibraryId, LibraryInstance, LibraryManifest, LibraryProvider,
    LibraryQuery, LibraryQueryHit, LibraryQueryResult, LibraryRegistry, LibraryResult,
    LibrarySource, QueryStrategy,
};
pub use library_manifest::{
    LIBRARY_MANIFEST_FILENAME, LibraryCatalogDeclaration, LibraryManifestError,
    LibraryPackageManifest, LibraryProviderDeclaration, LibraryQueryDeclaration,
};
pub use model::{Bundle, Document, DocumentId, InvalidDocumentId, Metadata, Reference};
pub use parser::{BundleParser, ParserOptions, parse_document};
pub use process_provider::ProcessLibraryProvider;
pub use provider_protocol::{
    PROVIDER_PROTOCOL_V1, ProviderOperation, ProviderProtocolError, ProviderRequest,
    ProviderResponse, decode_provider_request, decode_provider_response,
};
pub use provider_stack::ProviderStack;
pub use providers::{BundleLibraryProvider, VirtualLibraryProvider};
pub use retrieval::{MatchField, SearchHit, SearchQuery};
pub use validator::{Severity, ValidationIssue, ValidationOptions, ValidationReport, Validator};
