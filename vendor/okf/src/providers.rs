//! Reference Library providers.

use std::collections::{BTreeMap, BTreeSet};

use crate::library::{
    CatalogEntry, KnowledgeNode, KnowledgeNodeKind, KnowledgeUri, LibraryCapability, LibraryCatalog,
    LibraryError, LibraryId, LibraryProvider, LibraryQuery, LibraryQueryHit, LibraryQueryResult,
    LibraryResult, QueryStrategy,
};
use crate::model::Bundle;
use crate::retrieval::SearchQuery;

/// Exposes an OKF Bundle through the Library provider contract.
#[derive(Clone, Debug)]
pub struct BundleLibraryProvider {
    bundle: Bundle,
}

impl BundleLibraryProvider {
    /// Creates a provider.
    pub fn new(bundle: Bundle) -> Self {
        Self { bundle }
    }
    /// Backing bundle.
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
                        .expect("document identifiers are valid paths"),
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
        let base = path.trim_matches('/');
        let prefix = if base.is_empty() { String::new() } else { format!("{base}/") };
        let mut children = BTreeMap::<String, KnowledgeNodeKind>::new();
        for id in self.bundle.ids() {
            let Some(remainder) = id.as_str().strip_prefix(&prefix) else { continue };
            if remainder.is_empty() { continue; }
            let (child, nested) = remainder
                .split_once('/')
                .map_or((remainder, false), |(child, _)| (child, true));
            let child_path = if prefix.is_empty() { child.to_owned() } else { format!("{}{child}", prefix) };
            let kind = if nested { KnowledgeNodeKind::Directory } else { KnowledgeNodeKind::Content };
            children.entry(child_path).and_modify(|current| {
                if kind == KnowledgeNodeKind::Directory { *current = kind; }
            }).or_insert(kind);
        }
        Ok(children.into_iter().map(|(path, kind)| KnowledgeNode {
            uri: KnowledgeUri::new(library.clone(), path).expect("derived path is valid"),
            kind,
            title: None,
            virtual_node: false,
        }).collect())
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.bundle.resolve(uri.path())
            .map(|document| document.body().to_owned())
            .ok_or_else(|| LibraryError::NodeNotFound(uri.to_string()))
    }

    fn query(&self, library: &LibraryId, query: &LibraryQuery) -> LibraryResult<LibraryQueryResult> {
        let hits = self.bundle
            .search(&SearchQuery::new(&query.text).limit(query.limit))
            .into_iter()
            .map(|hit| LibraryQueryHit {
                uri: KnowledgeUri::new(library.clone(), hit.document.id().as_str())
                    .expect("document identifier is valid"),
                title: Some(hit.document.title().to_owned()),
                snippet: Some(hit.snippet),
                score: Some(f64::from(hit.score)),
                metadata: BTreeMap::new(),
            }).collect();
        Ok(LibraryQueryResult {
            answer: None,
            hits,
            provider: self.provider_id().to_owned(),
            strategy: QueryStrategy::Lexical,
            provenance: BTreeMap::new(),
        })
    }
}

/// Purely virtual/in-memory reference provider.
#[derive(Clone, Debug, Default)]
pub struct VirtualLibraryProvider {
    provider_id: String,
    contents: BTreeMap<String, String>,
}

impl VirtualLibraryProvider {
    /// Creates a virtual provider.
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self { provider_id: provider_id.into(), contents: BTreeMap::new() }
    }
    /// Adds a generated content node.
    pub fn with_content(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.contents.insert(path.into().trim_matches('/').to_owned(), content.into());
        self
    }
}

impl LibraryProvider for VirtualLibraryProvider {
    fn provider_id(&self) -> &str { &self.provider_id }
    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        [LibraryCapability::Catalog, LibraryCapability::List, LibraryCapability::Read, LibraryCapability::Query]
            .into_iter().collect()
    }
    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        Ok(LibraryCatalog {
            library: library.clone(),
            entries: self.contents.keys().map(|path| CatalogEntry {
                id: path.clone(),
                title: path.clone(),
                description: None,
                uri: KnowledgeUri::new(library.clone(), path).expect("stored path is valid"),
                terms: BTreeSet::new(),
            }).collect(),
        })
    }
    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        let base = path.trim_matches('/');
        let prefix = if base.is_empty() { String::new() } else { format!("{base}/") };
        let mut children = BTreeMap::<String, KnowledgeNodeKind>::new();
        for path in self.contents.keys() {
            let Some(remainder) = path.strip_prefix(&prefix) else { continue };
            if remainder.is_empty() { continue; }
            let (child, nested) = remainder.split_once('/').map_or((remainder, false), |(child, _)| (child, true));
            let child_path = if prefix.is_empty() { child.to_owned() } else { format!("{}{child}", prefix) };
            let kind = if nested { KnowledgeNodeKind::Directory } else { KnowledgeNodeKind::Content };
            children.entry(child_path).and_modify(|current| {
                if kind == KnowledgeNodeKind::Directory { *current = kind; }
            }).or_insert(kind);
        }
        Ok(children.into_iter().map(|(path, kind)| KnowledgeNode {
            uri: KnowledgeUri::new(library.clone(), path).expect("stored path is valid"),
            kind,
            title: None,
            virtual_node: true,
        }).collect())
    }
    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.contents.get(uri.path()).cloned().ok_or_else(|| LibraryError::NodeNotFound(uri.to_string()))
    }
    fn query(&self, library: &LibraryId, query: &LibraryQuery) -> LibraryResult<LibraryQueryResult> {
        let needle = query.text.trim().to_lowercase();
        let mut hits = self.contents.iter().filter_map(|(path, content)| {
            let haystack = format!("{path}\n{content}").to_lowercase();
            if !needle.is_empty() && !haystack.contains(&needle) { return None; }
            Some(LibraryQueryHit {
                uri: KnowledgeUri::new(library.clone(), path).expect("stored path is valid"),
                title: None,
                snippet: Some(content.chars().take(180).collect()),
                score: Some(if path.to_lowercase().contains(&needle) { 2.0 } else { 1.0 }),
                metadata: BTreeMap::new(),
            })
        }).collect::<Vec<_>>();
        hits.sort_by(|left, right| left.uri.cmp(&right.uri));
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
