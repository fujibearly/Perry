use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{Config, GlobalConfig, SkillSetting};
use crate::function::{BlastRadius, FunctionDeclaration, JsonSchema, ToolMode};

/// Provenance of a skill runbook (Backlog #17, Spec B).
///
/// Invariant 3: Provenance-based taint. Skills discovered in workspace directories
/// (.kiro/skills/, .agents/skills/, .skills/) are marked WorkspaceTainted.
/// Skills in user global config or builtin directories are trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillProvenance {
    Builtin,
    Global,
    WorkspaceTainted,
}

impl SkillProvenance {
    pub fn is_tainted(&self) -> bool {
        matches!(self, SkillProvenance::WorkspaceTainted)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SkillProvenance::Builtin => "builtin",
            SkillProvenance::Global => "global",
            SkillProvenance::WorkspaceTainted => "workspace",
        }
    }
}

/// Metadata parsed from the frontmatter of a SKILL.md document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<SkillCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCompatibility {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

/// A parsed skill runbook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub instructions: String,
    pub provenance: SkillProvenance,
    pub path: PathBuf,
}

impl Skill {
    pub fn from_content(
        name_hint: &str,
        content: &str,
        provenance: SkillProvenance,
        path: PathBuf,
    ) -> Result<Self> {
        if let Some((frontmatter, instructions)) = split_front_matter(content) {
            let mut metadata: SkillMetadata = serde_yaml::from_str(frontmatter).unwrap_or_else(|_| {
                SkillMetadata {
                    name: name_hint.to_string(),
                    description: String::new(),
                    compatibility: None,
                    allowed_tools: None,
                }
            });
            if metadata.name.is_empty() {
                metadata.name = name_hint.to_string();
            }
            Ok(Skill {
                metadata,
                instructions: instructions.to_string(),
                provenance,
                path,
            })
        } else {
            // Plain markdown without frontmatter
            Ok(Skill {
                metadata: SkillMetadata {
                    name: name_hint.to_string(),
                    description: content.lines().next().unwrap_or("").trim().to_string(),
                    compatibility: None,
                    allowed_tools: None,
                },
                instructions: content.trim().to_string(),
                provenance,
                path,
            })
        }
    }

    pub fn from_file(path: &Path, provenance: SkillProvenance) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read skill file at '{}'", path.display()))?;
        let parent_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        Self::from_content(parent_name, &content, provenance, path.to_path_buf())
    }
}

fn split_front_matter(content: &str) -> Option<(&str, &str)> {
    let mut lines = content.split_inclusive('\n');
    let opening = lines.next()?;
    if opening.trim() != "---" {
        return None;
    }
    let metadata_start = opening.len();
    let mut current_offset = metadata_start;
    for line in lines {
        if line.trim() == "---" {
            let metadata = &content[metadata_start..current_offset];
            let instructions = &content[current_offset + line.len()..];
            return Some((metadata.trim(), instructions.trim()));
        }
        current_offset += line.len();
    }
    None
}

