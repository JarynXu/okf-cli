//! `okf` command-line interface.

#![forbid(unsafe_code)]

mod app;
mod cli;
mod commands;
#[path = "libraries_v2.rs"]
mod libraries;
mod library_dispatch;
mod output;
mod view;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    app::run(env::args_os().collect())
}
