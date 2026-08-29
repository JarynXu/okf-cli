use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use okf::{
    BundleLibraryProvider, BundleParser, KnowledgeNode, KnowledgeUri, LibraryCapability,
    LibraryCatalog, LibraryId, LibraryInstance, LibraryManifest, LibraryPackageManifest,
    LibraryProvider, LibraryQuery, LibraryQueryResult, LibraryRegistry, LibraryResult,
    LibrarySource,
};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wait_timeout::ChildExt;

use crate::output::Outcome;

const PROVIDER_PROTOCOL: &str = "okf-provider/1";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RegistryEntry {
    manifest: LibraryManifest,
    mounted: bool,
    materialized: Option<PathBuf>,
    #[serde(default)]
    package: Option<LibraryPackageManifest>,
    #[serde(default)]
    providers: Vec<ProviderDeclaration>,
    #[serde(default)]
    approved_provider_kinds: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct RegistryFile {
    libraries: BTreeMap<String, RegistryEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct DeploymentManifest {
    #[serde(default)]
    providers: Vec<ProviderDeclaration>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProviderDeclaration {
    id: String,
    kind: String,
    #[serde(default)]
    capabilities: BTreeSet<String>,
    #[serde(default)]
    config: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
struct ManifestBundleProvider {
    inner: BundleLibraryProvider,
    catalog: Option<LibraryCatalog>,
}

impl ManifestBundleProvider {
    fn new(inner: BundleLibraryProvider, catalog: Option<LibraryCatalog>) -> Self {
        Self { inner, catalog }
    }
}

impl LibraryProvider for ManifestBundleProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.inner.capabilities()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        if let Some(catalog) = &self.catalog {
            if &catalog.library != library {
                return Err(okf::LibraryError::Provider(format!(
                    "package catalog belongs to '{}' but Library is mounted as '{}'",
                    catalog.library, library
                )));
            }
            Ok(catalog.clone())
        } else {
            self.inner.catalog(library)
        }
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        self.inner.list(library, path)
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.inner.read(uri)
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        self.inner.query(library, query)
    }

    fn refresh(&self) -> LibraryResult<()> {
        self.inner.refresh()
    }
}

#[derive(Default)]
struct ProviderStack {
    id: String,
    providers: Vec<Arc<dyn LibraryProvider>>,
}

impl std::fmt::Debug for ProviderStack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStack")
            .field("id", &self.id)
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|provider| provider.provider_id())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ProviderStack {
    fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            providers: Vec::new(),
        }
    }

    fn push(&mut self, provider: Arc<dyn LibraryProvider>) {
        self.providers.push(provider);
    }

    fn provider_for(&self, capability: LibraryCapability) -> LibraryResult<&dyn LibraryProvider> {
        self.providers
            .iter()
            .find(|provider| provider.capabilities().contains(&capability))
            .map(AsRef::as_ref)
            .ok_or(okf::LibraryError::UnsupportedCapability(capability))
    }
}

impl LibraryProvider for ProviderStack {
    fn provider_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.providers
            .iter()
            .flat_map(|provider| provider.capabilities())
            .collect()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        self.provider_for(LibraryCapability::Catalog)?.catalog(library)
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        self.provider_for(LibraryCapability::List)?.list(library, path)
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        self.provider_for(LibraryCapability::Read)?.read(uri)
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        self.provider_for(LibraryCapability::Query)?.query(library, query)
    }

    fn refresh(&self) -> LibraryResult<()> {
        self.provider_for(LibraryCapability::Refresh)?.refresh()
    }
}

#[derive(Clone, Debug)]
struct ProcessProvider {
    id: String,
    library: LibraryId,
    command: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    capabilities: BTreeSet<LibraryCapability>,
    timeout: Duration,
}

