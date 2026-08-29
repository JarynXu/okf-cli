use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::libraries;
use crate::output::Outcome;

const PROFILE_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ImpactRule {
    topic: String,
    path_prefixes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Profile {
    schema_version: String,
    project: String,
    library_id: String,
    repository: PathBuf,
    validated_revision: Option<String>,
    impact_rules: Vec<ImpactRule>,
}

pub(crate) fn init(
    registry_path: &Path,
    profile_path: &Path,
    repository: &Path,
    project: Option<&str>,
    id: &str,
    force: bool,
) -> Result<Outcome> {
    let repository = repository
        .canonicalize()
        .with_context(|| format!("failed to resolve repository {}", repository.display()))?;
    ensure_git_repository(&repository)?;
    let project_name = project
        .map(str::to_owned)
        .or_else(|| {
            repository
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| id.to_owned());
    let bundle = profile_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("project-context");
    if bundle.exists() && !force {
        bail!(
            "project context bundle '{}' already exists; pass --force to rebuild it",
            bundle.display()
        );
    }
    if bundle.exists() {
        fs::remove_dir_all(&bundle)
            .with_context(|| format!("failed to reset {}", bundle.display()))?;
    }
    create_scaffold(&bundle, id, &project_name)?;
    let current = current_revision(&repository).ok();
    let profile = Profile {
        schema_version: PROFILE_SCHEMA_VERSION.to_owned(),
        project: project_name.clone(),
        library_id: id.to_owned(),
        repository: repository.clone(),
        validated_revision: None,
        impact_rules: default_impact_rules(id),
    };
    save_profile(profile_path, &profile)?;

    if !force {
        libraries::add_library(
            registry_path,
            bundle.to_string_lossy().as_ref(),
            Some(id),
            Some(&format!("{project_name} Project Context")),
            None,
        )?;
        libraries::set_mounted(registry_path, id, true)?;
    }

    Outcome::success(
        format!(
            "initialized Project Context Library '{}' at {}",
            id,
            bundle.display()
        ),
        json!({
            "project": project_name,
            "library_id": id,
            "bundle": bundle,
            "profile": profile_path,
            "current_revision": current,
            "state": "UNINITIALIZED",
            "mounted": !force,
        }),
    )
}

pub(crate) fn status(profile_path: &Path) -> Result<Outcome> {
    if !profile_path.exists() {
        return Outcome::success(
            "project context is UNINITIALIZED",
            json!({"state": "UNINITIALIZED", "profile": profile_path}),
        );
    }
    let profile = load_profile(profile_path)?;
    let current = current_revision(&profile.repository).ok();
    let (state, changed_paths) = match (&profile.validated_revision, &current) {
        (None, _) => ("UNINITIALIZED", Vec::new()),
        (Some(_), None) => ("UNKNOWN", Vec::new()),
        (Some(validated), Some(current)) if validated == current => ("VALID", Vec::new()),
        (Some(validated), Some(current)) => {
            match changed_paths(&profile.repository, validated, current) {
                Ok(paths) => ("DIRTY", paths),
                Err(_) => ("UNKNOWN", Vec::new()),
            }
        }
    };
    let impacted_topics = impacted_topics(&changed_paths, &profile.impact_rules);
    Outcome::success(
        format!(
            "{}: validated={}, current={}, changed={}, impacted={}",
            state,
            profile.validated_revision.as_deref().unwrap_or("<none>"),
            current.as_deref().unwrap_or("<unknown>"),
            changed_paths.len(),
            impacted_topics.len()
        ),
        json!({
            "state": state,
            "project": profile.project,
            "library_id": profile.library_id,
            "repository": profile.repository,
            "validated_revision": profile.validated_revision,
            "current_revision": current,
            "changed_paths": changed_paths,
            "impacted_topics": impacted_topics,
        }),
    )
}

pub(crate) fn checkpoint(profile_path: &Path, revision: Option<&str>) -> Result<Outcome> {
    let mut profile = load_profile(profile_path)?;
    let revision = revision
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(|| current_revision(&profile.repository))?;
    verify_revision(&profile.repository, &revision)?;
    profile.validated_revision = Some(revision.clone());
    save_profile(profile_path, &profile)?;
    append_history(profile_path, &revision)?;
    Outcome::success(
        format!("advanced Project Context checkpoint to {revision}"),
        json!({
            "state": "VALID",
            "project": profile.project,
            "validated_revision": revision,
        }),
    )
}

fn create_scaffold(root: &Path, id: &str, project: &str) -> Result<()> {
    fs::create_dir_all(root.join("current"))?;
    fs::create_dir_all(root.join("history"))?;
    fs::write(
        root.join("okf-library.yaml"),
        format!(
            "schema_version: \"1\"\nid: {id}\nname: {project} Project Context\nversion: \"1\"\n\ncatalog:\n  - id: architecture\n    title: Architecture\n    description: Current system structure, boundaries, and major flows.\n    path: current/architecture\n    terms: [architecture, modules, boundaries]\n  - id: constraints\n    title: Constraints\n    description: Durable technical and product constraints.\n    path: current/constraints\n    terms: [constraints, invariants, rules]\n  - id: decisions\n    title: Decisions\n    description: Active architectural and implementation decisions.\n    path: current/decisions\n    terms: [decisions, adr, rationale]\n  - id: components\n    title: Components\n    description: Component responsibilities and ownership.\n    path: current/components\n    terms: [components, modules, packages]\n  - id: history\n    title: Project history\n    description: Append-only evolution and checkpoint history.\n    path: history/log\n    terms: [history, changes, checkpoints]\n\nquery:\n  preferred: lexical\n  capabilities: [lexical]\n  hints:\n    - Prefer current/ for present-tense project understanding.\n    - Use history/log for why and when the project changed.\n"
        ),
    )?;
    write_doc(
        &root.join("index.md"),
        "Project Context",
        "Entry point for durable, revision-bound project knowledge.",
        "project-context",
    )?;
    for (name, title, summary, tags) in [
        (
            "architecture.md",
            "Architecture",
            "Current architecture, boundaries, dependencies, and major flows.",
            "architecture,current",
        ),
        (
            "constraints.md",
            "Constraints",
            "Current invariants, compatibility requirements, and non-negotiable constraints.",
            "constraints,current",
        ),
        (
            "decisions.md",
            "Decisions",
            "Active decisions with rationale and supersession notes.",
            "decisions,current",
        ),
        (
            "components.md",
            "Components",
            "Current component responsibilities, interfaces, and ownership boundaries.",
            "components,current",
        ),
    ] {
        write_doc(&root.join("current").join(name), title, summary, tags)?;
    }
    write_doc(
        &root.join("history/log.md"),
        "Project History",
        "Append-only history of material context changes and validated checkpoints.",
        "history,log",
    )?;
    Ok(())
}

fn write_doc(path: &Path, title: &str, summary: &str, tags: &str) -> Result<()> {
    fs::write(
        path,
        format!(
            "---\ntitle: {title}\nsummary: {summary}\ntags: [{tags}]\n---\n# {title}\n\n<!-- Maintained by an authorized Project Context workflow. -->\n"
        ),
    )?;
    Ok(())
}

fn default_impact_rules(id: &str) -> Vec<ImpactRule> {
    vec![
        ImpactRule {
            topic: format!("okf://{id}/current/architecture"),
            path_prefixes: vec![
                "src".into(),
                "packages".into(),
                "crates".into(),
                "docs".into(),
            ],
        },
        ImpactRule {
            topic: format!("okf://{id}/current/components"),
            path_prefixes: vec!["src".into(), "packages".into(), "crates".into()],
        },
        ImpactRule {
            topic: format!("okf://{id}/current/constraints"),
            path_prefixes: vec![
                ".github".into(),
                "Cargo.toml".into(),
                "package.json".into(),
                "pom.xml".into(),
                "build.gradle".into(),
            ],
        },
        ImpactRule {
            topic: format!("okf://{id}/current/decisions"),
            path_prefixes: vec!["docs".into(), "adr".into(), "decisions".into()],
        },
    ]
}

fn ensure_git_repository(repository: &Path) -> Result<()> {
    current_revision(repository).map(|_| ())
}

fn current_revision(repository: &Path) -> Result<String> {
    git_output(repository, &["rev-parse", "HEAD"])
}

fn verify_revision(repository: &Path, revision: &str) -> Result<()> {
    git_output(
        repository,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )
    .map(|_| ())
}

fn changed_paths(repository: &Path, from: &str, to: &str) -> Result<Vec<String>> {
    let output = git_output(
        repository,
        &["diff", "--name-only", &format!("{from}..{to}")],
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn git_output(repository: &Path, args: &[&str]) -> Result<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .with_context(|| "failed to execute git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|error| anyhow!("git output was not UTF-8: {error}"))?
        .trim()
        .to_owned())
}

fn impacted_topics(changed: &[String], rules: &[ImpactRule]) -> Vec<String> {
    let mut topics = BTreeSet::new();
    for rule in rules {
        if rule.path_prefixes.iter().any(|prefix| {
            changed.iter().any(|path| {
                path == prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        }) {
            topics.insert(rule.topic.clone());
        }
    }
    topics.into_iter().collect()
}

fn load_profile(path: &Path) -> Result<Profile> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let profile: Profile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if profile.schema_version != PROFILE_SCHEMA_VERSION {
        bail!(
            "unsupported Project Context profile schema version '{}'",
            profile.schema_version
        );
    }
    Ok(profile)
}

fn save_profile(path: &Path, profile: &Profile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(profile)?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn append_history(profile_path: &Path, revision: &str) -> Result<()> {
    let history = profile_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("project-context/history/log.md");
    if history.exists() {
        let mut content = fs::read_to_string(&history)?;
        content.push_str(&format!(
            "\n## Validated {revision}\n\n- Repository checkpoint advanced to `{revision}`.\n"
        ));
        fs::write(history, content)?;
    }
    Ok(())
}
