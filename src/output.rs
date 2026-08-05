use std::io::{self, Write};

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::OutputFormat;

const JSON_SCHEMA_VERSION: &str = "1";

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) human: String,
    pub(crate) data: Value,
    pub(crate) exit_code: u8,
}

impl Outcome {
    pub(crate) fn success(human: impl Into<String>, data: impl Serialize) -> Result<Self> {
        Ok(Self {
            human: human.into(),
            data: serde_json::to_value(data)?,
            exit_code: 0,
        })
    }

    pub(crate) fn with_exit_code(mut self, exit_code: u8) -> Self {
        self.exit_code = exit_code;
        self
    }
}

pub(crate) fn write_outcome(format: OutputFormat, command: &str, outcome: &Outcome) -> Result<()> {
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

pub(crate) fn write_json_error(command: &str, kind: &str, message: &str) -> Result<()> {
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
