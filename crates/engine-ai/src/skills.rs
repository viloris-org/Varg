//! Varg skill discovery and scoped skill reading.
//!
//! Project skills live under `<project>/.varg/skills`; user-global skills live
//! under `~/.varg/skills`. Project skills shadow global skills with the same
//! name during search, but resolved IDs keep the source explicit.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::ToolDefinition;

/// Source location for a resolved Varg skill.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Skill stored in the active project at `.varg/skills`.
    Project,
    /// Skill stored in the user's global Varg home at `~/.varg/skills`.
    Global,
}

impl SkillSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }

    fn from_id_prefix(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "global" => Some(Self::Global),
            _ => None,
        }
    }
}

/// Filesystem roots used for Varg skill discovery.
#[derive(Clone, Debug)]
pub struct SkillRegistryConfig {
    /// Active project root.
    pub project_root: PathBuf,
    /// User-global Varg home. Defaults to `~/.varg` in session code.
    pub global_varg_root: PathBuf,
}

impl SkillRegistryConfig {
    /// Creates a registry config from explicit roots.
    pub fn new(project_root: impl Into<PathBuf>, global_varg_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            global_varg_root: global_varg_root.into(),
        }
    }

    fn source_root(&self, source: SkillSource) -> PathBuf {
        match source {
            SkillSource::Project => self.project_root.join(".varg/skills"),
            SkillSource::Global => self.global_varg_root.join("skills"),
        }
    }
}

/// Skill search request.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SkillSearchQuery {
    /// Natural-language search text.
    pub query: String,
    /// Optional source filter: `project` or `global`.
    #[serde(default)]
    pub source: Option<String>,
    /// Maximum result count.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Skill search result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillSearchResult {
    /// Resolved skill ID, such as `project://skills/combat`.
    pub id: String,
    /// Skill directory name.
    pub name: String,
    /// Skill source.
    pub source: SkillSource,
    /// Display path relative to project root or global Varg root.
    pub path: String,
    /// Short description extracted from `SKILL.md`.
    pub description: String,
    /// Whether this skill shadows a lower-priority skill with the same name.
    pub shadows: bool,
    /// Lightweight relevance score.
    pub score: u32,
}

/// Skill read request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillReadRequest {
    /// Resolved skill ID from search, such as `project://skills/combat`.
    pub id: String,
    /// Optional path inside the skill directory. Defaults to `SKILL.md`.
    #[serde(default)]
    pub path: Option<String>,
}

/// Skill read result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillReadResult {
    /// Resolved skill ID.
    pub id: String,
    /// Skill source.
    pub source: SkillSource,
    /// File path relative to the skill directory.
    pub path: String,
    /// UTF-8 file content.
    pub content: String,
}

#[derive(Clone, Debug)]
struct SkillRecord {
    id: String,
    name: String,
    source: SkillSource,
    display_path: String,
    description: String,
    shadows: bool,
}

/// Returns the direct skill search tool definition.
pub fn skill_search_definition() -> ToolDefinition {
    ToolDefinition {
        name: "skill_search".into(),
        description:
            "Search Varg project skills in .varg/skills and user-global skills in ~/.varg/skills."
                .into(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "query": { "type": "string", "description": "Natural-language search text" },
                "source": { "type": "string", "description": "Optional source filter: project or global" },
                "limit": { "type": "integer", "description": "Maximum number of results" }
            },
            "required": ["query"]
        }),
    }
}

/// Returns the direct skill read tool definition.
pub fn skill_read_definition() -> ToolDefinition {
    ToolDefinition {
        name: "skill_read".into(),
        description: "Read a Varg skill file by resolved id. Defaults to SKILL.md; references must stay inside the skill directory.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "id": { "type": "string", "description": "Resolved skill id from skill_search, e.g. project://skills/varg-modeling" },
                "path": { "type": "string", "description": "Optional path inside the skill directory, e.g. references/primitives.md" }
            },
            "required": ["id"]
        }),
    }
}

