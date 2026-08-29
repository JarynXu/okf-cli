use std::path::Path;

use anyhow::Result;

use crate::cli::LibraryCommand;
use crate::libraries;
use crate::output::Outcome;

pub(crate) fn execute(registry_path: &Path, command: &LibraryCommand) -> Result<Outcome> {
    match command {
        LibraryCommand::Add {
            source,
            id,
            name,
            reference,
        } => libraries::add_library(
            registry_path,
            source,
            id.as_deref(),
            name.as_deref(),
            reference.as_deref(),
        ),
        LibraryCommand::Update { id } => libraries::update_library(registry_path, id),
        LibraryCommand::Remove { id } => libraries::remove_library(registry_path, id),
        LibraryCommand::Mount { id } => libraries::set_mounted(registry_path, id, true),
        LibraryCommand::Unmount { id } => libraries::set_mounted(registry_path, id, false),
        LibraryCommand::List => libraries::list_libraries(registry_path),
    }
}
