//! `okf` command-line interface.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum, error::ErrorKind};
use okf::{
    Bundle, BundleParser, Document, DocumentId, KnowledgeGraph, SearchQuery, Severity,
    ValidationOptions, Validator,
};
use serde::Serialize;
use serde_json::{Value, json};

const JSON_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum GraphRepresentation {
    #[default]
    Summary,
    Dot,
}

#[derive(Debug, Parser)]
#[command(name = "okf", version, about = "Inspect and query Open Knowledge Format bundles")]
struct Cli {
    /// Bundle directory used by the command.
    #[arg(long, global = true, default_value = ".")]
    bundle: PathBuf,

    /// Output format. JSON is recommended for agents and scripts.
    #[arg(long, global = true, value_enum, default_value = "human")]
    output: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a minimal Markdown knowledge bundle.
    Init {
        /// Replace the generated index.md file when it exists.
        #[arg(long)]
        force: bool,
    },
    /// Parse and structurally validate a bundle.
    Validate {
        /// Downgrade unresolved references from errors to warnings.
        #[arg(long)]
        allow_unresolved: bool,
        /// Do not report disconnected documents.
        #[arg(long)]
        no_orphans: bool,
        /// Return exit code 1 when warnings exist.
        #[arg(long)]
        deny_warnings: bool,
    },
    /// List documents in stable identifier order.
    List {
        /// Required tag. Repeat to require every supplied tag.
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Read one document by canonical identifier or alias.
    Get { id: String },
    /// Inspect one document and its resolved graph neighborhood.
    Inspect { id: String },
    /// Search documents with deterministic lexical ranking.
    Search {
        query: String,
        /// Required tag. Repeat to require every supplied tag.
        #[arg(long)]
        tag: Vec<String>,
        /// Maximum number of returned hits.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(usize).range(1..=1000))]
        limit: usize,
    },
    /// Build the resolved directed knowledge graph.
    Graph {
        /// Optionally focus graph details on one identifier or alias.
        #[arg(long)]
        id: Option<String>,
        /// Human graph representation.
        #[arg(long, value_enum, default_value = "summary")]
        representation: GraphRepresentation,
    },
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Init { .. } => "init",
            Self::Validate { .. } => "validate",
            Self::List { .. } => "list",
            Self::Get { .. } => "get",
            Self::Inspect { .. } => "inspect",
            Self::Search { .. } => "search",
            Self::Graph { .. } => "graph",
        }
    }
}

#[derive(Debug)]
struct Outcome {
    human: String,
    data: Value,
    exit_code: u8,
}

impl Outcome {
    fn success(human: impl Into<String>, data: impl Serialize) -> Result<Self> {
        Ok(Self {
            human: human.into(),
            data: serde_json::to_value(data)?,
            exit_code: 0,
        })
    }

    fn with_exit_code(mut self, exit_code: u8) -> Self {
        self.exit_code = exit_code;
        self
    }
}

