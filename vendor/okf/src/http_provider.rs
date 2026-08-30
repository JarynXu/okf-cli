//! HTTP-backed Library provider adapter.

use std::collections::BTreeSet;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::library::{
    KnowledgeNode, KnowledgeUri, LibraryCapability, LibraryCatalog, LibraryError, LibraryId,
    LibraryProvider, LibraryQuery, LibraryQueryResult, LibraryResult,
};
use crate::provider_protocol::{ProviderRequest, ProviderResponse};

/// Executes Library capabilities against an `okf-provider/1` HTTP endpoint.
#[derive(Clone, Debug)]
pub struct HttpLibraryProvider {
    id: String,
    library: LibraryId,
    endpoint: String,
    bearer_token: Option<String>,
    capabilities: BTreeSet<LibraryCapability>,
    client: Client,
}
impl HttpLibraryProvider {
    /// Creates a remote provider pointing at a base URL.
    pub fn new(
        id: impl Into<String>,
        library: LibraryId,
        base_url: impl Into<String>,
        capabilities: impl IntoIterator<Item = LibraryCapability>,
    ) -> LibraryResult<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(LibraryError::Provider(
                "HTTP provider base URL must use http:// or https://".into(),
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        Ok(Self {
            id: id.into(),
            library,
            endpoint: format!("{base_url}/v1/execute"),
            bearer_token: None,
            capabilities: capabilities.into_iter().collect(),
            client,
        })
    }
    /// Configures bearer authentication.
    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }
    fn ensure_library(&self, library: &LibraryId) -> LibraryResult<()> {
        if library == &self.library {
            Ok(())
        } else {
            Err(LibraryError::Provider(format!(
                "HTTP provider '{}' belongs to Library '{}' but was called for '{}'",
                self.id, self.library, library
            )))
        }
    }
    fn invoke(&self, request: &ProviderRequest) -> LibraryResult<ProviderResponse> {
        let mut builder = self.client.post(&self.endpoint).json(request);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        builder
            .send()
            .map_err(http_error)?
            .error_for_status()
            .map_err(http_error)?
            .json()
            .map_err(http_error)
    }
}
impl LibraryProvider for HttpLibraryProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }
    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.capabilities.clone()
    }
    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        self.ensure_library(library)?;
        self.invoke(&ProviderRequest::catalog(library.clone()))?
            .into_catalog()
    }
    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        self.ensure_library(library)?;
        self.invoke(&ProviderRequest::list(library.clone(), path))?
            .into_nodes()
    }
    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.ensure_library(uri.library())?;
        self.invoke(&ProviderRequest::read(uri.clone()))?
            .into_typed()
    }
    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        self.ensure_library(library)?;
        self.invoke(&ProviderRequest::query(library.clone(), query.clone()))?
            .into_query_result()
    }
    fn refresh(&self) -> LibraryResult<()> {
        self.invoke(&ProviderRequest::refresh(self.library.clone()))?
            .into_typed()
    }
}
fn http_error(error: impl std::fmt::Display) -> LibraryError {
    LibraryError::Provider(format!("HTTP provider error: {error}"))
}
