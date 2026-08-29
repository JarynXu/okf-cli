//! Pluggable OKF Library domain model and runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type for Library operations.
pub type LibraryResult<T> = std::result::Result<T, LibraryError>;

/// Stable Library runtime errors.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum LibraryError {
    #[error("invalid library id: {0}")]
    InvalidLibraryId(String),
    #[error("invalid knowledge URI: {0}")]
    InvalidUri(String),
    #[error("unknown library: {0}")]
    UnknownLibrary(String),
    #[error("library is not mounted: {0}")]
    NotMounted(String),
    #[error("unsupported capability: {0:?}")]
    UnsupportedCapability(LibraryCapability),
    #[error("knowledge node not found: {0}")]
    NodeNotFound(String),
    #[error("library already registered: {0}")]
    Conflict(String),
    #[error("provider failed: {0}")]
    Provider(String),
}

/// Stable Library identifier.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LibraryId(String);

impl LibraryId {
    /// Parses a portable Library identifier.
    pub fn parse(value: impl Into<String>) -> LibraryResult<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'));
        if !valid {
            return Err(LibraryError::InvalidLibraryId(value));
        }
        Ok(Self(value))
    }

    /// Returns the identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LibraryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Canonical logical knowledge URI.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KnowledgeUri {
    library: LibraryId,
    path: String,
}

impl KnowledgeUri {
    /// Creates a URI.
    pub fn new(library: LibraryId, path: impl Into<String>) -> LibraryResult<Self> {
        let path = normalize_path(&path.into())?;
        Ok(Self { library, path })
    }

    /// Parses `okf://<library>/<path>`.
    pub fn parse(value: &str) -> LibraryResult<Self> {
        let remainder = value
            .strip_prefix("okf://")
            .ok_or_else(|| LibraryError::InvalidUri(value.to_owned()))?;
        let (library, path) = remainder.split_once('/').unwrap_or((remainder, ""));
        Self::new(LibraryId::parse(library)?, path)
    }

    /// Owning Library.
    pub fn library(&self) -> &LibraryId {
        &self.library
    }

    /// Logical path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for KnowledgeUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(formatter, "okf://{}/", self.library)
        } else {
            write!(formatter, "okf://{}/{}", self.library, self.path)
        }
    }
}

fn normalize_path(value: &str) -> LibraryResult<String> {
    let value = value.trim().trim_matches('/');
    if value.split('/').any(|segment| segment == "..") || value.contains('\\') {
        return Err(LibraryError::InvalidUri(value.to_owned()));
    }
    Ok(value.to_owned())
}

/// Runtime capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LibraryCapability {
    List,
    Read,
    Catalog,
    Query,
    Refresh,
    Maintain,
}

/// Library source descriptor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LibrarySource {
    Local { path: PathBuf },
    Git {
        repository: String,
        reference: Option<String>,
    },
    Custom { kind: String, location: String },
}

/// Portable Library manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryManifest {
    pub id: LibraryId,
    pub name: String,
    pub version: Option<String>,
    pub source: Option<LibrarySource>,
    pub revision: Option<String>,
}

impl LibraryManifest {
    /// Creates a minimal manifest.
    pub fn new(id: LibraryId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            version: None,
            source: None,
            revision: None,
        }
    }
}

/// Logical node type.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeNodeKind {
    Directory,
    Content,
}

/// Logical knowledge node metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KnowledgeNode {
    pub uri: KnowledgeUri,
    pub kind: KnowledgeNodeKind,
    pub title: Option<String>,
    pub virtual_node: bool,
}

/// Semantic catalog entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogEntry {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub uri: KnowledgeUri,
    pub terms: BTreeSet<String>,
}

/// Semantic Library catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryCatalog {
    pub library: LibraryId,
    pub entries: Vec<CatalogEntry>,
}

impl LibraryCatalog {
    /// Creates an empty catalog.
    pub fn empty(library: LibraryId) -> Self {
        Self {
            library,
            entries: Vec::new(),
        }
    }
}

/// Query strategy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueryStrategy {
    Exact,
    Lexical,
    Semantic,
    Graph,
    Agentic,
    Custom(String),
}

/// Portable Library query.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryQuery {
    pub text: String,
    pub limit: usize,
}

impl LibraryQuery {
    /// Creates a query.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            limit: 20,
        }
    }

    /// Sets result limit.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Query evidence hit.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LibraryQueryHit {
    pub uri: KnowledgeUri,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub score: Option<f64>,
    pub metadata: BTreeMap<String, String>,
}

/// Query result envelope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LibraryQueryResult {
    pub answer: Option<String>,
    pub hits: Vec<LibraryQueryHit>,
    pub provider: String,
    pub strategy: QueryStrategy,
    pub provenance: BTreeMap<String, String>,
}

/// Polymorphic Library provider.
pub trait LibraryProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn capabilities(&self) -> BTreeSet<LibraryCapability>;
    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        let _ = library;
        Err(LibraryError::UnsupportedCapability(LibraryCapability::Catalog))
    }
    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        let _ = (library, path);
        Err(LibraryError::UnsupportedCapability(LibraryCapability::List))
    }
    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        let _ = uri;
        Err(LibraryError::UnsupportedCapability(LibraryCapability::Read))
    }
    fn query(&self, library: &LibraryId, query: &LibraryQuery) -> LibraryResult<LibraryQueryResult> {
        let _ = (library, query);
        Err(LibraryError::UnsupportedCapability(LibraryCapability::Query))
    }
    fn refresh(&self) -> LibraryResult<()> {
        Err(LibraryError::UnsupportedCapability(LibraryCapability::Refresh))
    }
}

