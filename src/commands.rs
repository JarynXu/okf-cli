use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use okf::{
    Bundle, BundleParser, Document, KnowledgeGraph, SearchQuery, Severity, ValidationOptions,
    Validator,
};
use serde_json::json;

use crate::cli::{Command, GraphRepresentation};
use crate::output::Outcome;
use crate::view::{
    EdgeView, GraphFocus, GraphView, InitView, InspectionView, SearchHitView, display_values,
    document_summary, document_view, graph_to_dot, portable_path,
};

pub(crate) fn execute(bundle_path: &Path, command: &Command) -> Result<Outcome> {
    match command {
        Command::Init { force } => init_bundle(bundle_path, *force),
        Command::Validate {
            allow_unresolved,
            no_orphans,
            deny_warnings,
        } => validate_bundle(bundle_path, *allow_unresolved, *no_orphans, *deny_warnings),
        Command::List { tag } => list_documents(bundle_path, tag),
        Command::Get { id } => get_document(bundle_path, id),
        Command::Inspect { id } => inspect_document(bundle_path, id),
        Command::Search { query, tag, limit } => search_documents(bundle_path, query, tag, *limit),
        Command::Graph { id, representation } => {
            graph_bundle(bundle_path, id.as_deref(), *representation)
        }
        Command::Library { .. } => {
            bail!("library commands must be dispatched through the Library Runtime")
        }
        Command::Project { .. } => {
            bail!("project commands must be dispatched through the Project Context Runtime")
        }
    }
}

fn load_bundle(path: &Path) -> Result<Bundle> {
    BundleParser::default()
        .parse_dir(path)
        .with_context(|| format!("failed to load bundle at {}", path.display()))
}

fn init_bundle(root: &Path, force: bool) -> Result<Outcome> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    let index = root.join("index.md");
    if index.exists() && !force {
        bail!(
            "{} already exists; pass --force to replace it",
            index.display()
        );
    }

    fs::write(
        &index,
        "---\ntitle: Knowledge bundle\nsummary: Entry point for this bundle.\ntags: [index]\n---\n# Knowledge bundle\n\nDescribe the bundle here.\n",
    )
    .with_context(|| format!("failed to write {}", index.display()))?;

    Outcome::success(
        format!("created {}", index.display()),
        InitView {
            root: portable_path(root),
            created: vec![portable_path(&index)],
        },
    )
}

fn validate_bundle(
    path: &Path,
    allow_unresolved: bool,
    no_orphans: bool,
    deny_warnings: bool,
) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let report = Validator::new(ValidationOptions {
        unresolved_references_are_errors: !allow_unresolved,
        warn_on_self_references: true,
        warn_on_orphans: !no_orphans,
    })
    .validate(&bundle);

    let mut lines = report
        .issues()
        .iter()
        .map(|issue| {
            let document = issue
                .document
                .as_ref()
                .map_or_else(|| "<bundle>".to_owned(), ToString::to_string);
            format!(
                "{} [{}] {}: {}",
                match issue.severity {
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                },
                issue.code,
                document,
                issue.message
            )
        })
        .collect::<Vec<_>>();
    let errors = report.errors().count();
    let warnings = report.warnings().count();
    lines.push(format!(
        "checked {} document(s): {errors} error(s), {warnings} warning(s)",
        bundle.len()
    ));
    let exit_code = u8::from(!report.is_valid() || (deny_warnings && warnings > 0));

    Outcome::success(
        lines.join("\n"),
        json!({
            "documents_checked": bundle.len(),
            "valid": report.is_valid(),
            "errors": errors,
            "warnings": warnings,
            "issues": report.issues(),
        }),
    )
    .map(|outcome| outcome.with_exit_code(exit_code))
}

fn list_documents(path: &Path, required_tags: &[String]) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let required = normalized_tags(required_tags);
    let documents = bundle
        .documents()
        .filter(|document| document_has_tags(document, &required))
        .map(document_summary)
        .collect::<Vec<_>>();
    let human = if documents.is_empty() {
        "no matching documents".to_owned()
    } else {
        documents
            .iter()
            .map(|document| format!("{}\t{}", document.id, document.title))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::success(human, documents)
}

fn get_document(path: &Path, id_or_alias: &str) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let document = bundle
        .resolve(id_or_alias)
        .ok_or_else(|| anyhow!("document '{id_or_alias}' was not found"))?;
    Outcome::success(document.body(), document_view(document))
}

