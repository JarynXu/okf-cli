//! Ordered composition of multiple Library providers.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::library::{
    KnowledgeNode, KnowledgeUri, LibraryCapability, LibraryCatalog, LibraryError, LibraryId,
    LibraryProvider, LibraryQuery, LibraryQueryResult, LibraryResult,
};

/// Deterministic capability-oriented provider composition.
#[derive(Default)]
pub struct ProviderStack {
    id: String,
    providers: Vec<Arc<dyn LibraryProvider>>,
}
impl std::fmt::Debug for ProviderStack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStack")
            .field("id", &self.id)
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|provider| provider.provider_id())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}
impl ProviderStack {
    /// Creates an empty stack.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            providers: Vec::new(),
        }
    }
    /// Adds a provider at the lowest remaining precedence.
    pub fn push(&mut self, provider: Arc<dyn LibraryProvider>) {
        self.providers.push(provider);
    }
    fn provider_for(&self, capability: LibraryCapability) -> LibraryResult<&dyn LibraryProvider> {
        self.providers
            .iter()
            .find(|provider| provider.capabilities().contains(&capability))
            .map(|provider| provider.as_ref())
            .ok_or(LibraryError::UnsupportedCapability(capability))
    }
}
impl LibraryProvider for ProviderStack {
    fn provider_id(&self) -> &str {
        &self.id
    }
    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.providers
            .iter()
            .flat_map(|provider| provider.capabilities())
            .collect()
    }
    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        self.provider_for(LibraryCapability::Catalog)?
            .catalog(library)
    }
    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        self.provider_for(LibraryCapability::List)?
            .list(library, path)
    }
    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.provider_for(LibraryCapability::Read)?.read(uri)
    }
    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        self.provider_for(LibraryCapability::Query)?
            .query(library, query)
    }
    fn refresh(&self) -> LibraryResult<()> {
        self.provider_for(LibraryCapability::Refresh)?.refresh()
    }
}
