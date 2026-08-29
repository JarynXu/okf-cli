use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use okf::{
    BundleLibraryProvider, BundleParser, KnowledgeUri, LibraryId, LibraryInstance,
    LibraryManifest, LibraryQuery, LibraryRegistry, LibrarySource,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::output::Outcome;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RegistryEntry {
    manifest: LibraryManifest,
    mounted: bool,
    materialized: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct RegistryFile {
    libraries: BTreeMap<String, RegistryEntry>,
}

pub(crate) fn add_library(
    registry_path: &Path,
    source: &str,
    id: Option<&str>,
    name: Option<&str>,
    reference: Option<&str>,
) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let inferred = id.map(str::to_owned).unwrap_or_else(|| infer_id(source));
    let library_id = LibraryId::parse(inferred.clone())
        .map_err(|error| anyhow!(error.to_string()))?;
    if registry.libraries.contains_key(library_id.as_str()) {
        bail!("library '{}' is already installed", library_id);
    }

    let display_name = name.unwrap_or(library_id.as_str()).to_owned();
    let (library_source, materialized) = if is_git_source(source) {
        let cache = cache_path(registry_path, &library_id);
        materialize_git(source, reference, &cache)?;
        (
            LibrarySource::Git {
                repository: source.to_owned(),
                reference: reference.map(str::to_owned),
            },
            Some(cache),
        )
    } else {
        let path = PathBuf::from(source);
        if !path.is_dir() {
            bail!("local library source '{}' is not a directory", path.display());
        }
        (
            LibrarySource::Local { path: path.clone() },
            Some(path),
        )
    };

    let mut manifest = LibraryManifest::new(library_id.clone(), display_name);
    manifest.source = Some(library_source);
    registry.libraries.insert(
        library_id.to_string(),
        RegistryEntry {
            manifest: manifest.clone(),
            mounted: false,
            materialized,
        },
    );
    save_registry(registry_path, &registry)?;

    Outcome::success(
        format!("installed {}", library_id),
        json!({"library": manifest, "mounted": false}),
    )
}

pub(crate) fn remove_library(registry_path: &Path, id: &str) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let entry = registry
        .libraries
        .remove(id)
        .ok_or_else(|| anyhow!("library '{id}' is not installed"))?;

    if matches!(entry.manifest.source, Some(LibrarySource::Git { .. })) {
        if let Some(path) = &entry.materialized {
            if path.exists() {
                fs::remove_dir_all(path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
        }
    }
    save_registry(registry_path, &registry)?;
    Outcome::success(format!("uninstalled {id}"), json!({"id": id}))
}

pub(crate) fn update_library(registry_path: &Path, id: &str) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let entry = registry
        .libraries
        .get_mut(id)
        .ok_or_else(|| anyhow!("library '{id}' is not installed"))?;

    match &entry.manifest.source {
        Some(LibrarySource::Git {
            repository,
            reference,
        }) => {
            let path = entry
                .materialized
                .as_ref()
                .ok_or_else(|| anyhow!("git library '{id}' has no materialized path"))?;
            if path.exists() {
                run_git(path, &["fetch", "--all", "--tags"])?;
                let target = reference.as_deref().unwrap_or("HEAD");
                run_git(path, &["checkout", target])?;
                if reference.is_none() {
                    let _ = run_git(path, &["pull", "--ff-only"]);
                }
            } else {
                materialize_git(repository, reference.as_deref(), path)?;
            }
        }
        Some(LibrarySource::Local { path }) => {
            if !path.is_dir() {
                bail!("local library source '{}' is no longer available", path.display());
            }
        }
        Some(LibrarySource::Custom { .. }) => {
            bail!("custom source update requires a source adapter")
        }
        None => bail!("library '{id}' does not declare a source"),
    }

    save_registry(registry_path, &registry)?;
    Outcome::success(format!("updated {id}"), json!({"id": id}))
}

pub(crate) fn set_mounted(registry_path: &Path, id: &str, mounted: bool) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let entry = registry
        .libraries
        .get_mut(id)
        .ok_or_else(|| anyhow!("library '{id}' is not installed"))?;
    if mounted {
        // Validate that the currently materialized content can be opened before publishing it.
        let _ = resolve_instance(entry)?;
    }
    entry.mounted = mounted;
    save_registry(registry_path, &registry)?;
    Outcome::success(
        format!("{} {id}", if mounted { "mounted" } else { "unmounted" }),
        json!({"id": id, "mounted": mounted}),
    )
}