impl ProcessProvider {
    fn from_declaration(
        declaration: &ProviderDeclaration,
        library: &LibraryId,
        root: &Path,
    ) -> Result<Self> {
        let command = required_config_string(declaration, "command")?;
        let command = substitute(&command, library, root);
        let command = resolve_executable(root, &command);
        let args = optional_config_strings(declaration, "args")?
            .into_iter()
            .map(|value| substitute(&value, library, root))
            .collect();
        let cwd = optional_config_string(declaration, "cwd")?
            .map(|value| root.join(substitute(&value, library, root)))
            .unwrap_or_else(|| root.to_path_buf());
        let timeout = optional_config_u64(declaration, "timeout_seconds")?
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(30));
        let mut env = BTreeMap::from([
            (
                "OKF_LIBRARY_ROOT".to_owned(),
                root.to_string_lossy().into_owned(),
            ),
            ("OKF_LIBRARY_ID".to_owned(), library.to_string()),
            ("OKF_PROVIDER_ID".to_owned(), declaration.id.clone()),
        ]);
        for name in optional_config_strings(declaration, "inherit_env")? {
            if let Some(value) = std::env::var_os(&name) {
                env.insert(name, value.to_string_lossy().into_owned());
            }
        }
        Ok(Self {
            id: declaration.id.clone(),
            library: library.clone(),
            command,
            args,
            cwd,
            env,
            capabilities: parse_capabilities(&declaration.capabilities)?,
            timeout,
        })
    }

    fn invoke<T: DeserializeOwned>(&self, request: &ProviderRequest) -> LibraryResult<T> {
        let mut child = ProcessCommand::new(&self.command)
            .args(&self.args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| provider_error(format!("failed to start '{}': {error}", self.id)))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| provider_error("provider stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| provider_error("provider stderr was not piped"))?;
        let stdout_reader = thread::spawn(move || read_all(stdout));
        let stderr_reader = thread::spawn(move || read_all(stderr));

        let bytes = serde_json::to_vec(request)
            .map_err(|error| provider_error(format!("failed to encode request: {error}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&bytes)
                .and_then(|()| stdin.write_all(b"\n"))
                .map_err(|error| provider_error(format!("failed to write provider request: {error}")))?;
        }

        let status = child
            .wait_timeout(self.timeout)
            .map_err(|error| provider_error(format!("failed waiting for provider: {error}")))?;
        let status = match status {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(provider_error(format!(
                    "provider '{}' timed out after {:?}",
                    self.id, self.timeout
                )));
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| provider_error("provider stdout reader panicked"))
            .and_then(|value| value)?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| provider_error("provider stderr reader panicked"))
            .and_then(|value| value)?;
        if !status.success() {
            return Err(provider_error(format!(
                "provider '{}' exited with {}: {}",
                self.id,
                status,
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        decode_provider_response(&stdout)
    }
}

impl LibraryProvider for ProcessProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.capabilities.clone()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::catalog(library))
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::list(library, path))
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        ensure_provider_library(&self.library, uri.library())?;
        self.invoke(&ProviderRequest::read(uri))
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::query(library, query))
    }

    fn refresh(&self) -> LibraryResult<()> {
        self.invoke(&ProviderRequest::refresh(&self.library))
    }
}

#[derive(Clone, Debug)]
struct HttpProvider {
    id: String,
    library: LibraryId,
    endpoint: String,
    bearer_token: Option<String>,
    capabilities: BTreeSet<LibraryCapability>,
    client: Client,
}

