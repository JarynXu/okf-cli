//! Language-neutral request/response model for external Library providers.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::library::{
    CatalogEntry, KnowledgeNode, KnowledgeNodeKind, KnowledgeUri, LibraryCatalog, LibraryError,
    LibraryId, LibraryQuery, LibraryQueryHit, LibraryQueryResult, LibraryResult, QueryStrategy,
};

/// Stable provider protocol identifier.
pub const PROVIDER_PROTOCOL_V1: &str = "okf-provider/1";

/// External provider operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOperation {
    /// Return semantic catalog.
    Catalog,
    /// List direct children.
    List,
    /// Read canonical URI.
    Read,
    /// Execute query.
    Query,
    /// Refresh state.
    Refresh,
}

/// Portable provider request. URIs are canonical `okf://...` strings on the wire.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderRequest {
    /// Protocol identifier.
    pub protocol: String,
    /// Operation.
    pub operation: ProviderOperation,
    /// Library identity.
    pub library: LibraryId,
    /// Path for list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Canonical URI for read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Query payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<LibraryQuery>,
}

impl ProviderRequest {
    /// Catalog request.
    pub fn catalog(library: LibraryId) -> Self {
        Self::new(library, ProviderOperation::Catalog)
    }
    /// List request.
    pub fn list(library: LibraryId, path: impl Into<String>) -> Self {
        let mut value = Self::new(library, ProviderOperation::List);
        value.path = Some(path.into());
        value
    }
    /// Read request.
    pub fn read(uri: KnowledgeUri) -> Self {
        let mut value = Self::new(uri.library().clone(), ProviderOperation::Read);
        value.uri = Some(uri.to_string());
        value
    }
    /// Query request.
    pub fn query(library: LibraryId, query: LibraryQuery) -> Self {
        let mut value = Self::new(library, ProviderOperation::Query);
        value.query = Some(query);
        value
    }
    /// Refresh request.
    pub fn refresh(library: LibraryId) -> Self {
        Self::new(library, ProviderOperation::Refresh)
    }
    fn new(library: LibraryId, operation: ProviderOperation) -> Self {
        Self {
            protocol: PROVIDER_PROTOCOL_V1.to_owned(),
            operation,
            library,
            path: None,
            uri: None,
            query: None,
        }
    }
}

/// Portable provider error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderProtocolError {
    /// Stable code.
    pub code: String,
    /// Diagnostic.
    pub message: String,
}

/// Portable provider response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderResponse {
    /// Success flag.
    pub ok: bool,
    /// Result payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Failure diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderProtocolError>,
}

impl ProviderResponse {
    /// Decode ordinary payload.
    pub fn into_typed<T: DeserializeOwned>(self) -> LibraryResult<T> {
        serde_json::from_value(self.success_data()?).map_err(protocol_error)
    }
    /// Decode catalog with canonical URI strings.
    pub fn into_catalog(self) -> LibraryResult<LibraryCatalog> {
        let wire: WireCatalog =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        let library = LibraryId::parse(wire.library)?;
        let entries = wire
            .entries
            .into_iter()
            .map(|entry| {
                let uri = KnowledgeUri::parse(&entry.uri)?;
                if uri.library() != &library {
                    return Err(LibraryError::Provider(format!(
                        "catalog URI '{uri}' does not belong to Library '{library}'"
                    )));
                }
                Ok(CatalogEntry {
                    id: entry.id,
                    title: entry.title,
                    description: entry.description,
                    uri,
                    terms: entry.terms,
                })
            })
            .collect::<LibraryResult<Vec<_>>>()?;
        Ok(LibraryCatalog { library, entries })
    }
    /// Decode logical nodes with canonical URI strings.
    pub fn into_nodes(self) -> LibraryResult<Vec<KnowledgeNode>> {
        let values: Vec<WireNode> =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        values
            .into_iter()
            .map(|value| {
                Ok(KnowledgeNode {
                    uri: KnowledgeUri::parse(&value.uri)?,
                    kind: value.kind,
                    title: value.title,
                    virtual_node: value.virtual_node,
                })
            })
            .collect()
    }
    /// Decode query result with canonical URI strings.
    pub fn into_query_result(self) -> LibraryResult<LibraryQueryResult> {
        let value: WireQueryResult =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        let hits = value
            .hits
            .into_iter()
            .map(|hit| {
                Ok(LibraryQueryHit {
                    uri: KnowledgeUri::parse(&hit.uri)?,
                    title: hit.title,
                    snippet: hit.snippet,
                    score: hit.score,
                    metadata: hit.metadata,
                })
            })
            .collect::<LibraryResult<Vec<_>>>()?;
        Ok(LibraryQueryResult {
            answer: value.answer,
            hits,
            provider: value.provider,
            strategy: value.strategy,
            provenance: value.provenance,
        })
    }
    fn success_data(self) -> LibraryResult<Value> {
        if !self.ok {
            let error = self.error.unwrap_or(ProviderProtocolError {
                code: "provider-error".into(),
                message: "external provider failed without a diagnostic".into(),
            });
            return Err(LibraryError::Provider(format!(
                "{}: {}",
                error.code, error.message
            )));
        }
        Ok(self.data.unwrap_or(Value::Null))
    }
}

#[derive(Deserialize)]
struct WireCatalog {
    library: String,
    entries: Vec<WireCatalogEntry>,
}
#[derive(Deserialize)]
struct WireCatalogEntry {
    id: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    uri: String,
    #[serde(default)]
    terms: BTreeSet<String>,
}
#[derive(Deserialize)]
struct WireNode {
    uri: String,
    kind: KnowledgeNodeKind,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    virtual_node: bool,
}
#[derive(Deserialize)]
struct WireQueryResult {
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    hits: Vec<WireQueryHit>,
    provider: String,
    strategy: QueryStrategy,
    #[serde(default)]
    provenance: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct WireQueryHit {
    uri: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

/// Decode and validate request.
pub fn decode_provider_request(bytes: &[u8]) -> LibraryResult<ProviderRequest> {
    let request: ProviderRequest = serde_json::from_slice(bytes).map_err(protocol_error)?;
    if request.protocol != PROVIDER_PROTOCOL_V1 {
        return Err(LibraryError::Provider(format!(
            "unsupported provider protocol '{}'",
            request.protocol
        )));
    }
    Ok(request)
}

/// Decode response.
pub fn decode_provider_response(bytes: &[u8]) -> LibraryResult<ProviderResponse> {
    serde_json::from_slice(bytes).map_err(protocol_error)
}

fn protocol_error(error: impl std::fmt::Display) -> LibraryError {
    LibraryError::Provider(format!("provider protocol error: {error}"))
}