/// Registry of available skills, discovered across workspace, global, and builtin roots.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: IndexMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: IndexMap::new(),
        }
    }

    /// Discovers skills across all discovery roots following strict precedence:
    /// Workspace > Global > Builtin.
    pub fn discover(
        workspace_dir: Option<&Path>,
        global_dir: Option<&Path>,
        builtin_dir: Option<&Path>,
    ) -> Self {
        let mut registry = Self::new();

        // 1. Built-in (lowest precedence)
        if let Some(dir) = builtin_dir {
            registry.load_dir(dir, SkillProvenance::Builtin);
        }

        // 2. Global (overrides built-in)
        if let Some(dir) = global_dir {
            registry.load_dir(dir, SkillProvenance::Global);
        }

        // 3. Workspace (highest precedence, overrides global and builtin; tainted)
        if let Some(ws) = workspace_dir {
            for sub in &[".kiro/skills", ".agents/skills", ".skills"] {
                let ws_skills = ws.join(sub);
                if ws_skills.is_dir() {
                    registry.load_dir(&ws_skills, SkillProvenance::WorkspaceTainted);
                }
            }
        }

        registry
    }

    pub fn discover_default(config: &GlobalConfig) -> Self {
        let workspace_dir = std::env::var("AICHAT_WORKSPACE_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());

        let global_dir = Config::skills_dir();
        let builtin_dir = std::env::var("AICHAT_BUILTIN_SKILLS_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                let p = Config::config_dir().join("builtin").join("skills");
                if p.is_dir() {
                    Some(p)
                } else {
                    None
                }
            });

        let mut reg = Self::discover(
            workspace_dir.as_deref(),
            Some(&global_dir),
            builtin_dir.as_deref(),
        );

        // Also check agent-specific skills directory if an agent is active
        if let Some(agent) = config.read().agent.as_ref() {
            let agent_skills_dir = Config::agent_functions_dir(agent.name()).join("skills");
            if agent_skills_dir.is_dir() {
                reg.load_dir(&agent_skills_dir, SkillProvenance::Global);
            }
        }

        reg
    }

    pub fn load_dir(&mut self, dir: &Path, provenance: SkillProvenance) {
        if !dir.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut subdirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            subdirs.sort();
            for path in subdirs {
                if path.is_dir() {
                    let skill_file = path.join("SKILL.md");
                    if skill_file.is_file() {
                        if let Ok(skill) = Skill::from_file(&skill_file, provenance) {
                            self.skills.insert(skill.metadata.name.clone(), skill);
                        }
                    }
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    #[allow(dead_code)]
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    pub fn filter_eligible(&self, setting: &SkillSetting, is_nano: bool) -> Vec<&Skill> {
        if is_nano || !setting.is_enabled() {
            return Vec::new();
        }
        self.skills
            .values()
            .filter(|s| setting.allows(&s.metadata.name))
            .collect()
    }

    pub fn format_prompt_catalogue(&self, eligible_skills: &[&Skill]) -> Option<String> {
        if eligible_skills.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        lines.push("### Available Skills".to_string());
        lines.push("The following skills provide specialized instructions for complex workflows. Call `read_skill` with the skill name to read instructions before proceeding.".to_string());
        for skill in eligible_skills {
            lines.push(format!(
                "- {}: {}",
                skill.metadata.name, skill.metadata.description
            ));
        }
        Some(lines.join("\n"))
    }
}

/// Helper function to retrieve eligible skills for an agent configuration.
pub fn get_eligible_skills_for_config(config: &Config) -> Vec<Skill> {
    let workspace_dir = std::env::var("AICHAT_WORKSPACE_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());

    let global_dir = Config::skills_dir();
    let builtin_dir = std::env::var("AICHAT_BUILTIN_SKILLS_DIR")
        .ok()
        .map(PathBuf::from);

    let reg = SkillRegistry::discover(
        workspace_dir.as_deref(),
        Some(&global_dir),
        builtin_dir.as_deref(),
    );

    let (setting, is_nano) = if let Some(agent) = &config.agent {
        (agent.skills_setting(), agent.is_nano())
    } else {
        (SkillSetting::Bool(true), false)
    };

    reg.filter_eligible(&setting, is_nano)
        .into_iter()
        .cloned()
        .collect()
}

/// Tracks concurrently active loaded skills and their provenance-based taint.
///
/// Invariant 3 & Review Fix 1a:
/// Tracks a set of loaded-and-not-yet-consumed skills. `untrusted_runbook` is true
/// while any tainted skill is active in that set; taint clears when the skill is
/// marked consumed or the associated plan step completes.
#[derive(Debug, Clone, Default)]
pub struct ActiveSkillTracker {
    loaded_skills: HashMap<String, SkillProvenance>,
    step_skills: HashMap<usize, HashSet<String>>,
    active_step: Option<usize>,
}

impl ActiveSkillTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_active_step(&mut self, step_id: Option<usize>) {
        self.active_step = step_id;
    }

    #[allow(dead_code)]
    pub fn active_step(&self) -> Option<usize> {
        self.active_step
    }

    pub fn load(&mut self, name: &str, provenance: SkillProvenance) {
        self.loaded_skills.insert(name.to_string(), provenance);
        if let Some(step_id) = self.active_step {
            self.step_skills
                .entry(step_id)
                .or_default()
                .insert(name.to_string());
        }
    }

    #[allow(dead_code)]
    pub fn consume(&mut self, name: &str) {
        self.loaded_skills.remove(name);
        for skills in self.step_skills.values_mut() {
            skills.remove(name);
        }
    }

    pub fn complete_step(&mut self, step_id: usize) {
        if let Some(skills) = self.step_skills.remove(&step_id) {
            for skill_name in skills {
                self.loaded_skills.remove(&skill_name);
            }
        }
    }

    pub fn is_untrusted(&self) -> bool {
        self.loaded_skills.values().any(|p| p.is_tainted())
    }

    pub fn active_tainted_skills(&self) -> Vec<String> {
        let mut tainted: Vec<String> = self
            .loaded_skills
            .iter()
            .filter(|(_, p)| p.is_tainted())
            .map(|(k, _)| k.clone())
            .collect();
        tainted.sort();
        tainted
    }
}

/// Tool declaration for `read_skill`.
pub fn read_skill_tool_declaration() -> FunctionDeclaration {
    let mut props = IndexMap::new();
    props.insert(
        "name".to_string(),
        JsonSchema {
            type_value: Some("string".to_string()),
            description: Some(
                "The exact name of the skill to read from the Available Skills catalogue."
                    .to_string(),
            ),
            ..Default::default()
        },
    );

    FunctionDeclaration {
        name: "read_skill".to_string(),
        description: "Read instructions for an available skill runbook. Call this before performing tasks covered by a specialized skill.".to_string(),
        parameters: JsonSchema {
            type_value: Some("object".to_string()),
            properties: Some(props),
            required: Some(vec!["name".to_string()]),
            ..Default::default()
        },
        agent: false,
        output: None,
        // Plain read with no state mutation
        mode: Some(ToolMode::Readonly),
        risk: Some(BlastRadius::Safe),
        reversible: Some(true),
        reversible_via: None,
        nano: None,
    }
}

/// Evaluates a `read_skill` tool call.
///
/// Plain filesystem read with zero SHA-256 integrity gate (Review Fix 5).
pub fn eval_read_skill(config: &GlobalConfig, call: &crate::function::ToolCall) -> Value {
    let skill_name = call
        .arguments
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| call.arguments.as_str());

    let name = match skill_name {
        Some(n) if !n.trim().is_empty() => n.trim(),
        _ => {
            return json!({
                "error": "Missing or empty 'name' argument for read_skill"
            });
        }
    };

    let registry = SkillRegistry::discover_default(config);
    match registry.get(name) {
        Some(skill) => {
            json!({
                "name": skill.metadata.name,
                "description": skill.metadata.description,
                "instructions": skill.instructions,
                "provenance": skill.provenance.as_str(),
                "allowed_tools": skill.metadata.allowed_tools,
            })
        }
        None => {
            json!({
                "error": format!("Skill '{name}' not found in registry")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_frontmatter_parsing() {
        let content = r#"---
name: deploy-preview
description: Deploy preview environments
compatibility:
  os: [linux]
  tools: [docker, git]
allowed_tools: [docker]
---
# Deploy Preview Runbook

Run `docker compose up -d` to create preview.
"#;
        let skill = Skill::from_content(
            "fallback",
            content,
            SkillProvenance::Global,
            PathBuf::from("/skills/deploy-preview/SKILL.md"),
        )
        .unwrap();

        assert_eq!(skill.metadata.name, "deploy-preview");
        assert_eq!(skill.metadata.description, "Deploy preview environments");
        assert_eq!(
            skill.metadata.compatibility.as_ref().unwrap().os,
            vec!["linux"]
        );
        assert_eq!(
            skill.metadata.allowed_tools.as_ref().unwrap(),
            &vec!["docker".to_string()]
        );
        assert!(skill.instructions.contains("Deploy Preview Runbook"));
        assert_eq!(skill.provenance, SkillProvenance::Global);
        assert!(!skill.provenance.is_tainted());
    }

    #[test]
    fn test_skill_precedence_and_taint() {
        let temp = crate::utils::temp_file("-test-skills-", "");
        let ws_dir = temp.join("workspace");
        let global_dir = temp.join("global");
        let builtin_dir = temp.join("builtin");

        // Create builtin skill: triage
        let builtin_triage = builtin_dir.join("triage");
        std::fs::create_dir_all(&builtin_triage).unwrap();
        std::fs::write(
            builtin_triage.join("SKILL.md"),
            "---\nname: triage\ndescription: Builtin triage\n---\nBuiltin triage instructions",
        )
        .unwrap();

        // Create global skill: triage (overrides builtin) and release
        let global_triage = global_dir.join("triage");
        std::fs::create_dir_all(&global_triage).unwrap();
        std::fs::write(
            global_triage.join("SKILL.md"),
            "---\nname: triage\ndescription: Global triage\n---\nGlobal triage instructions",
        )
        .unwrap();

        let global_release = global_dir.join("release");
        std::fs::create_dir_all(&global_release).unwrap();
        std::fs::write(
            global_release.join("SKILL.md"),
            "---\nname: release\ndescription: Global release\n---\nGlobal release instructions",
        )
        .unwrap();

        // Create workspace skill: triage (overrides global and builtin, tainted)
        let ws_triage = ws_dir.join(".kiro").join("skills").join("triage");
        std::fs::create_dir_all(&ws_triage).unwrap();
        std::fs::write(
            ws_triage.join("SKILL.md"),
            "---\nname: triage\ndescription: Workspace triage\n---\nWorkspace triage instructions",
        )
        .unwrap();

        let registry = SkillRegistry::discover(
            Some(&ws_dir),
            Some(&global_dir),
            Some(&builtin_dir),
        );

        let triage = registry.get("triage").unwrap();
        assert_eq!(triage.metadata.description, "Workspace triage");
        assert_eq!(triage.provenance, SkillProvenance::WorkspaceTainted);
        assert!(triage.provenance.is_tainted());

        let release = registry.get("release").unwrap();
        assert_eq!(release.metadata.description, "Global release");
        assert_eq!(release.provenance, SkillProvenance::Global);
        assert!(!release.provenance.is_tainted());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_eligibility_gating_and_nano_exclusion() {
        let mut registry = SkillRegistry::new();
        registry.skills.insert(
            "triage".to_string(),
            Skill::from_content(
                "triage",
                "---\nname: triage\ndescription: Triage issues\n---\nInstructions",
                SkillProvenance::Global,
                PathBuf::new(),
            )
            .unwrap(),
        );
        registry.skills.insert(
            "release".to_string(),
            Skill::from_content(
                "release",
                "---\nname: release\ndescription: Release project\n---\nInstructions",
                SkillProvenance::Global,
                PathBuf::new(),
            )
            .unwrap(),
        );

        // All allowed
        let all = registry.filter_eligible(&SkillSetting::Bool(true), false);
        assert_eq!(all.len(), 2);

        // Disabled
        let disabled = registry.filter_eligible(&SkillSetting::Bool(false), false);
        assert!(disabled.is_empty());

        // Allowlist
        let allowlist = registry.filter_eligible(
            &SkillSetting::List(vec!["triage".to_string()]),
            false,
        );
        assert_eq!(allowlist.len(), 1);
        assert_eq!(allowlist[0].metadata.name, "triage");

        // Nano agent exclusion
        let nano = registry.filter_eligible(&SkillSetting::Bool(true), true);
        assert!(nano.is_empty());
    }

    #[test]
    fn test_active_skill_tracker_lifecycle() {
        let mut tracker = ActiveSkillTracker::new();
        assert!(!tracker.is_untrusted());
        assert!(tracker.active_tainted_skills().is_empty());

        // Load trusted skill
        tracker.load("safe-skill", SkillProvenance::Global);
        assert!(!tracker.is_untrusted());

        // Set active step and load tainted skill
        tracker.set_active_step(Some(1));
        tracker.load("tainted-skill", SkillProvenance::WorkspaceTainted);
        assert!(tracker.is_untrusted());
        assert_eq!(tracker.active_tainted_skills(), vec!["tainted-skill".to_string()]);

        // Complete step 1 -> tainted skill consumed, taint clears
        tracker.complete_step(1);
        assert!(!tracker.is_untrusted());
        assert!(tracker.active_tainted_skills().is_empty());

        // Safe skill still present until explicitly consumed
        tracker.consume("safe-skill");
        assert!(tracker.loaded_skills.is_empty());
    }

    #[test]
    fn test_eval_read_skill_output_conformance() {
        let temp = crate::utils::temp_file("-test-skills-read-", "");
        let ws_dir = temp.join("repo");
        let ws_skill_dir = ws_dir.join(".kiro").join("skills").join("deploy");
        std::fs::create_dir_all(&ws_skill_dir).unwrap();
        std::fs::write(
            ws_skill_dir.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy cluster\n---\nRun deploy steps.",
        )
        .unwrap();

        let registry = SkillRegistry::discover(Some(&ws_dir), None, None);
        let skill = registry.get("deploy").expect("skill should be discovered");
        assert_eq!(skill.metadata.name, "deploy");
        assert_eq!(skill.metadata.description, "Deploy cluster");
        assert_eq!(skill.instructions.trim(), "Run deploy steps.");
        assert_eq!(skill.provenance, SkillProvenance::WorkspaceTainted);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_active_skill_tracker_multiple_skills_isolation() {
        let mut tracker = ActiveSkillTracker::new();
        tracker.set_active_step(Some(1));
        tracker.load("ws-skill-1", SkillProvenance::WorkspaceTainted);
        tracker.load("safe-skill", SkillProvenance::Builtin);
        assert!(tracker.is_untrusted());
        assert_eq!(tracker.active_tainted_skills(), vec!["ws-skill-1"]);

        tracker.set_active_step(Some(2));
        tracker.load("ws-skill-2", SkillProvenance::WorkspaceTainted);
        assert!(tracker.is_untrusted());

        // Step 1 completes
        tracker.complete_step(1);
        // ws-skill-2 from step 2 is still active!
        assert!(tracker.is_untrusted());
        assert_eq!(tracker.active_tainted_skills(), vec!["ws-skill-2"]);

        // Step 2 completes -> now all tainted skills are consumed
        tracker.complete_step(2);
        assert!(!tracker.is_untrusted());
        assert!(tracker.active_tainted_skills().is_empty());
    }
}