/// Searches project and global Varg skills.
pub fn search_skills(
    config: &SkillRegistryConfig,
    query: &SkillSearchQuery,
) -> EngineResult<Vec<SkillSearchResult>> {
    let source_filter = match query.source.as_deref() {
        Some(value) => Some(parse_source(value)?),
        None => None,
    };
    let terms = tokenize(&query.query);
    let limit = query.limit.unwrap_or(8).clamp(1, 32);
    let mut scored = Vec::new();

    for record in discover_skills(config)? {
        if let Some(source) = source_filter
            && record.source != source
        {
            continue;
        }

        let search_text = format!("{} {}", record.name, record.description).to_lowercase();
        let mut score = score_terms(&search_text, &terms);
        if terms.is_empty() {
            score += 1;
        }
        if score == 0 {
            continue;
        }
        scored.push((
            score,
            SkillSearchResult {
                id: record.id,
                name: record.name,
                source: record.source,
                path: record.display_path,
                description: record.description,
                shadows: record.shadows,
                score,
            },
        ));
    }

    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(scored
        .into_iter()
        .take(limit)
        .map(|(_, result)| result)
        .collect())
}

/// Reads `SKILL.md` or a referenced file from a resolved Varg skill.
pub fn read_skill(
    config: &SkillRegistryConfig,
    request: &SkillReadRequest,
) -> EngineResult<SkillReadResult> {
    let (source, name) = parse_skill_id(&request.id)?;
    let skill_root = config.source_root(source).join(&name);
    let relative = request.path.as_deref().unwrap_or("SKILL.md");
    let safe_relative = validate_skill_relative_path(relative)?;
    let full_path = skill_root.join(&safe_relative);

    if !full_path.starts_with(&skill_root) {
        return Err(EngineError::config("skill path escapes skill directory"));
    }

    let content = fs::read_to_string(&full_path).map_err(|source| EngineError::Filesystem {
        path: full_path.clone(),
        source,
    })?;

    Ok(SkillReadResult {
        id: request.id.clone(),
        source,
        path: safe_relative.to_string_lossy().replace('\\', "/"),
        content,
    })
}

fn discover_skills(config: &SkillRegistryConfig) -> EngineResult<Vec<SkillRecord>> {
    let mut records_by_name: BTreeMap<String, Vec<SkillRecord>> = BTreeMap::new();
    for source in [SkillSource::Project, SkillSource::Global] {
        for record in scan_skill_root(config, source)? {
            records_by_name
                .entry(record.name.clone())
                .or_default()
                .push(record);
        }
    }

    let mut records = Vec::new();
    for mut group in records_by_name.into_values() {
        group.sort_by_key(|record| record.source);
        let has_project = group
            .iter()
            .any(|record| record.source == SkillSource::Project);
        for mut record in group {
            record.shadows = record.source == SkillSource::Project && has_project;
            records.push(record);
        }
    }
    Ok(records)
}

fn scan_skill_root(
    config: &SkillRegistryConfig,
    source: SkillSource,
) -> EngineResult<Vec<SkillRecord>> {
    let root = config.source_root(source);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&root).map_err(|source| EngineError::Filesystem {
        path: root.clone(),
        source,
    })?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| EngineError::Filesystem {
            path: root.clone(),
            source,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !is_valid_skill_name(name) || !path.join("SKILL.md").is_file() {
            continue;
        }
        let description = extract_description(&path.join("SKILL.md"))?;
        let display_path = match source {
            SkillSource::Project => format!(".varg/skills/{name}/SKILL.md"),
            SkillSource::Global => format!("skills/{name}/SKILL.md"),
        };
        records.push(SkillRecord {
            id: format!("{}://skills/{name}", source.as_str()),
            name: name.to_owned(),
            source,
            display_path,
            description,
            shadows: false,
        });
    }
    Ok(records)
}

fn extract_description(path: &Path) -> EngineResult<String> {
    let content = fs::read_to_string(path).map_err(|source| EngineError::Filesystem {
        path: path.to_path_buf(),
        source,
    })?;
    let mut heading = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if heading.is_none() && trimmed.starts_with('#') {
            heading = Some(trimmed.trim_start_matches('#').trim().to_owned());
            continue;
        }
        if !trimmed.starts_with('#') {
            return Ok(trimmed.to_owned());
        }
    }
    Ok(heading.unwrap_or_else(|| "Varg skill".to_owned()))
}

