//! Portable `okf-library.yaml` package manifest.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::library::{CatalogEntry, KnowledgeUri, LibraryCatalog, LibraryId, LibraryManifest, LibrarySource};

/// Canonical Library package manifest filename.
pub const LIBRARY_MANIFEST_FILENAME: &str = "okf-library.yaml";

/// Errors produced while loading a Library package manifest.
#[derive(Debug, Error)]
pub enum LibraryManifestError {
    #[error("failed to read Library manifest {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("failed to parse Library manifest {path}: {message}")]
    Parse { path: String, message: String },
    #[error("unsupported Library manifest schema version: {0}")]
    UnsupportedSchema(String),
    #[error("invalid Library manifest: {0}")]
    Invalid(String),
}

/// Portable package declaration read from `okf-library.yaml`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryPackageManifest {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub catalog: Vec<LibraryCatalogDeclaration>,
    #[serde(default)]
    pub query: LibraryQueryDeclaration,
}

impl LibraryPackageManifest {
    /// Parses YAML text.
    pub fn parse_yaml(source: &str) -> Result<Self, LibraryManifestError> {
        let manifest = yaml_serde::from_str::<Self>(source).map_err(|error| LibraryManifestError::Parse {
            path: LIBRARY_MANIFEST_FILENAME.to_owned(),
            message: error.to_string(),
        })?;
        manifest.validate()?;
        Ok(manifest)
    }
    /// Loads `okf-library.yaml` from a root directory.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, LibraryManifestError> {
        let path = root.as_ref().join(LIBRARY_MANIFEST_FILENAME);
        let source = fs::read_to_string(&path).map_err(|source| LibraryManifestError::Io {
            path: path.display().to_string(), source,
        })?;
        let manifest = yaml_serde::from_str::<Self>(&source).map_err(|error| LibraryManifestError::Parse {
            path: path.display().to_string(), message: error.to_string(),
        })?;
        manifest.validate()?;
        Ok(manifest)
    }
    /// Returns whether the canonical manifest exists.
    pub fn exists(root: impl AsRef<Path>) -> bool {
        root.as_ref().join(LIBRARY_MANIFEST_FILENAME).is_file()
    }
    /// Validates portable fields.
    pub fn validate(&self) -> Result<(), LibraryManifestError> {
        if self.schema_version != "1" {
            return Err(LibraryManifestError::UnsupportedSchema(self.schema_version.clone()));
        }
        let id = LibraryId::parse(self.id.clone()).map_err(|error| LibraryManifestError::Invalid(error.to_string()))?;
        for entry in &self.catalog {
            KnowledgeUri::new(id.clone(), &entry.path).map_err(|error| LibraryManifestError::Invalid(error.to_string()))?;
            if entry.id.trim().is_empty() || entry.title.trim().is_empty() {
                return Err(LibraryManifestError::Invalid("catalog entry id and title must be non-empty".to_owned()));
            }
        }
        Ok(())
    }
    /// Converts package identity fields into a runtime manifest.
    pub fn runtime_manifest(&self, source: Option<LibrarySource>) -> Result<LibraryManifest, LibraryManifestError> {
        let mut manifest = LibraryManifest::new(
            LibraryId::parse(self.id.clone()).map_err(|error| LibraryManifestError::Invalid(error.to_string()))?,
            self.name.clone(),
        );
        manifest.version = self.version.clone();
        manifest.source = source;
        Ok(manifest)
    }
    /// Resolves semantic catalog declarations into canonical URIs.
    pub fn runtime_catalog(&self) -> Result<LibraryCatalog, LibraryManifestError> {
        let id = LibraryId::parse(self.id.clone()).map_err(|error| LibraryManifestError::Invalid(error.to_string()))?;
        let entries = self.catalog.iter().map(|entry| {
            Ok(CatalogEntry {
                id: entry.id.clone(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                uri: KnowledgeUri::new(id.clone(), &entry.path).map_err(|error| LibraryManifestError::Invalid(error.to_string()))?,
                terms: entry.terms.clone(),
            })
        }).collect::<Result<Vec<_>, LibraryManifestError>>()?;
        Ok(LibraryCatalog { library: id, entries })
    }
}

/// Semantic topic declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryCatalogDeclaration {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub path: String,
    #[serde(default)]
    pub terms: BTreeSet<String>,
}

/// Retrieval guidance declaration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LibraryQueryDeclaration {
    #[serde(default)]
    pub preferred: Option<String>,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    #[serde(default)]
    pub hints: Vec<String>,
}