fn inspect_document(path: &Path, id_or_alias: &str) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let document = bundle
        .resolve(id_or_alias)
        .ok_or_else(|| anyhow!("document '{id_or_alias}' was not found"))?;
    let graph = KnowledgeGraph::from_bundle(&bundle);
    let incoming = graph
        .incoming(document.id())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let outgoing = graph
        .outgoing(document.id())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let unresolved = document
        .metadata()
        .links
        .iter()
        .filter(|reference| bundle.resolve(reference.target().as_str()).is_none())
        .map(|reference| reference.target().to_string())
        .collect::<Vec<_>>();
    let view = InspectionView {
        document: document_view(document),
        incoming,
        outgoing,
        unresolved,
    };
    let human = format!(
        "{}\n  title: {}\n  incoming: {}\n  outgoing: {}\n  unresolved: {}",
        view.document.id,
        view.document.title,
        display_values(&view.incoming),
        display_values(&view.outgoing),
        display_values(&view.unresolved)
    );
    Outcome::success(human, view)
}

fn search_documents(path: &Path, text: &str, tags: &[String], limit: usize) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let mut query = SearchQuery::new(text).limit(limit);
    for tag in tags {
        query = query.with_tag(tag);
    }

    let hits = bundle
        .search(&query)
        .into_iter()
        .map(|hit| SearchHitView {
            id: hit.document.id().to_string(),
            title: hit.document.title().to_owned(),
            score: hit.score,
            matched_fields: hit
                .matched_fields
                .iter()
                .map(|field| format!("{field:?}").to_lowercase())
                .collect(),
            snippet: hit.snippet,
        })
        .collect::<Vec<_>>();
    let human = if hits.is_empty() {
        "no matching documents".to_owned()
    } else {
        hits.iter()
            .map(|hit| {
                format!(
                    "{}\t{}\t{}\n  {}",
                    hit.score, hit.id, hit.title, hit.snippet
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::success(human, hits)
}

fn graph_bundle(
    path: &Path,
    id_or_alias: Option<&str>,
    representation: GraphRepresentation,
) -> Result<Outcome> {
    let bundle = load_bundle(path)?;
    let graph = KnowledgeGraph::from_bundle(&bundle);
    let nodes = graph.nodes().map(ToString::to_string).collect::<Vec<_>>();
    let mut edges = Vec::new();
    for source in graph.nodes() {
        for target in graph.outgoing(source) {
            edges.push(EdgeView {
                from: source.to_string(),
                to: target.to_string(),
                relations: graph.relations(source, target).map(str::to_owned).collect(),
            });
        }
    }

    let dot = graph_to_dot(&bundle, &edges);
    let focus = if let Some(value) = id_or_alias {
        let document = bundle
            .resolve(value)
            .ok_or_else(|| anyhow!("document '{value}' was not found"))?;
        Some(GraphFocus {
            id: document.id().to_string(),
            incoming: graph
                .incoming(document.id())
                .map(ToString::to_string)
                .collect(),
            outgoing: graph
                .outgoing(document.id())
                .map(ToString::to_string)
                .collect(),
            reachable: graph
                .reachable_from(document.id())
                .into_iter()
                .map(|id| id.to_string())
                .collect(),
        })
    } else {
        None
    };

    let view = GraphView {
        nodes,
        edges,
        roots: graph.roots().into_iter().map(ToString::to_string).collect(),
        leaves: graph
            .leaves()
            .into_iter()
            .map(ToString::to_string)
            .collect(),
        focus,
        dot: dot.clone(),
    };
    let human = match representation {
        GraphRepresentation::Dot => dot,
        GraphRepresentation::Summary => {
            let mut lines = vec![format!(
                "{} node(s), {} edge(s)",
                graph.node_count(),
                graph.edge_count()
            )];
            lines.extend(view.edges.iter().map(|edge| {
                format!(
                    "{} -> {} [{}]",
                    edge.from,
                    edge.to,
                    edge.relations.join(",")
                )
            }));
            lines.join("\n")
        }
    };
    Outcome::success(human, view)
}

fn normalized_tags(tags: &[String]) -> BTreeSet<String> {
    tags.iter().map(|tag| tag.trim().to_lowercase()).collect()
}

fn document_has_tags(document: &Document, required: &BTreeSet<String>) -> bool {
    let actual = document
        .metadata()
        .tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .collect::<BTreeSet<_>>();
    required.iter().all(|tag| actual.contains(tag))
}
