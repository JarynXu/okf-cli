use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use okf::{
    BundleLibraryProvider, BundleParser, HttpLibraryProvider, KnowledgeUri, LibraryCapability,
    LibraryId, LibraryInstance, LibraryManifest, LibraryPackageManifest,
    LibraryProviderDeclaration, LibraryQuery, LibraryRegistry, LibrarySource,
    ProcessLibraryProvider, ProviderStack,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::output::Outcome;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RegistryEntry {
    manifest: LibraryManifest,
    mounted: bool,
    materialized: Option<PathBuf>,
    #[serde(default)]
    package: Option<LibraryPackageManifest>,
    #[serde(default)]
    approved_provider_kinds: BTreeSet<String>,
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
    let provisional_id =
        LibraryId::parse(id.map(str::to_owned).unwrap_or_else(|| infer_id(source)))
            .map_err(|error| anyhow!(error.to_string()))?;

    let (library_source, materialized) = if is_git_source(source) {
        let cache = cache_path(registry_path, &provisional_id);
        materialize_git(source, reference, &cache)?;
        (
            LibrarySource::Git {
                repository: source.to_owned(),
                reference: reference.map(str::to_owned),
            },
            cache,
        )
    } else {
        let path = PathBuf::from(source);
        if !path.is_dir() {
            bail!("local Library source '{}' is not a directory", path.display());
        }
        let path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", path.display()))?;
        (LibrarySource::Local { path: path.clone() }, path)
    };

    let package = load_optional_package(&materialized)?;
    let mut manifest = if let Some(package) = &package {
        let package_manifest = package
            .runtime_manifest(Some(library_source.clone()))
            .map_err(|error| anyhow!(error.to_string()))?;
        if let Some(requested_id) = id {
            if requested_id != package_manifest.id.as_str() {
                cleanup_failed_git_install(&library_source, &materialized)?;
                bail!(
                    "requested Library id '{}' does not match okf-library.yaml id '{}'",
                    requested_id,
                    package_manifest.id
                );
            }
        }
        package_manifest
    } else {
        let display_name = name.unwrap_or(provisional_id.as_str()).to_owned();
        let mut manifest = LibraryManifest::new(provisional_id, display_name);
        manifest.source = Some(library_source.clone());
        manifest
    };

    if let Some(name) = name {
        manifest.name = name.to_owned();
    }
    if registry.libraries.contains_key(manifest.id.as_str()) {
        cleanup_failed_git_install(&library_source, &materialized)?;
        bail!("Library '{}' is already installed", manifest.id);
    }

    let library_id = manifest.id.clone();
    registry.libraries.insert(
        library_id.to_string(),
        RegistryEntry {
            manifest: manifest.clone(),
            mounted: false,
            materialized: Some(materialized),
            package,
            approved_provider_kinds: BTreeSet::new(),
        },
    );
    save_registry(registry_path, &registry)?;

    Outcome::success(
        format!("installed {library_id}"),
        json!({
            "library": manifest,
            "mounted": false,
            "provider_authorizations": [],
        }),
    )
}

pub(crate) fn remove_library(registry_path: &Path, id: &str) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let entry = registry
        .libraries
        .remove(id)
        .ok_or_else(|| anyhow!("Library '{id}' is not installed"))?;

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
        .ok_or_else(|| anyhow!("Library '{id}' is not installed"))?;

    let path = refresh_materialized_source(entry, id)?;
    let package = load_optional_package(&path)?;
    if let Some(package) = &package {
        if package.id != id {
            bail!(
                "updated okf-library.yaml changed Library id from '{}' to '{}'; reinstall explicitly",
                id,
                package.id
            );
        }
        entry.manifest.name = package.name.clone();
        entry.manifest.version = package.version.clone();
    }
    entry.package = package;

    if entry.mounted {
        let _ = resolve_instance(entry)?;
    }
    save_registry(registry_path, &registry)?;
    Outcome::success(format!("updated {id}"), json!({"id": id}))
}

