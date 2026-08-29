//! Vendor-neutral APIs for Open Knowledge Format bundles.
//!
//! The crate loads Markdown-based bundles, validates their structure, exposes a directed
//! knowledge graph, provides deterministic retrieval, and implements the storage-independent OKF
//! Library runtime plus pluggable provider transports/adapters.

#![forbid(unsafe_code)]

pub mod agent_provider;
pub mod error;
pub mod graph;
#[cfg(feature = "http-provider")]
pub mod http_provider;
pub mod library;
pub mod library_manifest;
pub mod library_providers;
pub mod library_sources;
pub mod model;
pub mod parser;
pub mod process_provider;
pub mod provider_protocol;
pub mod provider_stack;
pub mod providers;
pub mod retrieval;
#[cfg(feature = "s3-provider")]
pub mod s3_provider;
pub mod semantic_provider;
#[cfg(feature = "sqlite-provider")]
pub mod sqlite_provider;
pub mod validator;

pub use agent_provider::{AgentQueryExecutor, AgentQueryProvider};
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
pub use library_providers::{
    CatalogProvider, CompositeLibraryProvider, ContentProvider, QueryProvider, RefreshProvider,
};
pub use library_sources::{LibrarySourceResolver, LibrarySourceResolvers};
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
#[cfg(feature = "s3-provider")]
pub use s3_provider::S3ContentProvider;
pub use semantic_provider::{
    EmbeddingProvider, FnEmbeddingProvider, SemanticEntry, VectorSemanticQueryProvider,
};
#[cfg(feature = "sqlite-provider")]
pub use sqlite_provider::SqliteLibraryProvider;
pub use validator::{Severity, ValidationIssue, ValidationOptions, ValidationReport, Validator};
