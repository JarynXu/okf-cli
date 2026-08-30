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

/// Provider operation transported over process or remote boundaries.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOperation {
    /// Return the Library semantic catalog.
    Catalog,
    /// List direct children below a logical path.
    List,
    /// Read one canonical knowledge URI.
    Read,
    /// Execute a Library query.
    Query,
    /// Refresh provider-derived state.
    Refresh,
}

/// Portable request envelope used by external provider transports.
///
/// Canonical knowledge URIs cross process/HTTP boundaries as `okf://...` strings. This keeps the
/// wire protocol independent of the Rust SDK's internal struct representation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderRequest {
    /// Protocol identifier. Must be [`PROVIDER_PROTOCOL_V1`].
    pub protocol: String,
    /// Requested capability operation.
    pub operation: ProviderOperation,
    /// Active Library identity.
    pub library: LibraryId,
    /// Logical path for `list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Canonical `okf://...` URI string for `read`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Query payload for `query`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<LibraryQuery>,
}

impl ProviderRequest {
    /// Creates a catalog request.
    pub fn catalog(library: LibraryId) -> Self {
        Self::new(library, ProviderOperation::Catalog)
    }

    /// Creates a list request.
    pub fn list(library: LibraryId, path: impl Into<String>) -> Self {
        let mut request = Self::new(library, ProviderOperation::List);
        request.path = Some(path.into());
        request
    }

    /// Creates a read request.
    pub fn read(uri: KnowledgeUri) -> Self {
        let mut request = Self::new(uri.library().clone(), ProviderOperation::Read);
        request.uri = Some(uri.to_string());
        request
    }

    /// Creates a query request.
    pub fn query(library: LibraryId, query: LibraryQuery) -> Self {
        let mut request = Self::new(library, ProviderOperation::Query);
        request.query = Some(query);
        request
    }

    /// Creates a refresh request.
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

/// Portable provider error returned across an external transport.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderProtocolError {
    /// Stable machine-readable error code.
    pub code: String,
    /// Human-readable diagnostic.
    pub message: String,
}

/// Portable response envelope used by process and HTTP provider transports.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProviderResponse {
    /// Whether the provider operation succeeded.
    pub ok: bool,
    /// Successful operation payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Failure diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProviderProtocolError>,
}

impl ProviderResponse {
    /// Creates a successful response from a serializable payload.
    pub fn success<T: Serialize>(value: &T) -> LibraryResult<Self> {
        Ok(Self {
            ok: true,
            data: Some(serde_json::to_value(value).map_err(protocol_error)?),
            error: None,
        })
    }

    /// Encodes a Library catalog using the language-neutral wire representation.
    ///
    /// Canonical knowledge URIs are emitted as `okf://...` strings rather than the Rust
    /// SDK's internal struct representation.
    pub fn success_catalog(catalog: &LibraryCatalog) -> LibraryResult<Self> {
        let entries = catalog
            .entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "id": &entry.id,
                    "title": &entry.title,
                    "description": &entry.description,
                    "uri": entry.uri.to_string(),
                    "terms": &entry.terms,
                })
            })
            .collect::<Vec<_>>();
        Self::success(&serde_json::json!({
            "library": catalog.library.as_str(),
            "entries": entries,
        }))
    }

    /// Encodes logical knowledge nodes using canonical URI strings.
    pub fn success_nodes(nodes: &[KnowledgeNode]) -> LibraryResult<Self> {
        let nodes = nodes
            .iter()
            .map(|node| {
                serde_json::json!({
                    "uri": node.uri.to_string(),
                    "kind": node.kind,
                    "title": &node.title,
                    "virtual_node": node.virtual_node,
                })
            })
            .collect::<Vec<_>>();
        Self::success(&nodes)
    }

    /// Encodes a query result using the language-neutral provider wire representation.
    pub fn success_query_result(result: &LibraryQueryResult) -> LibraryResult<Self> {
        let hits = result
            .hits
            .iter()
            .map(|hit| {
                serde_json::json!({
                    "uri": hit.uri.to_string(),
                    "title": &hit.title,
                    "snippet": &hit.snippet,
                    "score": hit.score,
                    "metadata": &hit.metadata,
                })
            })
            .collect::<Vec<_>>();
        Self::success(&serde_json::json!({
            "answer": &result.answer,
            "hits": hits,
            "provider": &result.provider,
            "strategy": &result.strategy,
            "provenance": &result.provenance,
        }))
    }

    /// Creates a failed response.
    pub fn failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(ProviderProtocolError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    /// Decodes a successful payload into a transport-native value.
    pub fn into_typed<T: DeserializeOwned>(self) -> LibraryResult<T> {
        serde_json::from_value(self.success_data()?).map_err(protocol_error)
    }

    /// Decodes a language-neutral catalog payload into SDK domain values.
    pub fn into_catalog(self) -> LibraryResult<LibraryCatalog> {
        let wire: WireLibraryCatalog =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        wire.try_into()
    }

    /// Decodes language-neutral knowledge nodes into SDK domain values.
    pub fn into_nodes(self) -> LibraryResult<Vec<KnowledgeNode>> {
        let wire: Vec<WireKnowledgeNode> =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        wire.into_iter().map(TryInto::try_into).collect()
    }

    /// Decodes a language-neutral query result into SDK domain values.
    pub fn into_query_result(self) -> LibraryResult<LibraryQueryResult> {
        let wire: WireQueryResult =
            serde_json::from_value(self.success_data()?).map_err(protocol_error)?;
        wire.try_into()
    }

    fn success_data(self) -> LibraryResult<Value> {
        if !self.ok {
            let error = self.error.unwrap_or(ProviderProtocolError {
                code: "provider-error".to_owned(),
                message: "external provider failed without a diagnostic".to_owned(),
            });
            return Err(LibraryError::Provider(format!(
                "{}: {}",
                error.code, error.message
            )));
        }
        Ok(self.data.unwrap_or(Value::Null))
    }
}