pub(crate) fn set_mounted(
    registry_path: &Path,
    id: &str,
    mounted: bool,
    allow_provider: &[String],
) -> Result<Outcome> {
    let mut registry = load_registry(registry_path)?;
    let entry = registry
        .libraries
        .get_mut(id)
        .ok_or_else(|| anyhow!("Library '{id}' is not installed"))?;

    if mounted {
        authorize_provider_kinds(entry, allow_provider)?;
        let _ = resolve_instance(entry)?;
    }
    entry.mounted = mounted;
    save_registry(registry_path, &registry)?;

    Outcome::success(
        format!("{} {id}", if mounted { "mounted" } else { "unmounted" }),
        json!({
            "id": id,
            "mounted": mounted,
            "provider_authorizations": entry.approved_provider_kinds,
        }),
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
                "query": entry.package.as_ref().map(|package| &package.query),
                "declared_providers": entry.package.as_ref().map(|package| &package.providers),
                "provider_authorizations": entry.approved_provider_kinds,
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
                let approvals = if entry.approved_provider_kinds.is_empty() {
                    "-".to_owned()
                } else {
                    entry
                        .approved_provider_kinds
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",")
                };
                format!(
                    "{}\t{}\t{}\tproviders={approvals}",
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

pub(crate) fn read(registry_path: &Path, uri: &str) -> Result<Outcome> {
    let runtime = build_runtime(registry_path)?;
    let uri = KnowledgeUri::parse(uri).map_err(|error| anyhow!(error.to_string()))?;
    let content = runtime
        .read(&uri)
        .map_err(|error| anyhow!(error.to_string()))?;
    Outcome::success(content.clone(), json!({"uri": uri.to_string(), "content": content}))
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
                Err(error) => {
                    json_results.push(json!({"library": id, "error": error.to_string()}));
                }
            }
        }
        Outcome::success(
            if human.iter().all(String::is_empty) {
                "no matching knowledge".to_owned()
            } else {
                human
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            json_results,
        )
    }
}