/// Resolved Library instance.
#[derive(Clone)]
pub struct LibraryInstance {
    manifest: LibraryManifest,
    provider: Arc<dyn LibraryProvider>,
}

impl fmt::Debug for LibraryInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LibraryInstance")
            .field("manifest", &self.manifest)
            .field("provider_id", &self.provider.provider_id())
            .finish()
    }
}

impl LibraryInstance {
    /// Creates a resolved Library.
    pub fn new(manifest: LibraryManifest, provider: Arc<dyn LibraryProvider>) -> Self {
        Self { manifest, provider }
    }
    /// Manifest.
    pub fn manifest(&self) -> &LibraryManifest {
        &self.manifest
    }
    /// Provider.
    pub fn provider(&self) -> &Arc<dyn LibraryProvider> {
        &self.provider
    }
}

/// Dynamic Library registry and mount table.
#[derive(Default)]
pub struct LibraryRegistry {
    registered: BTreeMap<LibraryId, LibraryInstance>,
    mounted: BTreeSet<LibraryId>,
}

impl LibraryRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }
    /// Registers a Library.
    pub fn register(&mut self, library: LibraryInstance) -> LibraryResult<()> {
        let id = library.manifest.id.clone();
        if self.registered.contains_key(&id) {
            return Err(LibraryError::Conflict(id.to_string()));
        }
        self.registered.insert(id, library);
        Ok(())
    }
    /// Unregisters a Library.
    pub fn unregister(&mut self, id: &LibraryId) -> LibraryResult<LibraryInstance> {
        self.mounted.remove(id);
        self.registered
            .remove(id)
            .ok_or_else(|| LibraryError::UnknownLibrary(id.to_string()))
    }
    /// Mounts a registered Library.
    pub fn mount(&mut self, id: &LibraryId) -> LibraryResult<()> {
        if !self.registered.contains_key(id) {
            return Err(LibraryError::UnknownLibrary(id.to_string()));
        }
        self.mounted.insert(id.clone());
        Ok(())
    }
    /// Unmounts a Library.
    pub fn unmount(&mut self, id: &LibraryId) -> LibraryResult<()> {
        if !self.registered.contains_key(id) {
            return Err(LibraryError::UnknownLibrary(id.to_string()));
        }
        self.mounted.remove(id);
        Ok(())
    }
    /// Registered manifests.
    pub fn libraries(&self) -> Vec<&LibraryManifest> {
        self.registered.values().map(LibraryInstance::manifest).collect()
    }
    /// Mounted identifiers.
    pub fn mounted(&self) -> impl Iterator<Item = &LibraryId> {
        self.mounted.iter()
    }
    /// Mount state.
    pub fn is_mounted(&self, id: &LibraryId) -> bool {
        self.mounted.contains(id)
    }
    /// Returns one catalog.
    pub fn catalog(&self, id: &LibraryId) -> LibraryResult<LibraryCatalog> {
        let library = self.mounted_library(id)?;
        require_capability(library.provider.as_ref(), LibraryCapability::Catalog)?;
        library.provider.catalog(id)
    }
    /// Aggregates mounted catalogs.
    pub fn global_catalog(&self) -> LibraryResult<Vec<LibraryCatalog>> {
        self.mounted
            .iter()
            .map(|id| self.catalog(id))
            .collect::<LibraryResult<Vec<_>>>()
    }
    /// Lists logical nodes.
    pub fn list(&self, id: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        let library = self.mounted_library(id)?;
        require_capability(library.provider.as_ref(), LibraryCapability::List)?;
        library.provider.list(id, path)
    }
    /// Reads a logical node.
    pub fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        let library = self.mounted_library(uri.library())?;
        require_capability(library.provider.as_ref(), LibraryCapability::Read)?;
        library.provider.read(uri)
    }
    /// Queries one Library.
    pub fn query(&self, id: &LibraryId, query: &LibraryQuery) -> LibraryResult<LibraryQueryResult> {
        let library = self.mounted_library(id)?;
        require_capability(library.provider.as_ref(), LibraryCapability::Query)?;
        library.provider.query(id, query)
    }
    /// Queries every mounted query-capable Library.
    pub fn query_all(&self, query: &LibraryQuery) -> Vec<(LibraryId, LibraryResult<LibraryQueryResult>)> {
        self.mounted
            .iter()
            .filter_map(|id| {
                let library = self.registered.get(id)?;
                if library.provider.capabilities().contains(&LibraryCapability::Query) {
                    Some((id.clone(), library.provider.query(id, query)))
                } else {
                    None
                }
            })
            .collect()
    }
    fn mounted_library(&self, id: &LibraryId) -> LibraryResult<&LibraryInstance> {
        let library = self
            .registered
            .get(id)
            .ok_or_else(|| LibraryError::UnknownLibrary(id.to_string()))?;
        if !self.mounted.contains(id) {
            return Err(LibraryError::NotMounted(id.to_string()));
        }
        Ok(library)
    }
}

fn require_capability(provider: &dyn LibraryProvider, capability: LibraryCapability) -> LibraryResult<()> {
    if provider.capabilities().contains(&capability) {
        Ok(())
    } else {
        Err(LibraryError::UnsupportedCapability(capability))
    }
}
