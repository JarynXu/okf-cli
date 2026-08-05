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
    about = "Inspect and query Open Knowledge Format bundles"
)]
pub(crate) struct Cli {
    /// Bundle directory used by the command.
    #[arg(long, global = true, default_value = ".")]
    pub(crate) bundle: PathBuf,

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