fn format_query_hits(id: &LibraryId, hits: &[okf::LibraryQueryHit]) -> String {
    if hits.is_empty() {
        return String::new();
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
    for entry in registry_file
        .libraries
        .values()
        .filter(|entry| entry.mounted)
    {
        let instance = resolve_instance(entry)?;
        let id = instance.manifest().id.clone();
        runtime
            .register(instance)
            .map_err(|error| anyhow!(error.to_string()))?;
        runtime
            .mount(&id)
            .map_err(|error| anyhow!(error.to_string()))?;
    }
    Ok(runtime)
}

fn resolve_instance(entry: &RegistryEntry) -> Result<LibraryInstance> {
    let root = materialized_path(entry)?;
    let mut stack = ProviderStack::new(format!("library:{}", entry.manifest.id));

    if let Some(package) = &entry.package {
        for declaration in &package.providers {
            if !entry.approved_provider_kinds.contains(&declaration.kind) {
                continue;
            }
            stack.push(resolve_provider(
                declaration,
                &entry.manifest.id,
                root,
            )?);
        }
    }

    // A materialized OKF bundle is the compatibility fallback and always remains available after
    // provider-specific adapters. Invalid Markdown is never silently ignored.
    let bundle = BundleParser::default().parse_dir(root).with_context(|| {
        format!(
            "failed to load Library '{}' from {}",
            entry.manifest.id,
            root.display()
        )
    })?;
    let mut bundle_provider = BundleLibraryProvider::new(bundle);
    if let Some(package) = &entry.package {
        if !package.catalog.is_empty() {
            bundle_provider = bundle_provider.with_catalog(
                package
                    .runtime_catalog()
                    .map_err(|error| anyhow!(error.to_string()))?,
            );
        }
    }
    stack.push(Arc::new(bundle_provider));

    Ok(LibraryInstance::new(
        entry.manifest.clone(),
        Arc::new(stack),
    ))
}

fn resolve_provider(
    declaration: &LibraryProviderDeclaration,
    library: &LibraryId,
    root: &Path,
) -> Result<Arc<dyn okf::LibraryProvider>> {
    let capabilities = declaration
        .capabilities
        .iter()
        .map(|value| parse_capability(value))
        .collect::<Result<Vec<_>>>()?;

    match declaration.kind.as_str() {
        "process" => {
            let command = required_config_string(declaration, "command")?;
            let command = resolve_process_command(root, &command);
            let args = config_string_array(declaration, "args")?
                .into_iter()
                .map(|value| substitute_config(&value, root, library))
                .collect::<Vec<_>>();
            let cwd = optional_config_string(declaration, "cwd")?
                .map(|value| resolve_root_path(root, &substitute_config(&value, root, library)))
                .unwrap_or_else(|| root.to_path_buf());
            let mut provider = ProcessLibraryProvider::new(
                declaration.id.clone(),
                library.clone(),
                command,
                capabilities,
            )
            .args(args)
            .cwd(cwd);
            if let Some(timeout_ms) = optional_config_u64(declaration, "timeout_ms")? {
                provider = provider.timeout(Duration::from_millis(timeout_ms.clamp(1, 300_000)));
            }
            let inherited = config_string_array(declaration, "inherit_env")?;
            if !inherited.is_empty() {
                provider = provider.inherit_environment(inherited);
            }
            Ok(Arc::new(provider))
        }
        "http" => {
            let base_url = required_config_string(declaration, "base_url")?;
            let mut provider = HttpLibraryProvider::new(
                declaration.id.clone(),
                library.clone(),
                base_url,
                capabilities,
            )
            .map_err(|error| anyhow!(error.to_string()))?;
            if let Some(token_env) = optional_config_string(declaration, "token_env")? {
                let token = std::env::var(&token_env).with_context(|| {
                    format!(
                        "HTTP provider '{}' requires environment variable '{}'",
                        declaration.id, token_env
                    )
                })?;
                provider = provider.bearer_token(token);
            }
            Ok(Arc::new(provider))
        }
        other => bail!(
            "provider '{}' uses authorized kind '{}' but this CLI has no direct deployment adapter for it; expose it through an okf-provider/1 process or HTTP bridge, or use the SDK adapter directly",
            declaration.id,
            other
        ),
    }
}

fn authorize_provider_kinds(entry: &mut RegistryEntry, allowed: &[String]) -> Result<()> {
    let declared = entry
        .package
        .as_ref()
        .map(|package| {
            package
                .providers
                .iter()
                .map(|provider| provider.kind.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for kind in allowed {
        if !declared.contains(kind.as_str()) {
            bail!(
                "Library '{}' does not declare provider kind '{}'",
                entry.manifest.id,
                kind
            );
        }
        entry.approved_provider_kinds.insert(kind.clone());
    }
    Ok(())
}

fn parse_capability(value: &str) -> Result<LibraryCapability> {
    match value {
        "list" => Ok(LibraryCapability::List),
        "read" => Ok(LibraryCapability::Read),
        "catalog" => Ok(LibraryCapability::Catalog),
        "query" => Ok(LibraryCapability::Query),
        "refresh" => Ok(LibraryCapability::Refresh),
        "maintain" => Ok(LibraryCapability::Maintain),
        other => bail!("unknown Library provider capability '{other}'"),
    }
}

fn required_config_string(
    declaration: &LibraryProviderDeclaration,
    key: &str,
) -> Result<String> {
    optional_config_string(declaration, key)?.ok_or_else(|| {
        anyhow!(
            "provider '{}' requires string config '{}'",
            declaration.id,
            key
        )
    })
}

fn optional_config_string(
    declaration: &LibraryProviderDeclaration,
    key: &str,
) -> Result<Option<String>> {
    match declaration.config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!(
            "provider '{}' config '{}' must be a string",
            declaration.id,
            key
        ),
    }
}

fn optional_config_u64(
    declaration: &LibraryProviderDeclaration,
    key: &str,
) -> Result<Option<u64>> {
    match declaration.config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value.as_u64().map(Some).ok_or_else(|| {
            anyhow!(
                "provider '{}' config '{}' must be a non-negative integer",
                declaration.id,
                key
            )
        }),
        Some(_) => bail!(
            "provider '{}' config '{}' must be a non-negative integer",
            declaration.id,
            key
        ),
    }
}

fn config_string_array(
    declaration: &LibraryProviderDeclaration,
    key: &str,
) -> Result<Vec<String>> {
    match declaration.config.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    anyhow!(
                        "provider '{}' config '{}' must contain only strings",
                        declaration.id,
                        key
                    )
                })
            })
            .collect(),
        Some(_) => bail!(
            "provider '{}' config '{}' must be an array of strings",
            declaration.id,
            key
        ),
    }
}

fn substitute_config(value: &str, root: &Path, library: &LibraryId) -> String {
    value
        .replace("${library_root}", &root.to_string_lossy())
        .replace("${library_id}", library.as_str())
}

fn resolve_process_command(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute()
        || (!value.starts_with('.') && !value.contains('/') && !value.contains('\\'))
    {
        path
    } else {
        root.join(path)
    }
}

