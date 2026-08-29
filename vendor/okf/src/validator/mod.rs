//! Deterministic structural validation for OKF bundles.

use crate::graph::KnowledgeGraph;
use crate::model::{Bundle, DocumentId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}
/// One machine-readable validation diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DocumentId>,
}
impl ValidationIssue {
    fn new(
        severity: Severity,
        code: &str,
        message: impl Into<String>,
        document: Option<DocumentId>,
    ) -> Self {
        Self {
            severity,
            code: code.to_owned(),
            message: message.into(),
            document,
        }
    }
}
/// Validation behavior controls.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationOptions {
    pub unresolved_references_are_errors: bool,
    pub warn_on_self_references: bool,
    pub warn_on_orphans: bool,
}
impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            unresolved_references_are_errors: true,
            warn_on_self_references: true,
            warn_on_orphans: true,
        }
    }
}
/// Ordered validation results.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValidationReport {
    issues: Vec<ValidationIssue>,
}
impl ValidationReport {
    /// Returns all diagnostics in deterministic validation order.
    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }
    /// Iterates over error diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
    }
    /// Iterates over warning diagnostics.
    pub fn warnings(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == Severity::Warning)
    }
    /// Returns whether no error diagnostics were produced.
    pub fn is_valid(&self) -> bool {
        self.errors().next().is_none()
    }
    fn push(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }
}
/// Validates bundle structure and references.
#[derive(Clone, Debug, Default)]
pub struct Validator {
    options: ValidationOptions,
}
impl Validator {
    /// Creates a validator with explicit options.
    pub fn new(options: ValidationOptions) -> Self {
        Self { options }
    }
    /// Returns the active validation options.
    pub fn options(&self) -> &ValidationOptions {
        &self.options
    }
    /// Validates a bundle without mutating it.
    pub fn validate(&self, bundle: &Bundle) -> ValidationReport {
        let mut report = ValidationReport::default();
        if bundle.is_empty() {
            report.push(ValidationIssue::new(
                Severity::Warning,
                "OKF001",
                "bundle contains no Markdown documents",
                None,
            ));
            return report;
        }
        let mut aliases = BTreeMap::<String, DocumentId>::new();
        for document in bundle.documents() {
            if document.title().trim().is_empty() {
                report.push(ValidationIssue::new(
                    Severity::Error,
                    "OKF101",
                    "document title is empty",
                    Some(document.id().clone()),
                ));
            }
            for alias in &document.metadata().aliases {
                if let Err(error) = DocumentId::new(alias.clone()) {
                    report.push(ValidationIssue::new(
                        Severity::Error,
                        "OKF102",
                        format!("invalid alias '{alias}': {}", error.reason()),
                        Some(document.id().clone()),
                    ));
                    continue;
                }
                if let Some(canonical) = bundle.get_by_id(alias) {
                    if canonical.id() != document.id() {
                        report.push(ValidationIssue::new(
                            Severity::Error,
                            "OKF103",
                            format!("alias '{alias}' conflicts with a canonical document id"),
                            Some(document.id().clone()),
                        ));
                    }
                }
                if let Some(previous) = aliases.insert(alias.clone(), document.id().clone()) {
                    if previous.as_str() != document.id().as_str() {
                        report.push(ValidationIssue::new(
                            Severity::Error,
                            "OKF104",
                            format!("alias '{alias}' is also declared by '{previous}'"),
                            Some(document.id().clone()),
                        ));
                    }
                }
            }
        }
        for document in bundle.documents() {
            for reference in &document.metadata().links {
                if reference.relation().trim().is_empty() {
                    report.push(ValidationIssue::new(
                        Severity::Error,
                        "OKF201",
                        format!(
                            "reference to '{}' has an empty relation",
                            reference.target()
                        ),
                        Some(document.id().clone()),
                    ));
                }
                match bundle.resolve(reference.target().as_str()) {
                    Some(target)
                        if target.id() == document.id() && self.options.warn_on_self_references =>
                    {
                        report.push(ValidationIssue::new(
                            Severity::Warning,
                            "OKF202",
                            format!(
                                "document references itself through '{}'",
                                reference.relation()
                            ),
                            Some(document.id().clone()),
                        ))
                    }
                    Some(_) => {}
                    None => {
                        let severity = if self.options.unresolved_references_are_errors {
                            Severity::Error
                        } else {
                            Severity::Warning
                        };
                        report.push(ValidationIssue::new(
                            severity,
                            "OKF203",
                            format!("unresolved reference to '{}'", reference.target()),
                            Some(document.id().clone()),
                        ));
                    }
                }
            }
        }
        if self.options.warn_on_orphans && bundle.len() > 1 {
            let graph = KnowledgeGraph::from_bundle(bundle);
            for document in bundle.documents() {
                let has_incoming = graph.incoming(document.id()).next().is_some();
                let has_outgoing = graph.outgoing(document.id()).next().is_some();
                if !has_incoming && !has_outgoing {
                    report.push(ValidationIssue::new(
                        Severity::Warning,
                        "OKF301",
                        "document is disconnected from the resolved knowledge graph",
                        Some(document.id().clone()),
                    ));
                }
            }
        }
        report
    }
}
