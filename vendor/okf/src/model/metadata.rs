//! Extensible document metadata and directed references.

use std::collections::{BTreeMap, BTreeSet};
use serde::{Deserialize, Deserializer, Serialize};
use super::DocumentId;

/// A directed relation from one document to another.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Reference { target: DocumentId, relation: String }
impl Reference {
    /// Creates a reference with the supplied relation name.
    pub fn new(target: DocumentId, relation: impl Into<String>) -> Self { Self { target, relation: relation.into() } }
    /// Creates a `related` reference.
    pub fn related(target: DocumentId) -> Self { Self::new(target, "related") }
    /// Returns the referenced identifier or alias.
    pub fn target(&self) -> &DocumentId { &self.target }
    /// Returns the relation name.
    pub fn relation(&self) -> &str { &self.relation }
}
impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ReferenceInput { Shorthand(DocumentId), Detailed { target: DocumentId, #[serde(default = "default_relation")] relation: String } }
        fn default_relation() -> String { "related".to_owned() }
        match ReferenceInput::deserialize(deserializer)? {
            ReferenceInput::Shorthand(target) => Ok(Self::related(target)),
            ReferenceInput::Detailed { target, relation } => Ok(Self::new(target, relation)),
        }
    }
}

/// Extensible metadata attached to a document.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Metadata {
    /// Short human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Deterministic set of labels.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub tags: BTreeSet<String>,
    /// Alternative identifier-like names.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub aliases: BTreeSet<String>,
    /// Directed links to other documents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Reference>,
    /// Unrecognized front-matter fields preserved for higher-level applications.
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
