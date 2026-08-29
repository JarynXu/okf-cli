use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum GraphRepresentation {
    #[default]
    Summary,
    Dot,
}

#[derive(Debug, Parser)]
#[command(
    name = "okf",
    version,
    about = "Inspect, query, and compose Open Knowledge Format knowledge"
)]
pub(crate) struct Cli {
    /// Bundle directory used by core bundle commands.
    #[arg(long, global = true, default_value = ".")]
    pub(crate) bundle: PathBuf,

    /// Persistent Library registry used by `okf library` commands.
    #[arg(long, global = true, default_value = ".okf/libraries.json")]
    pub(crate) registry: PathBuf,

    /// Project Context profile state used by `okf project` commands.
    #[arg(long, global = true, default_value = ".okf/project-context.json")]
    pub(crate) project_context: PathBuf,

    /// Output format. JSON is recommended for agents and scripts.
    #[arg(long, global = true, value_enum, default_value = "human")]
    pub(crate) output: OutputFormat,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
        #[arg(long, default_value_t = 20, value_parser = parse_limit)]
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
    /// Install, mount, navigate, and query pluggable OKF Libraries.
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Bootstrap, recover, and checkpoint a repository-bound Project Context Library.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum LibraryCommand {
    /// Install/register a local directory or Git repository Library.
    Add {
        /// Local directory path or Git repository URL.
        source: String,
        /// Stable Library ID. Inferred from source when omitted.
        #[arg(long)]
        id: Option<String>,
        /// Human-readable Library name.
        #[arg(long)]
        name: Option<String>,
        /// Git branch, tag, or commit to check out.
        #[arg(long = "ref")]
        reference: Option<String>,
    },
    /// Update a Library source. Git sources fetch/pull; local sources are revalidated.
    Update { id: String },
    /// Uninstall/unregister a Library and remove managed Git cache data.
    Remove { id: String },
    /// Mount an installed Library into the active global knowledge space.
    Mount { id: String },
    /// Unmount a Library without uninstalling it.
    Unmount { id: String },
    /// List installed Libraries and mount state.
    List,
    /// Show semantic catalogs from one or all mounted Libraries.
    Catalog {
        /// Optional Library ID. Omit for the global catalog.
        id: Option<String>,
    },
    /// Read one canonical `okf://<library>/<path>` knowledge URI.
    Read { uri: String },
    /// Query one Library or every mounted query-capable Library.
    Query {
        query: String,
        /// Restrict query to one Library.
        #[arg(long)]
        library: Option<String>,
        /// Maximum number of evidence hits per Library.
        #[arg(long, default_value_t = 20, value_parser = parse_limit)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Create and mount a Project Context Library scaffold for a Git repository.
    Init {
        /// Git repository to bind to the context profile.
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        /// Human-readable project name. Inferred from the repository directory when omitted.
        #[arg(long)]
        project: Option<String>,
        /// Stable Library ID.
        #[arg(long, default_value = "project-context")]
        id: String,
        /// Rebuild an existing scaffold. Re-installation is intentionally not attempted.
        #[arg(long)]
        force: bool,
    },
    /// Evaluate recovery freshness against the current Git revision and compute impacted topics.
    Status,
    /// Advance the validated revision after required source/test verification has completed.
    Checkpoint {
        /// Explicit commit revision. Defaults to repository HEAD.
        #[arg(long)]
        revision: Option<String>,
    },
}

impl Command {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Init { .. } => "init",
            Self::Validate { .. } => "validate",
            Self::List { .. } => "list",
            Self::Get { .. } => "get",
            Self::Inspect { .. } => "inspect",
            Self::Search { .. } => "search",
            Self::Graph { .. } => "graph",
            Self::Library { .. } => "library",
            Self::Project { .. } => "project",
        }
    }
}

fn parse_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|error| format!("invalid limit: {error}"))?;
    if (1..=1000).contains(&limit) {
        Ok(limit)
    } else {
        Err("limit must be between 1 and 1000".to_owned())
    }
}
