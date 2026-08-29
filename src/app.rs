use std::ffi::OsString;
use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};

use crate::cli::{Cli, OutputFormat};
use crate::commands::execute;
use crate::output::{write_json_error, write_outcome};

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
    match execute(&cli.bundle, &cli.registry, &cli.command) {
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