#[derive(Debug, Serialize)]
struct DocumentSummary {
    id: String,
    title: String,
    summary: Option<String>,
    tags: Vec<String>,
    source_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct DocumentView {
    id: String,
    title: String,
    body: String,
    metadata: okf::Metadata,
    source_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectionView {
    document: DocumentView,
    incoming: Vec<String>,
    outgoing: Vec<String>,
    unresolved: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SearchHitView {
    id: String,
    title: String,
    score: u32,
    matched_fields: Vec<String>,
    snippet: String,
}

#[derive(Debug, Serialize)]
struct EdgeView {
    from: String,
    to: String,
    relations: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GraphFocus {
    id: String,
    incoming: Vec<String>,
    outgoing: Vec<String>,
    reachable: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GraphView {
    nodes: Vec<String>,
    edges: Vec<EdgeView>,
    roots: Vec<String>,
    leaves: Vec<String>,
    focus: Option<GraphFocus>,
    dot: String,
}

#[derive(Debug, Serialize)]
struct InitView {
    root: String,
    created: Vec<String>,
}

fn main() -> ExitCode {
    let args = env::args_os().collect::<Vec<_>>();
    let json_requested = requests_json(&args);
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) if matches!(error.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            if error.print().is_err() {
                return ExitCode::from(3);
            }
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            if json_requested {
                let _ = write_json_error("cli", "usage", &error.to_string());
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };

    let command_name = cli.command.name();
    match execute(&cli.bundle, &cli.command) {
        Ok(outcome) => match write_outcome(cli.output, command_name, &outcome) {
            Ok(()) => ExitCode::from(outcome.exit_code),
            Err(error) => {
                eprintln!("error: failed to write output: {error:#}");
                ExitCode::from(3)
            }
        },
        Err(error) => {
            let write_result = match cli.output {
                OutputFormat::Human => {
                    eprintln!("error: {error:#}");
                    Ok(())
                }
                OutputFormat::Json => {
                    write_json_error(command_name, "operational", &format!("{error:#}"))
                }
            };
            if let Err(write_error) = write_result {
                eprintln!("error: {error:#}");
                eprintln!("error: failed to write structured error: {write_error:#}");
            }
            ExitCode::from(3)
        }
    }
}

fn execute(bundle_path: &Path, command: &Command) -> Result<Outcome> {
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
        Command::Search { query, tag, limit } => {
            search_documents(bundle_path, query, tag, *limit)
        }
        Command::Graph { id, representation } => {
            graph_bundle(bundle_path, id.as_deref(), *representation)
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
        bail!("{} already exists; pass --force to replace it", index.display());
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
    let focus = id_or_alias
        .map(|value| {
            let document = bundle
                .resolve(value)
                .ok_or_else(|| anyhow!("document '{value}' was not found"))?;
            Ok(GraphFocus {
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
        })
        .transpose()?;
    let view = GraphView {
        nodes,
        edges,
        roots: graph
            .roots()
            .into_iter()
            .map(ToString::to_string)
            .collect(),
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

fn document_summary(document: &Document) -> DocumentSummary {
    DocumentSummary {
        id: document.id().to_string(),
        title: document.title().to_owned(),
        summary: document.metadata().summary.clone(),
        tags: document.metadata().tags.iter().cloned().collect(),
        source_path: document.source_path().map(portable_path),
    }
}

fn document_view(document: &Document) -> DocumentView {
    DocumentView {
        id: document.id().to_string(),
        title: document.title().to_owned(),
        body: document.body().to_owned(),
        metadata: document.metadata().clone(),
        source_path: document.source_path().map(portable_path),
    }
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

fn graph_to_dot(bundle: &Bundle, edges: &[EdgeView]) -> String {
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

fn escape_dot(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn display_values(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_owned()
    } else {
        values.join(", ")
    }
}

fn write_outcome(format: OutputFormat, command: &str, outcome: &Outcome) -> Result<()> {
    let mut stdout = io::stdout().lock();
    match format {
        OutputFormat::Human => writeln!(stdout, "{}", outcome.human.trim_end())?,
        OutputFormat::Json => {
            serde_json::to_writer_pretty(
                &mut stdout,
                &json!({
                    "schema_version": JSON_SCHEMA_VERSION,
                    "ok": outcome.exit_code == 0,
                    "command": command,
                    "data": outcome.data,
                }),
            )?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

fn write_json_error(command: &str, kind: &str, message: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(
        &mut stdout,
        &json!({
            "schema_version": JSON_SCHEMA_VERSION,
            "ok": false,
            "command": command,
            "error": { "kind": kind, "message": message },
        }),
    )?;
    writeln!(stdout)?;
    Ok(())
}

fn requests_json(args: &[OsString]) -> bool {
    args.iter()
        .any(|argument| argument.to_str() == Some("--output=json"))
        || args.windows(2).any(|pair| {
            pair.first()
                .is_some_and(|argument| argument.to_str() == Some("--output"))
                && pair
                    .get(1)
                    .is_some_and(|argument| argument.to_str() == Some("json"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_values_are_escaped() {
        assert_eq!(escape_dot("a\\b\"c"), "a\\\\b\\\"c");
    }

    #[test]
    fn detects_json_argument_forms() {
        assert!(requests_json(&[
            OsString::from("okf"),
            OsString::from("--output"),
            OsString::from("json"),
        ]));
        assert!(requests_json(&[
            OsString::from("okf"),
            OsString::from("--output=json"),
        ]));
    }

    #[test]
    fn validates_document_ids_used_by_cli() {
        assert!(DocumentId::new("architecture/runtime").is_ok());
    }
}
