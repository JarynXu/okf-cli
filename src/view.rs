use std::path::Path;

use okf::{Bundle, Document};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct DocumentSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) summary: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) source_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DocumentView {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) metadata: okf::Metadata,
    pub(crate) source_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InspectionView {
    pub(crate) document: DocumentView,
    pub(crate) incoming: Vec<String>,
    pub(crate) outgoing: Vec<String>,
    pub(crate) unresolved: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchHitView {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) score: u32,
    pub(crate) matched_fields: Vec<String>,
    pub(crate) snippet: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct EdgeView {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) relations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GraphFocus {
    pub(crate) id: String,
    pub(crate) incoming: Vec<String>,
    pub(crate) outgoing: Vec<String>,
    pub(crate) reachable: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GraphView {
    pub(crate) nodes: Vec<String>,
    pub(crate) edges: Vec<EdgeView>,
    pub(crate) roots: Vec<String>,
    pub(crate) leaves: Vec<String>,
    pub(crate) focus: Option<GraphFocus>,
    pub(crate) dot: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitView {
    pub(crate) root: String,
    pub(crate) created: Vec<String>,
}

pub(crate) fn document_summary(document: &Document) -> DocumentSummary {
    DocumentSummary {
        id: document.id().to_string(),
        title: document.title().to_owned(),
        summary: document.metadata().summary.clone(),
        tags: document.metadata().tags.iter().cloned().collect(),
        source_path: document.source_path().map(portable_path),
    }
}

pub(crate) fn document_view(document: &Document) -> DocumentView {
    DocumentView {
        id: document.id().to_string(),
        title: document.title().to_owned(),
        body: document.body().to_owned(),
        metadata: document.metadata().clone(),
        source_path: document.source_path().map(portable_path),
    }
}

pub(crate) fn graph_to_dot(bundle: &Bundle, edges: &[EdgeView]) -> String {
    let mut lines = vec!["digraph okf {".to_owned(), "  rankdir=LR;".to_owned()];
    for document in bundle.documents() {
        lines.push(format!(
            "  \"{}\" [label=\"{}\"];",
            escape_dot(document.id().as_str()),
            escape_dot(document.title())
        ));
    }
    for edge in edges {
        lines.push(format!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            escape_dot(&edge.from),
            escape_dot(&edge.to),
            escape_dot(&edge.relations.join(","))
        ));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

pub(crate) fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn display_values(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_owned()
    } else {
        values.join(", ")
    }
}

fn escape_dot(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_values_are_escaped() {
        assert_eq!(escape_dot("a\\b\"c"), "a\\\\b\\\"c");
    }
}