impl HttpProvider {
    fn from_declaration(
        declaration: &ProviderDeclaration,
        library: &LibraryId,
        _root: &Path,
    ) -> Result<Self> {
        let base_url = required_config_string(declaration, "base_url")?;
        if !base_url.starts_with("https://") && !base_url.starts_with("http://") {
            bail!("HTTP provider '{}' base_url must use http:// or https://", declaration.id);
        }
        let timeout = optional_config_u64(declaration, "timeout_seconds")?
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(30));
        let client = Client::builder().timeout(timeout).build()?;
        let bearer_token = optional_config_string(declaration, "token_env")?
            .map(|name| {
                std::env::var(&name).with_context(|| {
                    format!(
                        "HTTP provider '{}' requires credential environment variable '{}'",
                        declaration.id, name
                    )
                })
            })
            .transpose()?;
        Ok(Self {
            id: declaration.id.clone(),
            library: library.clone(),
            endpoint: format!("{}/v1/execute", base_url.trim_end_matches('/')),
            bearer_token,
            capabilities: parse_capabilities(&declaration.capabilities)?,
            client,
        })
    }

    fn invoke<T: DeserializeOwned>(&self, request: &ProviderRequest) -> LibraryResult<T> {
        let mut builder = self.client.post(&self.endpoint).json(request);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|error| provider_error(format!("HTTP provider '{}' failed: {error}", self.id)))?;
        let envelope: ProviderResponse = response
            .json()
            .map_err(|error| provider_error(format!("HTTP provider '{}' returned invalid JSON: {error}", self.id)))?;
        envelope.into_typed()
    }
}

impl LibraryProvider for HttpProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> BTreeSet<LibraryCapability> {
        self.capabilities.clone()
    }

    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::catalog(library))
    }

    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::list(library, path))
    }

    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> {
        ensure_provider_library(&self.library, uri.library())?;
        self.invoke(&ProviderRequest::read(uri))
    }

    fn query(
        &self,
        library: &LibraryId,
        query: &LibraryQuery,
    ) -> LibraryResult<LibraryQueryResult> {
        ensure_provider_library(&self.library, library)?;
        self.invoke(&ProviderRequest::query(library, query))
    }

    fn refresh(&self) -> LibraryResult<()> {
        self.invoke(&ProviderRequest::refresh(&self.library))
    }
}

#[derive(Clone, Debug, Serialize)]
struct ProviderRequest {
    protocol: &'static str,
    operation: &'static str,
    library: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<LibraryQuery>,
}

impl ProviderRequest {
    fn catalog(library: &LibraryId) -> Self {
        Self::new("catalog", library)
    }

    fn list(library: &LibraryId, path: &str) -> Self {
        let mut request = Self::new("list", library);
        request.path = Some(path.to_owned());
        request
    }

    fn read(uri: &KnowledgeUri) -> Self {
        let mut request = Self::new("read", uri.library());
        request.uri = Some(uri.to_string());
        request
    }

    fn query(library: &LibraryId, query: &LibraryQuery) -> Self {
        let mut request = Self::new("query", library);
        request.query = Some(query.clone());
        request
    }

    fn refresh(library: &LibraryId) -> Self {
        Self::new("refresh", library)
    }

    fn new(operation: &'static str, library: &LibraryId) -> Self {
        Self {
            protocol: PROVIDER_PROTOCOL,
            operation,
            library: library.to_string(),
            path: None,
            uri: None,
            query: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ProviderResponse {
    ok: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<ProviderResponseError>,
}

impl ProviderResponse {
    fn into_typed<T: DeserializeOwned>(self) -> LibraryResult<T> {
        if !self.ok {
            let error = self.error.unwrap_or(ProviderResponseError {
                code: "provider-error".to_owned(),
                message: "provider failed without a diagnostic".to_owned(),
            });
            return Err(provider_error(format!("{}: {}", error.code, error.message)));
        }
        serde_json::from_value(self.data.unwrap_or(Value::Null))
            .map_err(|error| provider_error(format!("invalid provider payload: {error}")))
    }
}

#[derive(Clone, Debug, Deserialize)]
struct ProviderResponseError {
    code: String,
    message: String,
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
            bail!(
                "local library source '{}' is not a directory",
                path.display()
            );
        }
        (LibrarySource::Local { path: path.clone() }, path)
    };

    let package = load_optional_package(&materialized)?;
    let providers = load_provider_declarations(&materialized)?;
    validate_provider_declarations(&providers)?;
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
        bail!("library '{}' is already installed", manifest.id);
    }

    let library_id = manifest.id.clone();
    registry.libraries.insert(
        library_id.to_string(),
        RegistryEntry {
            manifest: manifest.clone(),
            mounted: false,
            materialized: Some(materialized),
            package,
            providers,
            approved_provider_kinds: BTreeSet::new(),
        },
    );
    save_registry(registry_path, &registry)?;

