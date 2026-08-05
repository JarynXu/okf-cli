//! Bundle storage and lookup APIs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::retrieval::{SearchHit, SearchQuery, search};

use super::{Document, DocumentId};

/// An in-memory OKF bundle with deterministic document ordering.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bundle { root: PathBuf, documents: BTreeMap<DocumentId, Document> }

impl Bundle {
    /// Creates an empty bundle rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into(), documents: BTreeMap::new() } }
    /// Returns the bundle root.
    pub fn root(&self) -> &Path { &self.root }
    /// Returns the number of documents.
    pub fn len(&self) -> usize { self.documents.len() }
    /// Returns whether the bundle has no documents.
    pub fn is_empty(&self) -> bool { self.documents.is_empty() }
    /// Inserts a document and rejects duplicate identifiers.
    pub fn insert(&mut self, document: Document) -> Result<()> {
        if let Some(existing) = self.documents.get(document.id()) {
            return Err(Error::DuplicateDocument {
                id: document.id().to_string(),
                first_path: existing.source_path().map_or_else(|| PathBuf::from("<memory>"), Path::to_path_buf),
                second_path: document.source_path().map_or_else(|| PathBuf::from("<memory>"), Path::to_path_buf),
            });
        }
        self.documents.insert(document.id().clone(), document);
        Ok(())
    }
    /// Returns a document by canonical identifier.
    pub fn get(&self, id: &DocumentId) -> Option<&Document> { self.documents.get(id) }
    /// Returns a document by canonical identifier string.
    pub fn get_by_id(&self, id: &str) -> Option<&Document> { self.documents.get(id) }
    /// Resolves a canonical identifier first, then an alias.
    pub fn resolve(&self, id_or_alias: &str) -> Option<&Document> {
        self.get_by_id(id_or_alias).or_else(|| self.documents.values().find(|document| document.metadata().aliases.contains(id_or_alias)))
    }
    /// Iterates over documents in identifier order.
    pub fn documents(&self) -> impl ExactSizeIterator<Item = &Document> { self.documents.values() }
    /// Iterates over canonical identifiers in sorted order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = &DocumentId> { self.documents.keys() }
    /// Runs deterministic in-memory retrieval over this bundle.
    pub fn search<'a>(&'a self, query: &SearchQuery) -> Vec<SearchHit<'a>> { search(self, query) }
}

impl Default for Bundle { fn default() -> Self { Self::new(".") } }