pub(crate) fn list_libraries(registry_path: &Path) -> Result<Outcome> {
    let registry = load_registry(registry_path)?;
    let values = registry
        .libraries
        .values()
        .map(|entry| {
            json!({
                "manifest": entry.manifest,
                "mounted": entry.mounted,
                "materialized": entry.materialized,
            })
        })
        .collect::<Vec<_>>();
    let human = if registry.libraries.is_empty() {
        "no installed libraries".to_owned()
    } else {
        registry
            .libraries
            .iter()
            .map(|(id, entry)| {
                format!(
                    "{}\t{}\t{}",
                    if entry.mounted { "mounted" } else { "unmounted" },
                    id,
                    entry.manifest.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::success(human, values)
}

pub(crate) fn catalog(registry_path: &Path, id: Option<&str>) -> Result<Outcome> {
    let runtime = build_runtime(registry_path)?;
    let catalogs = if let Some(id) = id {
        let library_id = LibraryId::parse(id.to_owned()).map_err(|error| anyhow!(error.to_string()))?;
        vec![runtime.catalog(&library_id).map_err(|error| anyhow!(error.to_string()))?]
    } else {
        runtime
            .global_catalog()
            .map_err(|error| anyhow!(error.to_string()))?
    };
    let human = if catalogs.is_empty() {
        "no mounted library catalogs".to_owned()
    } else {
        catalogs
            .iter()
            .flat_map(|catalog| {
                catalog.entries.iter().map(move |entry| {
                    format!("{}\t{}\t{}", catalog.library, entry.id, entry.title)
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::success(human, catalogs)
}

pub(crate) fn read(registry_path: &Path, uri: &str) -> Result<Outcome> {
    let runtime = build_runtime(registry_path)?;
    let uri = KnowledgeUri::parse(uri).map_err(|error| anyhow!(error.to_string()))?;
    let content = runtime.read(&uri).map_err(|error| anyhow!(error.to_string()))?;
    Outcome::success(content.clone(), json!({"uri": uri, "content": content}))
}

pub(crate) fn query(
    registry_path: &Path,
    library: Option<&str>,
    text: &str,
    limit: usize,
) -> Result<Outcome> {
    let runtime = build_runtime(registry_path)?;
    let request = LibraryQuery::new(text).limit(limit);
    if let Some(library) = library {
        let id = LibraryId::parse(library.to_owned()).map_err(|error| anyhow!(error.to_string()))?;
        let result = runtime
            .query(&id, &request)
            .map_err(|error| anyhow!(error.to_string()))?;
        let human = format_query_hits(&id, &result.hits);
        Outcome::success(human, json!({"library": id, "result": result}))
    } else {
        let results = runtime.query_all(&request);
        let mut human = Vec::new();
        let mut json_results = Vec::new();
        for (id, result) in results {
            match result {
                Ok(result) => {
                    human.push(format_query_hits(&id, &result.hits));
                    json_results.push(json!({"library": id, "result": result}));
                }
                Err(error) => json_results.push(json!({"library": id, "error": error.to_string()})),
            }
        }
        Outcome::success(
            if human.is_empty() {
                "no matching knowledge".to_owned()
            } else {
                human.into_iter().filter(|value| !value.is_empty()).collect::<Vec<_>>().join("\n")
            },
            json_results,
        )
    }
}

fn format_query_hits(id: &LibraryId, hits: &[okf::LibraryQueryHit]) -> String {
    if hits.is_empty() {
        return format!("{id}: no matching knowledge");
    }
    hits.iter()
        .map(|hit| {
            format!(
                "{}\t{}\n  {}",
                id,
                hit.uri,
                hit.snippet.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_runtime(registry_path: &Path) -> Result<LibraryRegistry> {
    let registry_file = load_registry(registry_path)?;
    let mut runtime = LibraryRegistry::new();
    for entry in registry_file.libraries.values().filter(|entry| entry.mounted) {
        let instance = resolve_instance(entry)?;
        let id = instance.manifest().id.clone();
        runtime.register(instance).map_err(|error| anyhow!(error.to_string()))?;
        runtime.mount(&id).map_err(|error| anyhow!(error.to_string()))?;
    }
    Ok(runtime)
}

fn resolve_instance(entry: &RegistryEntry) -> Result<LibraryInstance> {
    let path = match &entry.manifest.source {
        Some(LibrarySource::Local { path }) => path,
        Some(LibrarySource::Git { .. }) => entry
            .materialized
            .as_ref()
            .ok_or_else(|| anyhow!("git library '{}' is not materialized", entry.manifest.id))?,
        Some(LibrarySource::Custom { kind, .. }) => {
            bail!("custom library source '{kind}' requires an adapter")
        }
        None => bail!("library '{}' does not declare a source", entry.manifest.id),
    };
    let bundle = BundleParser::default()
        .parse_dir(path)
        .with_context(|| format!("failed to load Library '{}' from {}", entry.manifest.id, path.display()))?;
    Ok(LibraryInstance::new(
        entry.manifest.clone(),
        Arc::new(BundleLibraryProvider::new(bundle)),
    ))
}

fn load_registry(path: &Path) -> Result<RegistryFile> {
    if !path.exists() {
        return Ok(RegistryFile::default());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_registry(path: &Path, registry: &RegistryFile) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(registry)?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn cache_path(registry_path: &Path, id: &LibraryId) -> PathBuf {
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cache")
        .join(id.as_str())
}

fn materialize_git(source: &str, reference: Option<&str>, path: &Path) -> Result<()> {
    if path.exists() {
        bail!("git cache path '{}' already exists", path.display());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let status = ProcessCommand::new("git")
        .arg("clone")
        .arg(source)
        .arg(path)
        .status()
        .context("failed to execute git clone")?;
    if !status.success() {
        bail!("git clone failed for '{source}'");
    }
    if let Some(reference) = reference {
        run_git(path, &["checkout", reference])?;
    }
    Ok(())
}

fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    let status = ProcessCommand::new("git")
        .current_dir(path)
        .args(args)
        .status()
        .with_context(|| format!("failed to execute git {}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        bail!("git {} failed in {}", args.join(" "), path.display())
    }
}

fn is_git_source(source: &str) -> bool {
    source.starts_with("git@")
        || source.starts_with("ssh://")
        || source.starts_with("http://")
        || source.starts_with("https://")
        || source.ends_with(".git")
}

fn infer_id(source: &str) -> String {
    let trimmed = source.trim_end_matches('/').trim_end_matches(".git");
    let raw = trimmed.rsplit(['/', ':']).next().unwrap_or("library");
    let normalized = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        "library".to_owned()
    } else {
        normalized
    }
}
