//! Document identifiers and document content.

use std::borrow::Borrow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::Metadata;

/// A stable, portable identifier for a document within a bundle.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DocumentId(String);

impl DocumentId {
    /// Creates and validates a document identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidDocumentId> {
        let value = value.into();
        if value.is_empty() {
            return Err(InvalidDocumentId::new(value, "identifier is empty"));
        }
        if value.trim() != value {
            return Err(InvalidDocumentId::new(
                value,
                "leading or trailing whitespace is not allowed",
            ));
        }
        if value.starts_with('/') || value.ends_with('/') {
            return Err(InvalidDocumentId::new(
                value,
                "leading or trailing '/' is not allowed",
            ));
        }
        for segment in value.split('/') {
            if segment.is_empty() {
                return Err(InvalidDocumentId::new(value, "empty path segment"));
            }
            if matches!(segment, "." | "..") {
                return Err(InvalidDocumentId::new(
                    value,
                    "'.' and '..' path segments are not allowed",
                ));
            }
            if let Some(character) = segment.chars().find(|character| {
                !character.is_alphanumeric() && !matches!(*character, '-' | '_' | '.')
            }) {
                return Err(InvalidDocumentId::new(
                    value,
                    format!("character '{character}' is not allowed"),
                ));
            }
        }
        Ok(Self(value))
    }
    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Returns the final identifier segment.
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(self.0.as_str())
    }
}
impl AsRef<str> for DocumentId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl Borrow<str> for DocumentId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
impl FromStr for DocumentId {
    type Err = InvalidDocumentId;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}
impl<'de> Deserialize<'de> for DocumentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Details explaining why a document identifier was rejected.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("invalid document id '{value}': {reason}")]
pub struct InvalidDocumentId {
    value: String,
    reason: String,
}
impl InvalidDocumentId {
    fn new(value: String, reason: impl Into<String>) -> Self {
        Self {
            value,
            reason: reason.into(),
        }
    }
    /// Returns the rejected identifier.
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Returns a human-readable rejection reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// A parsed Markdown document in an OKF bundle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Document {
    id: DocumentId,
    title: String,
    body: String,
    metadata: Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_path: Option<PathBuf>,
}
impl Document {
    /// Creates a document from validated parts.
    pub fn new(
        id: DocumentId,
        title: impl Into<String>,
        body: impl Into<String>,
        metadata: Metadata,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            body: body.into(),
            metadata,
            source_path: None,
        }
    }
    /// Attaches the document's path relative to the bundle root.
    pub fn with_source_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.source_path = Some(path.into());
        self
    }
    /// Returns the stable document identifier.
    pub fn id(&self) -> &DocumentId {
        &self.id
    }
    /// Returns the display title.
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Returns the Markdown body without front matter.
    pub fn body(&self) -> &str {
        &self.body
    }
    /// Returns structured document metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    /// Returns the path relative to the bundle root when known.
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }
}
