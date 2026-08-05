//! Deterministic in-memory lexical retrieval.

use std::collections::{BTreeSet, HashSet};
use serde::{Deserialize, Serialize};
use crate::model::{Bundle, Document};

/// A document field that contributed to a search hit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchField { Id, Title, Summary, Tag, Alias, Body }
/// A deterministic lexical query.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchQuery { text: String, tags: BTreeSet<String>, limit: usize }
impl SearchQuery {
    /// Creates a text query with a default limit of 20 hits.
    pub fn new(text: impl Into<String>) -> Self { Self { text: text.into(), tags: BTreeSet::new(), limit: 20 } }
    /// Requires a tag. Multiple calls use AND semantics.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self { let tag = tag.into(); self.tags.insert(normalize(&tag)); self }
    /// Sets the maximum number of returned hits.
    pub fn limit(mut self, limit: usize) -> Self { self.limit = limit; self }
    /// Returns the original query text.
    pub fn text(&self) -> &str { &self.text }
    /// Returns normalized required tags.
    pub fn tags(&self) -> &BTreeSet<String> { &self.tags }
    /// Returns the maximum number of hits.
    pub fn max_results(&self) -> usize { self.limit }
}
/// One ranked search result.
#[derive(Clone, Debug)]
pub struct SearchHit<'a> { pub document: &'a Document, pub score: u32, pub matched_fields: BTreeSet<MatchField>, pub snippet: String }
/// Searches a bundle with deterministic ranking and tie-breaking.
pub fn search<'a>(bundle: &'a Bundle, query: &SearchQuery) -> Vec<SearchHit<'a>> {
    if query.limit == 0 { return Vec::new(); }
    let normalized_text = normalize(&query.text);
    let terms = tokenize(&normalized_text);
    let mut hits = Vec::new();
    for document in bundle.documents() {
        if !matches_required_tags(document, &query.tags) { continue; }
        let mut score = 0;
        let mut matched_fields = BTreeSet::new();
        let id = normalize(document.id().as_str());
        let title = normalize(document.title());
        if !normalized_text.is_empty() && title == normalized_text { score += 100; matched_fields.insert(MatchField::Title); }
        score += score_field(&id, &terms, 16, MatchField::Id, &mut matched_fields);
        score += score_field(&title, &terms, 24, MatchField::Title, &mut matched_fields);
        if let Some(summary) = &document.metadata().summary { score += score_field(&normalize(summary), &terms, 8, MatchField::Summary, &mut matched_fields); }
        for tag in &document.metadata().tags { score += score_field(&normalize(tag), &terms, 12, MatchField::Tag, &mut matched_fields); }
        for alias in &document.metadata().aliases { score += score_field(&normalize(alias), &terms, 14, MatchField::Alias, &mut matched_fields); }
        score += score_field(&normalize(document.body()), &terms, 2, MatchField::Body, &mut matched_fields);
        if !terms.is_empty() && score == 0 { continue; }
        hits.push(SearchHit { document, score, matched_fields, snippet: make_snippet(document.body(), &terms) });
    }
    hits.sort_by(|left, right| right.score.cmp(&left.score).then_with(|| left.document.id().cmp(right.document.id())));
    hits.truncate(query.limit);
    hits
}
fn score_field(field: &str, terms: &[String], weight: u32, matched_field: MatchField, matched_fields: &mut BTreeSet<MatchField>) -> u32 {
    let matches = terms.iter().filter(|term| field.contains(term.as_str())).count() as u32;
    if matches > 0 { matched_fields.insert(matched_field); }
    matches * weight
}
fn matches_required_tags(document: &Document, required: &BTreeSet<String>) -> bool {
    if required.is_empty() { return true; }
    let document_tags = document.metadata().tags.iter().map(|tag| normalize(tag)).collect::<HashSet<_>>();
    required.iter().all(|tag| document_tags.contains(tag))
}
fn tokenize(text: &str) -> Vec<String> { let mut terms = text.split(|character: char| !character.is_alphanumeric()).filter(|term| !term.is_empty()).map(str::to_owned).collect::<Vec<_>>(); terms.sort(); terms.dedup(); terms }
fn normalize(value: &str) -> String { value.trim().to_lowercase() }
fn make_snippet(body: &str, terms: &[String]) -> String {
    let selected = body.lines().map(str::trim).filter(|line| !line.is_empty()).find(|line| { let normalized = normalize(line); terms.is_empty() || terms.iter().any(|term| normalized.contains(term)) }).or_else(|| body.lines().map(str::trim).find(|line| !line.is_empty())).unwrap_or_default();
    truncate_chars(selected, 180)
}
fn truncate_chars(value: &str, max_chars: usize) -> String { let mut characters = value.chars(); let prefix = characters.by_ref().take(max_chars).collect::<String>(); if characters.next().is_some() { format!("{prefix}…") } else { prefix } }