#[derive(Debug, Deserialize)]
struct WireLibraryCatalog {
    library: String,
    entries: Vec<WireCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct WireCatalogEntry {
    id: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    uri: String,
    #[serde(default)]
    terms: BTreeSet<String>,
}

impl TryFrom<WireLibraryCatalog> for LibraryCatalog {
    type Error = LibraryError;

    fn try_from(value: WireLibraryCatalog) -> Result<Self, Self::Error> {
        let library = LibraryId::parse(value.library)?;
        let entries = value
            .entries
            .into_iter()
            .map(|entry| {
                let uri = KnowledgeUri::parse(&entry.uri)?;
                if uri.library() != &library {
                    return Err(LibraryError::Provider(format!(
                        "catalog URI '{}' does not belong to Library '{}'",
                        uri, library
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
        Ok(Self { library, entries })
    }
}

#[derive(Debug, Deserialize)]
struct WireKnowledgeNode {
    uri: String,
    kind: KnowledgeNodeKind,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    virtual_node: bool,
}

impl TryFrom<WireKnowledgeNode> for KnowledgeNode {
    type Error = LibraryError;

    fn try_from(value: WireKnowledgeNode) -> Result<Self, Self::Error> {
        Ok(Self {
            uri: KnowledgeUri::parse(&value.uri)?,
            kind: value.kind,
            title: value.title,
            virtual_node: value.virtual_node,
        })
    }
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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

impl TryFrom<WireQueryResult> for LibraryQueryResult {
    type Error = LibraryError;

    fn try_from(value: WireQueryResult) -> Result<Self, Self::Error> {
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
        Ok(Self {
            answer: value.answer,
            hits,
            provider: value.provider,
            strategy: value.strategy,
            provenance: value.provenance,
        })
    }
}

/// Parses and validates a provider request received by an external provider implementation.
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

/// Parses a provider response returned by an external transport.
pub fn decode_provider_response(bytes: &[u8]) -> LibraryResult<ProviderResponse> {
    serde_json::from_slice(bytes).map_err(protocol_error)
}

fn protocol_error(error: impl std::fmt::Display) -> LibraryError {
    LibraryError::Provider(format!("provider protocol error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_canonical_uri_strings() {
        let id = LibraryId::parse("demo").expect("id");
        let uri = KnowledgeUri::new(id, "docs/a").expect("uri");
        let request = ProviderRequest::read(uri);
        let value = serde_json::to_value(request).expect("request json");
        assert_eq!(value["uri"], "okf://demo/docs/a");
    }

    #[test]
    fn language_neutral_catalog_decodes() {
        let response: ProviderResponse = serde_json::from_value(serde_json::json!({
            "ok": true,
            "data": {
                "library": "demo",
                "entries": [{
                    "id": "a",
                    "title": "A",
                    "description": null,
                    "uri": "okf://demo/a",
                    "terms": ["alpha"]
                }]
            }
        }))
        .expect("response");
        let catalog = response.into_catalog().expect("catalog");
        assert_eq!(catalog.entries[0].uri.to_string(), "okf://demo/a");
    }

    #[test]
    fn domain_response_helpers_round_trip() {
        let library = LibraryId::parse("demo").expect("library");
        let uri = KnowledgeUri::new(library.clone(), "current/architecture").expect("uri");
        let catalog = LibraryCatalog {
            library: library.clone(),
            entries: vec![CatalogEntry {
                id: "architecture".to_owned(),
                title: "Architecture".to_owned(),
                description: Some("Current architecture".to_owned()),
                uri: uri.clone(),
                terms: ["architecture".to_owned()].into_iter().collect(),
            }],
        };
        let decoded_catalog = ProviderResponse::success_catalog(&catalog)
            .expect("catalog response")
            .into_catalog()
            .expect("decoded catalog");
        assert_eq!(decoded_catalog, catalog);

        let nodes = vec![KnowledgeNode {
            uri: uri.clone(),
            kind: KnowledgeNodeKind::Content,
            title: Some("Architecture".to_owned()),
            virtual_node: true,
        }];
        let decoded_nodes = ProviderResponse::success_nodes(&nodes)
            .expect("node response")
            .into_nodes()
            .expect("decoded nodes");
        assert_eq!(decoded_nodes, nodes);

        let result = LibraryQueryResult {
            answer: Some("Use the architecture topic.".to_owned()),
            hits: vec![LibraryQueryHit {
                uri,
                title: Some("Architecture".to_owned()),
                snippet: Some("Current architecture".to_owned()),
                score: Some(1.0),
                metadata: BTreeMap::new(),
            }],
            provider: "project-context".to_owned(),
            strategy: QueryStrategy::Lexical,
            provenance: BTreeMap::new(),
        };
        let decoded_result = ProviderResponse::success_query_result(&result)
            .expect("query response")
            .into_query_result()
            .expect("decoded query result");
        assert_eq!(decoded_result, result);
    }
}
