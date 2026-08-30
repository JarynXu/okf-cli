//! Reference Library providers.

use std::collections::{BTreeMap, BTreeSet};

use crate::library::{
    CatalogEntry, KnowledgeNode, KnowledgeNodeKind, KnowledgeUri, LibraryCapability,
    LibraryCatalog, LibraryError, LibraryId, LibraryProvider, LibraryQuery, LibraryQueryHit,
    LibraryQueryResult, LibraryResult, QueryStrategy,
};
use crate::model::Bundle;
use crate::retrieval::SearchQuery;

/// Exposes an already parsed OKF [`Bundle`] through the generic Library provider contract.
#[derive(Clone, Debug)]
pub struct BundleLibraryProvider {
    bundle: Bundle,
    catalog: Option<LibraryCatalog>,
}

impl BundleLibraryProvider {
    /// Creates a provider for an OKF bundle with a catalog derived from document metadata.
    pub fn new(bundle: Bundle) -> Self {
        Self {
            bundle,
            catalog: None,
        }
    }

    /// Uses a Library-owned semantic catalog instead of deriving one document-per-entry.
    pub fn with_catalog(mut self, catalog: LibraryCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Returns the backing bundle.
    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }
}

impl LibraryProvider for BundleLibraryProvider {
    fn provider_id(&self) -> &str {
        "okf-bundle"
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        [
            LibraryCapability::Catalog,
            LibraryCapability::List,
            LibraryCapability::Read,
            LibraryCapability::Query,
        ]
        .into_iter()
        .collect()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        if let Some(catalog) = &self.catalog {
            if &catalog.library != library {
                return Err(LibraryError::Provider(format!(
                    "catalog belongs to '{}' but provider is mounted as '{}'",
                    catalog.library, library
                )));
            }
            return Ok(catalog.clone());
        }

        let entries = self
            .bundle
            .documents()
            .map(|document| {
                let mut terms = BTreeSet::new();
                terms.extend(document.metadata().tags.iter().cloned());
                terms.extend(document.metadata().aliases.iter().cloned());
                CatalogEntry {
                    id: document.id().to_string(),
                    title: document.title().to_owned(),
                    description: document.metadata().summary.clone(),
                    uri: KnowledgeUri::new(library.clone(), document.id().as_str())
                        .expect("document identifiers are valid logical paths"),
                    terms,
                }
            })
            .collect();
        Ok(LibraryCatalog {
            library: library.clone(),
            entries,
        })
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        let prefix = path.trim_matches('/');
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut children = BTreeMap::<String, KnowledgeNodeKind>::new();

        for id in self.bundle.ids() {
            let id = id.as_str();
            let Some(remainder) = id.strip_prefix(&prefix) else {
                continue;
            };
            if remainder.is_empty() {
                continue;
            }
            let (child, rest) = remainder
                .split_once('/')
                .map_or((remainder, None), |(child, rest)| (child, Some(rest)));
            let child_path = if prefix.is_empty() {
                child.to_owned()
            } else {
                format!("{}{child}", prefix)
            };
            let kind = if rest.is_some() {
                KnowledgeNodeKind::Directory
            } else {
                KnowledgeNodeKind::Content
            };
            children
                .entry(child_path)
                .and_modify(|existing| {
                    if kind == KnowledgeNodeKind::Directory {
                        *existing = kind;
                    }
                })
                .or_insert(kind);
        }

        Ok(children
            .into_iter()
            .map(|(path, kind)| KnowledgeNode {
                uri: KnowledgeUri::new(library.clone(), path)
                    .expect("derived bundle paths are valid logical paths"),
                kind,
                title: None,
                virtual_node: false,
            })
            .collect())
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.bundle
            .resolve(uri.path())
            .map(|document| document.body().to_owned())
            .ok_or_else(|| LibraryError::NodeNotFound(uri.to_string()))
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        let hits = self
            .bundle
            .search(&SearchQuery::new(&query.text).limit(query.limit))
            .into_iter()
            .map(|hit| LibraryQueryHit {
                uri: KnowledgeUri::new(library.clone(), hit.document.id().as_str())
                    .expect("document identifiers are valid logical paths"),
                title: Some(hit.document.title().to_owned()),
                snippet: Some(hit.snippet),
                score: Some(f64::from(hit.score)),
                metadata: BTreeMap::new(),
            })
            .collect();

        Ok(LibraryQueryResult {
            answer: None,
            hits,
            provider: self.provider_id().to_owned(),
            strategy: QueryStrategy::Lexical,
            provenance: BTreeMap::new(),
        })
    }
}