fn parse_skill_id(id: &str) -> EngineResult<(SkillSource, String)> {
    let (source, rest) = id
        .split_once("://skills/")
        .ok_or_else(|| EngineError::config("skill id must look like project://skills/name"))?;
    let source = SkillSource::from_id_prefix(source)
        .ok_or_else(|| EngineError::config(format!("unknown skill source: {source}")))?;
    if !is_valid_skill_name(rest) {
        return Err(EngineError::config(format!("invalid skill name: {rest}")));
    }
    Ok((source, rest.to_owned()))
}

fn parse_source(value: &str) -> EngineResult<SkillSource> {
    match normalize(value).as_str() {
        "project" => Ok(SkillSource::Project),
        "global" => Ok(SkillSource::Global),
        other => Err(EngineError::config(format!(
            "unknown skill source: {other}"
        ))),
    }
}

fn validate_skill_relative_path(value: &str) -> EngineResult<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(EngineError::config("skill read path must be relative"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(EngineError::config("invalid skill read path")),
        }
    }
    Ok(path)
}

fn is_valid_skill_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn score_terms(search_text: &str, terms: &[String]) -> u32 {
    terms
        .iter()
        .map(|term| {
            if search_text.contains(term) {
                if search_text
                    .split_whitespace()
                    .any(|candidate| candidate == term)
                {
                    3
                } else {
                    1
                }
            } else {
                0
            }
        })
        .sum()
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .map(normalize)
        .filter(|term| !term.is_empty())
        .collect()
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("varg-skill-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_skill(root: &Path, source_path: &str, name: &str, body: &str) {
        let dir = root.join(source_path).join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn project_skills_are_found_before_global_skills() {
        let project = temp_root("project-first-project");
        let global = temp_root("project-first-global");
        write_skill(
            &project,
            ".varg/skills",
            "combat",
            "# Combat\nProject combo authoring rules.",
        );
        write_skill(
            &global,
            "skills",
            "combat",
            "# Combat\nGlobal combat defaults.",
        );
        let config = SkillRegistryConfig::new(&project, &global);

        let results = search_skills(
            &config,
            &SkillSearchQuery {
                query: "combat".into(),
                ..SkillSearchQuery::default()
            },
        )
        .unwrap();

        assert_eq!(results[0].id, "project://skills/combat");
        assert_eq!(results[1].id, "global://skills/combat");
        assert!(results[0].shadows);
        let _ = fs::remove_dir_all(project);
        let _ = fs::remove_dir_all(global);
    }

    #[test]
    fn skill_read_defaults_to_skill_markdown_and_blocks_escape() {
        let project = temp_root("read-project");
        let global = temp_root("read-global");
        write_skill(
            &project,
            ".varg/skills",
            "style",
            "# Style\nUse quiet scene naming.",
        );
        fs::create_dir_all(project.join(".varg/skills/style/references")).unwrap();
        fs::write(
            project.join(".varg/skills/style/references/names.md"),
            "Name things plainly.",
        )
        .unwrap();
        let config = SkillRegistryConfig::new(&project, &global);

        let result = read_skill(
            &config,
            &SkillReadRequest {
                id: "project://skills/style".into(),
                path: None,
            },
        )
        .unwrap();
        assert!(result.content.contains("quiet scene naming"));

        let reference = read_skill(
            &config,
            &SkillReadRequest {
                id: "project://skills/style".into(),
                path: Some("references/names.md".into()),
            },
        )
        .unwrap();
        assert_eq!(reference.content, "Name things plainly.");

        let err = read_skill(
            &config,
            &SkillReadRequest {
                id: "project://skills/style".into(),
                path: Some("../secret.md".into()),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid skill read path"));
        let _ = fs::remove_dir_all(project);
        let _ = fs::remove_dir_all(global);
    }
}
