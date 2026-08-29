//! `okf` command-line interface.

#![forbid(unsafe_code)]

mod app;
mod cli;
mod commands;
mod libraries;
mod output;
mod view;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    app::run(env::args_os().collect())
}
