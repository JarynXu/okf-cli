use std::collections::{BTreeMap, BTreeSet};
use serde::Deserialize;
use crate::model::Reference;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FrontMatter {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) summary: Option<String>,
    #[serde(default)] pub(crate) tags: BTreeSet<String>,
    #[serde(default)] pub(crate) aliases: BTreeSet<String>,
    #[serde(default, alias = "references")] pub(crate) links: Vec<Reference>,
    #[serde(default, flatten)] pub(crate) extra: BTreeMap<String, serde_json::Value>,
}
pub(crate) enum FrontMatterParts<'a> { None(&'a str), Present { yaml: &'a str, body: &'a str } }
pub(crate) fn split_front_matter(input: &str) -> Result<FrontMatterParts<'_>, ()> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let Some(first_line_end) = input.find('\n') else {
        return if input.trim_end_matches('\r') == "---" { Err(()) } else { Ok(FrontMatterParts::None(input)) };
    };
    if input[..first_line_end].trim_end_matches('\r') != "---" { return Ok(FrontMatterParts::None(input)); }
    let yaml_start = first_line_end + 1;
    let mut line_start = yaml_start;
    while line_start <= input.len() {
        let remaining = &input[line_start..];
        let line_end = remaining.find('\n').map_or(input.len(), |relative| line_start + relative);
        let line = input[line_start..line_end].trim_end_matches('\r');
        if matches!(line, "---" | "...") {
            let body_start = if line_end < input.len() { line_end + 1 } else { line_end };
            return Ok(FrontMatterParts::Present { yaml: &input[yaml_start..line_start], body: &input[body_start..] });
        }
        if line_end == input.len() { break; }
        line_start = line_end + 1;
    }
    Err(())
}