/// Purely virtual provider whose nodes are generated from in-memory state rather than files.
#[derive(Clone, Debug, Default)]
pub struct VirtualLibraryProvider {
    provider_id: String,
    contents: BTreeMap<String, String>,
    catalog: Vec<VirtualCatalogEntry>,
}

#[derive(Clone, Debug)]
struct VirtualCatalogEntry {
    id: String,
    title: String,
    description: Option<String>,
    path: String,
    terms: BTreeSet<String>,
}

impl VirtualLibraryProvider {
    /// Creates an empty virtual provider.
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            contents: BTreeMap::new(),
            catalog: Vec::new(),
        }
    }

    /// Adds or replaces a virtual content node.
    pub fn with_content(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.contents
            .insert(path.into().trim_matches('/').to_owned(), content.into());
        self
    }

    /// Adds a semantic catalog entry.
    pub fn with_catalog_entry(
        mut self,
        id: impl Into<String>,
        title: impl Into<String>,
        path: impl Into<String>,
        terms: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.catalog.push(VirtualCatalogEntry {
            id: id.into(),
            title: title.into(),
            description: None,
            path: path.into().trim_matches('/').to_owned(),
            terms: terms.into_iter().map(Into::into).collect(),
        });
        self
    }
}

impl LibraryProvider for VirtualLibraryProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        [
            LibraryCapability::Catalog,
            LibraryCapability::List,
            LibraryCapability::Read,
            LibraryCapability::Query,
        ]
        .into_iter()
        .collect()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        let entries = self
            .catalog
            .iter()
            .map(|entry| {
                Ok(CatalogEntry {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    description: entry.description.clone(),
                    uri: KnowledgeUri::new(library.clone(), &entry.path)?,
                    terms: entry.terms.clone(),
                })
            })
            .collect::<LibraryResult<Vec<_>>>()?;
        Ok(LibraryCatalog {
            library: library.clone(),
            entries,
        })
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        let base = path.trim_matches('/');
        let prefix = if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        };
        let mut children = BTreeMap::<String, KnowledgeNodeKind>::new();
        for path in self.contents.keys() {
            let Some(remainder) = path.strip_prefix(&prefix) else {
                continue;
            };
            if remainder.is_empty() {
                continue;
            }
            let (child, nested) = remainder
                .split_once('/')
                .map_or((remainder, false), |(child, _)| (child, true));
            let child_path = if prefix.is_empty() {
                child.to_owned()
            } else {
                format!("{}{child}", prefix)
            };
            let kind = if nested {
                KnowledgeNodeKind::Directory
            } else {
                KnowledgeNodeKind::Content
            };
            children
                .entry(child_path)
                .and_modify(|existing| {
                    if kind == KnowledgeNodeKind::Directory {
                        *existing = kind;
                    }
                })
                .or_insert(kind);
        }
        Ok(children
            .into_iter()
            .map(|(path, kind)| KnowledgeNode {
                uri: KnowledgeUri::new(library.clone(), path)
                    .expect("derived virtual paths are valid"),
                kind,
                title: None,
                virtual_node: true,
            })
            .collect())
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.contents
            .get(uri.path())
            .cloned()
            .ok_or_else(|| LibraryError::NodeNotFound(uri.to_string()))
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        if query.limit == 0 {
            return Ok(LibraryQueryResult {
                answer: None,
                hits: Vec::new(),
                provider: self.provider_id.clone(),
                strategy: QueryStrategy::Lexical,
                provenance: BTreeMap::new(),
            });
        }

        let needle = query.text.trim().to_lowercase();
        let mut hits = self
            .contents
            .iter()
            .filter_map(|(path, content)| {
                let haystack = format!("{path}\n{content}").to_lowercase();
                if !needle.is_empty() && !haystack.contains(&needle) {
                    return None;
                }
                let score = if needle.is_empty() {
                    0.0
                } else if path.to_lowercase().contains(&needle) {
                    2.0
                } else {
                    1.0
                };
                Some(LibraryQueryHit {
                    uri: KnowledgeUri::new(library.clone(), path)
                        .expect("stored virtual paths are valid"),
                    title: None,
                    snippet: Some(truncate(content, 180)),
                    score: Some(score),
                    metadata: BTreeMap::new(),
                })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.uri.cmp(&right.uri))
        });
        hits.truncate(query.limit);

        Ok(LibraryQueryResult {
            answer: None,
            hits,
            provider: self.provider_id.clone(),
            strategy: QueryStrategy::Lexical,
            provenance: BTreeMap::new(),
        })
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}
