use std::ffi::OsString;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, error::ErrorKind};
use serde_json::json;

use crate::cli::{Cli, Command, OutputFormat};
use crate::commands;
use crate::libraries;
use crate::library_dispatch;
use crate::output::{Outcome, write_json_error, write_outcome};

pub(crate) fn run(args: Vec<OsString>) -> ExitCode {
    let json_requested = requests_json(&args);
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
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
    let execution = match &cli.command {
        Command::Library { command } => library_dispatch::execute(&cli.registry, command),
        Command::Search {
            query,
            tag,
            library,
            limit,
        } => search_active_knowledge(
            &cli.bundle,
            &cli.registry,
            query,
            tag,
            library.as_deref(),
            *limit,
        ),
        Command::Get { id } if id.starts_with("okf://") => libraries::read(&cli.registry, id),
        command => commands::execute(&cli.bundle, command),
    };
    match execution {
        Ok(outcome) => match write_outcome(cli.output, command_name, &outcome) {
            Ok(()) => ExitCode::from(outcome.exit_code),
            Err(error) => {
                eprintln!("error: failed to write output: {error:#}");
                ExitCode::from(3)
            }
        },
        Err(error) => {
            let result = match cli.output {
                OutputFormat::Human => {
                    eprintln!("error: {error:#}");
                    Ok(())
                }
                OutputFormat::Json => {
                    write_json_error(command_name, "operational", &format!("{error:#}"))
                }
            };
            if let Err(write_error) = result {
                eprintln!("error: {error:#}");
                eprintln!("error: failed to write structured error: {write_error:#}");
            }
            ExitCode::from(3)
        }
    }
}

fn search_active_knowledge(
    bundle: &std::path::Path,
    registry: &std::path::Path,
    query: &str,
    tags: &[String],
    library: Option<&str>,
    limit: usize,
) -> Result<Outcome> {
    if let Some(library) = library {
        return libraries::query(registry, Some(library), query, limit);
    }

    let bundle_command = Command::Search {
        query: query.to_owned(),
        tag: tags.to_vec(),
        library: None,
        limit,
    };
    let bundle_result = commands::execute(bundle, &bundle_command);
    let library_result = libraries::query(registry, None, query, limit);

    match (bundle_result, library_result) {
        (Ok(bundle), Ok(libraries)) => {
            let mut sections = Vec::new();
            if bundle.human != "no matching documents" {
                sections.push(bundle.human.clone());
            }
            if libraries.human != "no matching knowledge" {
                sections.push(libraries.human.clone());
            }
            Outcome::success(
                if sections.is_empty() {
                    "no matching knowledge".to_owned()
                } else {
                    sections.join("\n")
                },
                json!({
                    "bundle": bundle.data,
                    "libraries": libraries.data,
                }),
            )
        }
        (Ok(bundle), Err(_)) => Ok(bundle),
        (Err(bundle_error), Ok(libraries)) => {
            let has_libraries = libraries
                .data
                .as_array()
                .is_some_and(|values| !values.is_empty());
            if has_libraries {
                Ok(libraries)
            } else {
                Err(bundle_error)
            }
        }
        (Err(bundle_error), Err(_)) => Err(bundle_error),
    }
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
}