    Outcome::success(
        format!("installed {}", library_id),
        json!({
            "library": manifest,
            "mounted": false,
            "provider_kinds": registry
                .libraries
                .get(library_id.as_str())
                .map(|entry| declared_provider_kinds(&entry.providers))
                .unwrap_or_default(),
        }),
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

    let path = match &entry.manifest.source {
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
            path.clone()
        }
        Some(LibrarySource::Local { path }) => {
            if !path.is_dir() {
                bail!(
                    "local library source '{}' is no longer available",
                    path.display()
                );
            }
            path.clone()
        }
        Some(LibrarySource::Custom { .. }) => {
            bail!("custom source update requires a source adapter")
        }
        None => bail!("library '{id}' does not declare a source"),
    };

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
    let providers = load_provider_declarations(&path)?;
    validate_provider_declarations(&providers)?;
    entry.package = package;
    entry.providers = providers;

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
        .ok_or_else(|| anyhow!("library '{id}' is not installed"))?;
    if mounted {
        entry
            .approved_provider_kinds
            .extend(allow_provider.iter().map(|value| value.trim().to_ascii_lowercase()));
        ensure_provider_authorization(entry)?;
        let _ = resolve_instance(entry)?;
    }
    entry.mounted = mounted;
    save_registry(registry_path, &registry)?;
    Outcome::success(
        format!("{} {id}", if mounted { "mounted" } else { "unmounted" }),
        json!({
            "id": id,
            "mounted": mounted,
            "approved_provider_kinds": entry.approved_provider_kinds,
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
                "providers": entry.providers,
                "approved_provider_kinds": entry.approved_provider_kinds,
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
                let providers = declared_provider_kinds(&entry.providers)
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{}\t{}\t{}\tproviders:{}",
                    if entry.mounted {
                        "mounted"
                    } else {
                        "unmounted"
                    },
                    id,
                    entry.manifest.name,
                    if providers.is_empty() { "-" } else { &providers }
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
        let library_id =
            LibraryId::parse(id.to_owned()).map_err(|error| anyhow!(error.to_string()))?;
        vec![
            runtime
                .catalog(&library_id)
                .map_err(|error| anyhow!(error.to_string()))?,
        ]
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
                catalog
                    .entries
                    .iter()
                    .map(move |entry| format!("{}\t{}\t{}", catalog.library, entry.id, entry.title))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Outcome::success(human, catalogs)
}

pub(crate) fn read(registry_path: &Path, uri: &str) -> Result<Outcome> {
    let runtime = build_runtime(registry_path)?;
    let uri = KnowledgeUri::parse(uri).map_err(|error| anyhow!(error.to_string()))?;
    let content = runtime
        .read(&uri)
        .map_err(|error| anyhow!(error.to_string()))?;
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
        let id =
            LibraryId::parse(library.to_owned()).map_err(|error| anyhow!(error.to_string()))?;
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

pub(crate) fn list_nodes(registry_path: &Path, library: &str, path: &str) -> Result<Vec<KnowledgeNode>> {
    let runtime = build_runtime(registry_path)?;
    let id = LibraryId::parse(library.to_owned()).map_err(|error| anyhow!(error.to_string()))?;
    runtime
        .list(&id, path)
        .map_err(|error| anyhow!(error.to_string()))
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
    for entry in registry_file
        .libraries
        .values()
        .filter(|entry| entry.mounted)
    {
        ensure_provider_authorization(entry)?;
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
    let path = materialized_path(entry)?;
    let bundle = BundleParser::default().parse_dir(path).with_context(|| {
        format!(
            "failed to load Library '{}' from {}",
            entry.manifest.id,
            path.display()
        )
    })?;
    let catalog = entry
        .package
        .as_ref()
        .map(LibraryPackageManifest::runtime_catalog)
        .transpose()
        .map_err(|error| anyhow!(error.to_string()))?;
    let fallback = Arc::new(ManifestBundleProvider::new(
        BundleLibraryProvider::new(bundle),
        catalog,
    ));

    if entry.providers.is_empty() {
        return Ok(LibraryInstance::new(entry.manifest.clone(), fallback));
    }

    let mut stack = ProviderStack::new(format!("{}-providers", entry.manifest.id));
    for declaration in &entry.providers {
        let provider: Arc<dyn LibraryProvider> = match declaration.kind.trim().to_ascii_lowercase().as_str() {
            "process" => Arc::new(ProcessProvider::from_declaration(
                declaration,
                &entry.manifest.id,
                path,
            )?),
            "http" => Arc::new(HttpProvider::from_declaration(
                declaration,
                &entry.manifest.id,
                path,
            )?),
            kind => {
                bail!(
                    "Library '{}' declares provider kind '{}' which this CLI does not activate directly; wrap it with an okf-provider/1 process or HTTP adapter",
                    entry.manifest.id,
                    kind
                )
            }
        };
        stack.push(provider);
    }
    stack.push(fallback);
    Ok(LibraryInstance::new(entry.manifest.clone(), Arc::new(stack)))
}

fn ensure_provider_authorization(entry: &RegistryEntry) -> Result<()> {
    let declared = declared_provider_kinds(&entry.providers);
    let unauthorized = declared
        .difference(&entry.approved_provider_kinds)
        .cloned()
        .collect::<Vec<_>>();
    if unauthorized.is_empty() {
        Ok(())
    } else {
        bail!(
            "Library '{}' declares provider deployment kind(s) {}. Mounting them may execute code, access the network, or read deployment credentials. Re-run mount with {} after reviewing okf-library.yaml",
            entry.manifest.id,
            unauthorized.join(", "),
            unauthorized
                .iter()
                .map(|kind| format!("--allow-provider {kind}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn declared_provider_kinds(providers: &[ProviderDeclaration]) -> BTreeSet<String> {
    providers
        .iter()
        .map(|provider| provider.kind.trim().to_ascii_lowercase())
        .collect()
}

fn validate_provider_declarations(providers: &[ProviderDeclaration]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for provider in providers {
        if provider.id.trim().is_empty() || provider.kind.trim().is_empty() {
            bail!("provider id and kind must be non-empty");
        }
        if !ids.insert(provider.id.as_str()) {
            bail!("duplicate provider id '{}'", provider.id);
        }
        if provider.capabilities.is_empty() {
            bail!("provider '{}' must declare capabilities", provider.id);
        }
        let _ = parse_capabilities(&provider.capabilities)?;
    }
    Ok(())
}

fn parse_capabilities(values: &BTreeSet<String>) -> Result<BTreeSet<LibraryCapability>> {
    values
        .iter()
        .map(|value| {
            match value.trim().to_ascii_lowercase().as_str() {
                "list" => Ok(LibraryCapability::List),
                "read" => Ok(LibraryCapability::Read),
                "catalog" => Ok(LibraryCapability::Catalog),
                "query" => Ok(LibraryCapability::Query),
                "refresh" => Ok(LibraryCapability::Refresh),
                "maintain" => Ok(LibraryCapability::Maintain),
                other => bail!("unknown Library provider capability '{other}'"),
            }
        })
        .collect()
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

fn load_provider_declarations(path: &Path) -> Result<Vec<ProviderDeclaration>> {
    let manifest = path.join("okf-library.yaml");
    if !manifest.is_file() {
        return Ok(Vec::new());
    }
    let source = fs::read_to_string(&manifest)
        .with_context(|| format!("failed to read {}", manifest.display()))?;
    let deployment = yaml_serde::from_str::<DeploymentManifest>(&source)
        .with_context(|| format!("failed to parse provider deployments in {}", manifest.display()))?;
    Ok(deployment.providers)
}

fn materialized_path(entry: &RegistryEntry) -> Result<&Path> {
    match &entry.manifest.source {
        Some(LibrarySource::Local { path }) => Ok(path),
        Some(LibrarySource::Git { .. }) => entry
            .materialized
            .as_deref()
            .ok_or_else(|| anyhow!("git library '{}' is not materialized", entry.manifest.id)),
        Some(LibrarySource::Custom { kind, .. }) => {
            bail!("custom library source '{kind}' requires a source acquisition adapter")
        }
        None => bail!("library '{}' does not declare a source", entry.manifest.id),
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

fn cleanup_failed_git_install(source: &LibrarySource, path: &Path) -> Result<()> {
    if matches!(source, LibrarySource::Git { .. }) && path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to clean up {}", path.display()))?;
    }
    Ok(())
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

fn required_config_string(provider: &ProviderDeclaration, key: &str) -> Result<String> {
    optional_config_string(provider, key)?.ok_or_else(|| {
        anyhow!(
            "provider '{}' kind '{}' requires string config '{}'",
            provider.id,
            provider.kind,
            key
        )
    })
}

fn optional_config_string(provider: &ProviderDeclaration, key: &str) -> Result<Option<String>> {
    provider
        .config
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("provider '{}' config '{}' must be a string", provider.id, key))
        })
        .transpose()
}

fn optional_config_strings(provider: &ProviderDeclaration, key: &str) -> Result<Vec<String>> {
    let Some(value) = provider.config.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| anyhow!("provider '{}' config '{}' must be a string array", provider.id, key))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("provider '{}' config '{}' must contain strings", provider.id, key))
        })
        .collect()
}

fn optional_config_u64(provider: &ProviderDeclaration, key: &str) -> Result<Option<u64>> {
    provider
        .config
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| anyhow!("provider '{}' config '{}' must be a positive integer", provider.id, key))
        })
        .transpose()
}

fn substitute(value: &str, library: &LibraryId, root: &Path) -> String {
    value
        .replace("${library_root}", &root.to_string_lossy())
        .replace("${library_id}", library.as_str())
}

fn resolve_executable(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.components().count() > 1 && path.is_relative() {
        root.join(path)
    } else {
        path
    }
}

fn ensure_provider_library(expected: &LibraryId, actual: &LibraryId) -> LibraryResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(provider_error(format!(
            "provider belongs to Library '{}' but was called for '{}'",
            expected, actual
        )))
    }
}

