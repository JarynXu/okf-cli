//! External process-backed Library provider adapter.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::library::{
    KnowledgeNode, KnowledgeUri, LibraryCapability, LibraryCatalog, LibraryError, LibraryId,
    LibraryProvider, LibraryQuery, LibraryQueryResult, LibraryResult,
};
use crate::provider_protocol::{ProviderRequest, ProviderResponse, decode_provider_response};

/// Executes Library capabilities through `okf-provider/1`.
#[derive(Clone, Debug)]
pub struct ProcessLibraryProvider {
    id: String,
    library: LibraryId,
    command: PathBuf,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    env: BTreeMap<String, String>,
    capabilities: BTreeSet<LibraryCapability>,
    timeout: Duration,
}

impl ProcessLibraryProvider {
    /// Creates a process provider with a 30 second request timeout.
    pub fn new(id: impl Into<String>, library: LibraryId, command: impl Into<PathBuf>, capabilities: impl IntoIterator<Item = LibraryCapability>) -> Self {
        Self { id: id.into(), library, command: command.into(), args: Vec::new(), cwd: None, env: minimal_environment(), capabilities: capabilities.into_iter().collect(), timeout: Duration::from_secs(30) }
    }
    /// Sets arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self { self.args = args.into_iter().map(Into::into).collect(); self }
    /// Sets working directory.
    pub fn cwd(mut self, path: impl Into<PathBuf>) -> Self { self.cwd = Some(path.into()); self }
    /// Adds one explicitly authorized environment value.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self { self.env.insert(name.into(), value.into()); self }
    /// Explicitly inherits selected host variables.
    pub fn inherit_environment(mut self, names: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        for name in names { let name = name.as_ref(); if let Some(value) = std::env::var_os(name) { self.env.insert(name.to_owned(), value.to_string_lossy().into_owned()); } }
        self
    }
    /// Sets timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self { self.timeout = timeout; self }
    fn ensure_library(&self, library: &LibraryId) -> LibraryResult<()> {
        if library == &self.library { Ok(()) } else { Err(LibraryError::Provider(format!("process provider '{}' belongs to Library '{}' but was called for '{}'", self.id, self.library, library))) }
    }
    fn invoke(&self, request: &ProviderRequest) -> LibraryResult<ProviderResponse> {
        let mut command = Command::new(&self.command);
        command.args(&self.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env_clear().envs(&self.env).env("OKF_LIBRARY_ID", self.library.as_str()).env("OKF_PROVIDER_ID", &self.id);
        if let Some(cwd) = &self.cwd { command.current_dir(cwd); }
        let mut child = command.spawn().map_err(|error| LibraryError::Provider(format!("failed to start process provider '{}': {error}", self.id)))?;
        let stdout = child.stdout.take().ok_or_else(|| LibraryError::Provider("process stdout was not piped".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| LibraryError::Provider("process stderr was not piped".into()))?;
        let stdout_reader = thread::spawn(move || read_all(stdout));
        let stderr_reader = thread::spawn(move || read_all(stderr));
        let bytes = serde_json::to_vec(request).map_err(|error| LibraryError::Provider(format!("failed to encode provider request: {error}")))?;
        if let Some(mut stdin) = child.stdin.take() { stdin.write_all(&bytes).map_err(io_error)?; stdin.write_all(b"\n").map_err(io_error)?; }
        let status = wait_with_timeout(&mut child, self.timeout)?;
        let stdout = stdout_reader.join().map_err(|_| LibraryError::Provider("provider stdout reader panicked".into()))??;
        let stderr = stderr_reader.join().map_err(|_| LibraryError::Provider("provider stderr reader panicked".into()))??;
        if !status.success() { return Err(LibraryError::Provider(format!("process provider '{}' exited with {}: {}", self.id, status, String::from_utf8_lossy(&stderr).trim()))); }
        decode_provider_response(&stdout)
    }
}

impl LibraryProvider for ProcessLibraryProvider {
    fn provider_id(&self) -> &str { &self.id }
    fn capabilities(&self) -> BTreeSet<LibraryCapability> { self.capabilities.clone() }
    fn catalog(&self, library: &LibraryId) -> LibraryResult<LibraryCatalog> { self.ensure_library(library)?; self.invoke(&ProviderRequest::catalog(library.clone()))?.into_catalog() }
    fn list(&self, library: &LibraryId, path: &str) -> LibraryResult<Vec<KnowledgeNode>> { self.ensure_library(library)?; self.invoke(&ProviderRequest::list(library.clone(), path))?.into_nodes() }
    fn read(&self, uri: &KnowledgeUri) -> LibraryResult<String> { self.ensure_library(uri.library())?; self.invoke(&ProviderRequest::read(uri.clone()))?.into_typed() }
    fn query(&self, library: &LibraryId, query: &LibraryQuery) -> LibraryResult<LibraryQueryResult> { self.ensure_library(library)?; self.invoke(&ProviderRequest::query(library.clone(), query.clone()))?.into_query_result() }
    fn refresh(&self) -> LibraryResult<()> { self.invoke(&ProviderRequest::refresh(self.library.clone()))?.into_typed() }
}

fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> LibraryResult<ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| LibraryError::Provider(format!("failed waiting for provider: {error}")))? { return Ok(status); }
        if started.elapsed() >= timeout { let _ = child.kill(); let _ = child.wait(); return Err(LibraryError::Provider(format!("process provider timed out after {timeout:?}"))); }
        thread::sleep(Duration::from_millis(10));
    }
}
fn minimal_environment() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for name in ["PATH", "PATHEXT", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] { if let Some(value) = std::env::var_os(name) { env.insert(name.to_owned(), value.to_string_lossy().into_owned()); } }
    env
}
fn read_all(mut reader: impl Read) -> LibraryResult<Vec<u8>> { let mut bytes = Vec::new(); reader.read_to_end(&mut bytes).map_err(io_error)?; Ok(bytes) }
fn io_error(error: impl std::fmt::Display) -> LibraryError { LibraryError::Provider(format!("process provider I/O error: {error}")) }