fn resolve_root_path(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn refresh_materialized_source(entry: &RegistryEntry, id: &str) -> Result<PathBuf> {
    match &entry.manifest.source {
        Some(LibrarySource::Git {
            repository,
            reference,
        }) => {
            let path = entry
                .materialized
                .as_ref()
                .ok_or_else(|| anyhow!("Git Library '{id}' has no materialized path"))?;
            if path.exists() {
                run_git(path, &["fetch", "--all", "--tags"])?;
                if let Some(reference) = reference {
                    run_git(path, &["checkout", reference])?;
                } else {
                    let _ = run_git(path, &["pull", "--ff-only"]);
                }
            } else {
                materialize_git(repository, reference.as_deref(), path)?;
            }
            Ok(path.clone())
        }
        Some(LibrarySource::Local { path }) => {
            if !path.is_dir() {
                bail!("local Library source '{}' is no longer available", path.display());
            }
            Ok(path.clone())
        }
        Some(LibrarySource::Custom { .. }) => {
            bail!("custom source acquisition requires a registered source adapter")
        }
        None => bail!("Library '{id}' does not declare a source"),
    }
}

fn materialized_path(entry: &RegistryEntry) -> Result<&Path> {
    match &entry.manifest.source {
        Some(LibrarySource::Local { path }) => Ok(path),
        Some(LibrarySource::Git { .. }) => entry
            .materialized
            .as_deref()
            .ok_or_else(|| anyhow!("Git Library '{}' is not materialized", entry.manifest.id)),
        Some(LibrarySource::Custom { kind, .. }) => {
            bail!("custom Library source '{kind}' requires a source adapter")
        }
        None => bail!("Library '{}' does not declare a source", entry.manifest.id),
    }
}

fn load_optional_package(path: &Path) -> Result<Option<LibraryPackageManifest>> {
    if LibraryPackageManifest::exists(path) {
        LibraryPackageManifest::load(path)
            .map(Some)
            .map_err(|error| anyhow!(error.to_string()))
    } else {
        Ok(None)
    }
}

fn load_registry(path: &Path) -> Result<RegistryFile> {
    if !path.exists() {
        return Ok(RegistryFile::default());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_registry(path: &Path, registry: &RegistryFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(registry)?;
    bytes.push(b'\n');
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).with_context(|| format!("failed to write {}", temp.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to replace {}", path.display()))?;
    }
    fs::rename(&temp, path).with_context(|| format!("failed to commit {}", path.display()))
}

fn infer_id(source: &str) -> String {
    let trimmed = source.trim_end_matches('/').trim_end_matches(".git");
    let name = trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("library");
    let normalized = name
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

fn is_git_source(source: &str) -> bool {
    source.starts_with("git@")
        || source.starts_with("ssh://")
        || source.starts_with("git://")
        || source.ends_with(".git")
        || (source.starts_with("http://") || source.starts_with("https://"))
            && source.contains("git")
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
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to reset Git cache {}", path.display()))?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let status = ProcessCommand::new("git")
        .args(["clone", "--", source])
        .arg(path)
        .status()
        .context("failed to execute git clone")?;
    if !status.success() {
        bail!("git clone failed with {status}");
    }
    if let Some(reference) = reference {
        run_git(path, &["checkout", reference])?;
    }
    Ok(())
}

fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute git {}", args.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn cleanup_failed_git_install(source: &LibrarySource, materialized: &Path) -> Result<()> {
    if matches!(source, LibrarySource::Git { .. }) && materialized.exists() {
        fs::remove_dir_all(materialized).with_context(|| {
            format!("failed to clean Git cache {}", materialized.display())
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_process_commands_resolve_through_path() {
        let root = Path::new("/tmp/library");
        assert_eq!(resolve_process_command(root, "project-context"), PathBuf::from("project-context"));
        assert_eq!(resolve_process_command(root, "./bin/provider"), root.join("./bin/provider"));
    }

    #[test]
    fn substitutions_are_runtime_local() {
        let root = Path::new("/tmp/library");
        let id = LibraryId::parse("demo").expect("id");
        let value = substitute_config("${library_root}:${library_id}", root, &id);
        assert!(value.ends_with(":demo"));
        assert!(value.starts_with(&root.to_string_lossy().to_string()));
    }
}