fn decode_provider_response<T: DeserializeOwned>(bytes: &[u8]) -> LibraryResult<T> {
    let envelope: ProviderResponse = serde_json::from_slice(bytes)
        .map_err(|error| provider_error(format!("provider returned invalid JSON: {error}")))?;
    envelope.into_typed()
}

fn read_all(mut reader: impl Read) -> LibraryResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| provider_error(format!("failed reading provider output: {error}")))?;
    Ok(bytes)
}

fn provider_error(message: impl Into<String>) -> okf::LibraryError {
    okf::LibraryError::Provider(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_manifest_is_backward_compatible() {
        let old = yaml_serde::from_str::<DeploymentManifest>(
            "schema_version: \"1\"\nid: demo\nname: Demo\n",
        )
        .expect("old manifest");
        assert!(old.providers.is_empty());
    }

    #[test]
    fn provider_authorization_fails_closed() {
        let id = LibraryId::parse("demo").expect("library");
        let entry = RegistryEntry {
            manifest: LibraryManifest::new(id, "Demo"),
            mounted: false,
            materialized: None,
            package: None,
            providers: vec![ProviderDeclaration {
                id: "p".to_owned(),
                kind: "process".to_owned(),
                capabilities: BTreeSet::from(["read".to_owned()]),
                config: BTreeMap::new(),
            }],
            approved_provider_kinds: BTreeSet::new(),
        };
        assert!(ensure_provider_authorization(&entry).is_err());
    }

    #[test]
    fn substitution_is_library_scoped() {
        let id = LibraryId::parse("demo").expect("library");
        let value = substitute(
            "${library_root}/${library_id}",
            &id,
            Path::new("/tmp/library"),
        );
        assert!(value.ends_with("/tmp/library/demo"));
    }
}
