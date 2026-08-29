//! Markdown and bundle-directory parsing.

mod frontmatter;
use crate::error::{Error, Result};
use crate::model::{Bundle, Document, DocumentId, Metadata};
use frontmatter::{FrontMatter, FrontMatterParts, split_front_matter};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};
use walkdir::{DirEntry, WalkDir};

/// Options controlling directory traversal and recognized document files.
#[derive(Clone, Debug)]
pub struct ParserOptions {
    pub extensions: BTreeSet<String>,
    pub include_hidden: bool,
    pub follow_links: bool,
}
impl Default for ParserOptions {
    fn default() -> Self {
        Self {
            extensions: BTreeSet::from(["md".to_owned(), "markdown".to_owned()]),
            include_hidden: false,
            follow_links: false,
        }
    }
}
/// Loads Markdown documents into an in-memory [`Bundle`].
#[derive(Clone, Debug, Default)]
pub struct BundleParser {
    options: ParserOptions,
}
impl BundleParser {
    /// Creates a parser using explicit options.
    pub fn new(options: ParserOptions) -> Self {
        Self { options }
    }
    /// Returns the active parser options.
    pub fn options(&self) -> &ParserOptions {
        &self.options
    }
    /// Recursively parses a bundle directory.
    pub fn parse_dir(&self, root: impl AsRef<Path>) -> Result<Bundle> {
        let root = root.as_ref().to_path_buf();
        let metadata = fs::metadata(&root).map_err(|source| Error::io(&root, source))?;
        if !metadata.is_dir() {
            return Err(Error::NotDirectory { path: root });
        }
        let mut bundle = Bundle::new(&root);
        let walker = WalkDir::new(&root)
            .follow_links(self.options.follow_links)
            .into_iter();
        for entry in walker.filter_entry(|entry| self.should_visit(entry, &root)) {
            let entry = entry.map_err(|source| Error::Walk {
                path: source.path().map(Path::to_path_buf),
                source,
            })?;
            if !entry.file_type().is_file() || !self.is_document_path(entry.path()) {
                continue;
            }
            let source =
                fs::read_to_string(entry.path()).map_err(|error| Error::io(entry.path(), error))?;
            let relative_path = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            bundle.insert(parse_document_internal(&source, relative_path)?)?;
        }
        Ok(bundle)
    }
    fn should_visit(&self, entry: &DirEntry, root: &Path) -> bool {
        self.options.include_hidden
            || entry.path() == root
            || !entry.file_name().to_string_lossy().starts_with('.')
    }
    fn is_document_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|extension| self.options.extensions.contains(&extension))
    }
}
/// Parses one Markdown document using `source_path` for identifier derivation and diagnostics.
pub fn parse_document(source: &str, source_path: impl AsRef<Path>) -> Result<Document> {
    parse_document_internal(source, source_path.as_ref())
}
fn parse_document_internal(source: &str, source_path: &Path) -> Result<Document> {
    let (front_matter, body) = match split_front_matter(source) {
        Ok(FrontMatterParts::None(body)) => (FrontMatter::default(), body),
        Ok(FrontMatterParts::Present { yaml, body }) => {
            let front_matter =
                yaml_serde::from_str::<FrontMatter>(yaml).map_err(|source| Error::FrontMatter {
                    path: source_path.to_path_buf(),
                    source,
                })?;
            (front_matter, body)
        }
        Err(()) => {
            return Err(Error::UnterminatedFrontMatter {
                path: source_path.to_path_buf(),
            });
        }
    };
    let id = match front_matter.id {
        Some(id) => DocumentId::new(id)?,
        None => document_id_from_path(source_path)?,
    };
    let body = body
        .trim_start_matches(|character| matches!(character, '\r' | '\n'))
        .to_owned();
    let title = front_matter
        .title
        .unwrap_or_else(|| infer_title(&body, &id));
    let metadata = Metadata {
        summary: front_matter.summary,
        tags: front_matter.tags,
        aliases: front_matter.aliases,
        links: front_matter.links,
        extra: front_matter.extra,
    };
    Ok(Document::new(id, title, body, metadata).with_source_path(source_path))
}
fn document_id_from_path(path: &Path) -> Result<DocumentId> {
    let mut parts = Vec::new();
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return DocumentId::new(path.to_string_lossy().replace('\\', "/"))
                    .map_err(Into::into);
            }
        }
    }
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    parts.push(stem);
    if parts.len() > 1 && parts.last().is_some_and(|part| part == "index") {
        parts.pop();
    }
    DocumentId::new(parts.join("/")).map_err(Into::into)
}
fn infer_title(body: &str, id: &DocumentId) -> String {
    body.lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| id.name().replace('-', " ").replace('_', " "))
}
