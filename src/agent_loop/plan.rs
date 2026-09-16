use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Execution status of an individual plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl StepStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            StepStatus::Pending => "[ ]",
            StepStatus::InProgress => "[▶]",
            StepStatus::Completed => "[✓]",
            StepStatus::Failed => "[✗]",
            StepStatus::Skipped => "[-]",
        }
    }
}

fn default_pending() -> StepStatus {
    StepStatus::Pending
}

/// An individual step within an ordered execution plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    pub id: usize,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_preview: Option<serde_json::Value>,
    /// Recorded for inspection and future topological enforcement;
    /// Spec A tracks steps as an ordered list.
    #[serde(default)]
    pub depends_on: Vec<usize>,
    #[serde(default = "default_pending")]
    pub status: StepStatus,
}

/// A structured plan emitted by the model via `_plan`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredPlan {
    pub objective: String,
    pub steps: Vec<PlanStep>,
}

/// Legacy thought object emitted when the model populates `{"thought": "..."}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyThoughtObject {
    pub thought: String,
}

/// Untagged enum attempting structured deserialization before fallback arms.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawPlanPayload {
    Structured(StructuredPlan),
    LegacyThought(LegacyThoughtObject),
    LegacyString(String),
    LegacyCatchAll(serde_json::Value),
}

/// Strongly-typed plan payload representing either a validated structured plan
/// or a legacy free-text scratchpad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanPayload {
    Structured(StructuredPlan),
    Legacy(String),
}

impl PlanPayload {
    /// Deserializes JSON input with guaranteed fallback to Legacy mode.
    /// Never returns an Err; invalid or malformed shapes degrade gracefully (Fix 1 & Fix 5).
    pub fn parse_flexible(raw: serde_json::Value) -> Self {
        if let Ok(raw_payload) = serde_json::from_value::<RawPlanPayload>(raw.clone()) {
            match raw_payload {
                RawPlanPayload::Structured(plan) => {
                    if !plan.objective.trim().is_empty()
                        && !plan.steps.is_empty()
                        && Self::validate_steps(&plan.steps)
                    {
                        PlanPayload::Structured(plan)
                    } else {
                        // Malformed structured payload degrades to legacy string (Fix 5)
                        if let Some(thought) = raw.get("thought").and_then(|t| t.as_str()) {
                            PlanPayload::Legacy(thought.to_string())
                        } else {
                            PlanPayload::Legacy(raw.to_string())
                        }
                    }
                }
                RawPlanPayload::LegacyThought(obj) => PlanPayload::Legacy(obj.thought),
                RawPlanPayload::LegacyString(s) => PlanPayload::Legacy(s),
                RawPlanPayload::LegacyCatchAll(val) => {
                    if let Some(thought) = val.get("thought").and_then(|t| t.as_str()) {
                        PlanPayload::Legacy(thought.to_string())
                    } else {
                        PlanPayload::Legacy(val.to_string())
                    }
                }
            }
        } else {
            PlanPayload::Legacy(raw.to_string())
        }
    }

    fn validate_steps(steps: &[PlanStep]) -> bool {
        let mut seen_ids = HashSet::new();
        for step in steps {
            if step.id == 0 || step.intent.trim().is_empty() || !seen_ids.insert(step.id) {
                return false;
            }
        }
        true
    }
}

/// Tracks the live execution state of an ordered plan during the agent loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTracker {
    pub objective: String,
    pub steps: Vec<PlanStep>,
    pub active_step_id: Option<usize>,
}

impl PlanTracker {
    pub fn new(plan: StructuredPlan) -> Self {
        Self {
            objective: plan.objective,
            steps: plan.steps,
            active_step_id: None,
        }
    }

    /// Mark step as InProgress matching the invoked tool, or the next pending step.
    pub fn update_active_step(&mut self, tool_name: &str) -> Option<usize> {
        // If the currently active step already matches this tool, keep it
        if let Some(active_id) = self.active_step_id {
            if let Some(step) = self.steps.iter().find(|s| s.id == active_id) {
                if step.tool.as_deref() == Some(tool_name) {
                    return Some(active_id);
                }
            }
        }
        // Next, find first pending step that matches this tool
        if let Some(step) = self
            .steps
            .iter_mut()
            .find(|s| s.status == StepStatus::Pending && s.tool.as_deref() == Some(tool_name))
        {
            step.status = StepStatus::InProgress;
            let id = step.id;
            self.active_step_id = Some(id);
            return Some(id);
        }
        // Otherwise, advance first pending step
        if let Some(step) = self.steps.iter_mut().find(|s| s.status == StepStatus::Pending) {
            step.status = StepStatus::InProgress;
            let id = step.id;
            self.active_step_id = Some(id);
            return Some(id);
        }
        None
    }

    pub fn complete_active_step(&mut self) -> Option<usize> {
        if let Some(active_id) = self.active_step_id {
            if let Some(step) = self.steps.iter_mut().find(|s| s.id == active_id) {
                step.status = StepStatus::Completed;
                self.active_step_id = None;
                return Some(active_id);
            }
        }
        None
    }

    pub fn fail_active_step(&mut self) -> Option<usize> {
        if let Some(active_id) = self.active_step_id {
            if let Some(step) = self.steps.iter_mut().find(|s| s.id == active_id) {
                step.status = StepStatus::Failed;
                self.active_step_id = None;
                return Some(active_id);
            }
        }
        None
    }

    #[allow(dead_code)]
    pub fn render_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("objective: \"{}\"", self.objective));
        for step in &self.steps {
            let tool_suffix = match &step.tool {
                Some(t) => format!(" ({t})"),
                None => String::new(),
            };
            lines.push(format!(
                "  {} {}. {}{}",
                step.status.symbol(),
                step.id,
                step.intent,
                tool_suffix
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plan_payload_deserialization_and_degradation() {
        // 1. Structured payload
        let valid_json = json!({
            "objective": "Triage cluster issue",
            "steps": [
                { "id": 1, "intent": "Check pods", "tool": "kubectl" },
                { "id": 2, "intent": "Examine logs", "tool": "kubectl" }
            ]
        });
        match PlanPayload::parse_flexible(valid_json) {
            PlanPayload::Structured(plan) => {
                assert_eq!(plan.objective, "Triage cluster issue");
                assert_eq!(plan.steps.len(), 2);
                assert_eq!(plan.steps[0].tool.as_deref(), Some("kubectl"));
            }
            PlanPayload::Legacy(_) => panic!("Expected structured plan"),
        }

        // 2. Legacy thought object (Fix 1: model emitted schema-advertised thought field)
        let thought_json = json!({ "thought": "I will examine logs first" });
        assert_eq!(
            PlanPayload::parse_flexible(thought_json),
            PlanPayload::Legacy("I will examine logs first".to_string())
        );

        // 3. Raw string
        let raw_string = json!("Think step by step");
        assert_eq!(
            PlanPayload::parse_flexible(raw_string),
            PlanPayload::Legacy("Think step by step".to_string())
        );

        // 4. Unknown arbitrary object shape
        let unknown = json!({ "custom_field": "some notes" });
        match PlanPayload::parse_flexible(unknown) {
            PlanPayload::Legacy(s) => assert!(s.contains("custom_field")),
            PlanPayload::Structured(_) => panic!("Expected legacy degradation"),
        }
    }

    #[test]
    fn test_malformed_plan_degradation() {
        // Fix 5: Malformed structured plan must degrade to legacy scratchpad without error

        // Case A: Duplicate IDs
        let duplicate_ids = json!({
            "objective": "Duplicate test",
            "steps": [
                { "id": 1, "intent": "Step 1" },
                { "id": 1, "intent": "Step 2" }
            ]
        });
        match PlanPayload::parse_flexible(duplicate_ids) {
            PlanPayload::Legacy(s) => assert!(s.contains("Duplicate test")),
            PlanPayload::Structured(_) => panic!("Expected degradation due to duplicate IDs"),
        }

        // Case B: Step ID 0
        let zero_id = json!({
            "objective": "Zero id test",
            "steps": [
                { "id": 0, "intent": "Invalid step" }
            ]
        });
        assert!(matches!(
            PlanPayload::parse_flexible(zero_id),
            PlanPayload::Legacy(_)
        ));

        // Case C: Empty objective
        let empty_obj = json!({
            "objective": "   ",
            "steps": [
                { "id": 1, "intent": "Valid step" }
            ]
        });
        assert!(matches!(
            PlanPayload::parse_flexible(empty_obj),
            PlanPayload::Legacy(_)
        ));

        // Case D: Empty steps array
        let empty_steps = json!({
            "objective": "Empty steps",
            "steps": []
        });
        assert!(matches!(
            PlanPayload::parse_flexible(empty_steps),
            PlanPayload::Legacy(_)
        ));

        // Case E: Empty intent
        let empty_intent = json!({
            "objective": "Empty intent",
            "steps": [
                { "id": 1, "intent": "  " }
            ]
        });
        assert!(matches!(
            PlanPayload::parse_flexible(empty_intent),
            PlanPayload::Legacy(_)
        ));
    }

    #[test]
    fn test_step_list_progress() {
        let plan = StructuredPlan {
            objective: "Database Migration".to_string(),
            steps: vec![
                PlanStep {
                    id: 1,
                    intent: "Backup accounts table".to_string(),
                    tool: Some("pg_dump".to_string()),
                    args_preview: None,
                    depends_on: vec![],
                    status: StepStatus::Pending,
                },
                PlanStep {
                    id: 2,
                    intent: "Apply alter table migration".to_string(),
                    tool: Some("sql".to_string()),
                    args_preview: None,
                    depends_on: vec![1],
                    status: StepStatus::Pending,
                },
                PlanStep {
                    id: 3,
                    intent: "Verify index status".to_string(),
                    tool: Some("sql".to_string()),
                    args_preview: None,
                    depends_on: vec![2],
                    status: StepStatus::Pending,
                },
            ],
        };

        let mut tracker = PlanTracker::new(plan);
        assert_eq!(tracker.active_step_id, None);

        // Step 1: Tool pg_dump executes
        let act1 = tracker.update_active_step("pg_dump");
        assert_eq!(act1, Some(1));
        assert_eq!(tracker.steps[0].status, StepStatus::InProgress);

        // Step 1 completes
        let done1 = tracker.complete_active_step();
        assert_eq!(done1, Some(1));
        assert_eq!(tracker.steps[0].status, StepStatus::Completed);
        assert_eq!(tracker.active_step_id, None);

        // Step 2: Tool sql executes
        let act2 = tracker.update_active_step("sql");
        assert_eq!(act2, Some(2));
        assert_eq!(tracker.steps[1].status, StepStatus::InProgress);

        // Step 2 fails
        let fail2 = tracker.fail_active_step();
        assert_eq!(fail2, Some(2));
        assert_eq!(tracker.steps[1].status, StepStatus::Failed);
        assert_eq!(tracker.active_step_id, None);

        // Step 3 advances
        let act3 = tracker.update_active_step("sql");
        assert_eq!(act3, Some(3));
        assert_eq!(tracker.steps[2].status, StepStatus::InProgress);

        let summary = tracker.render_summary();
        assert!(summary.contains("[✓] 1. Backup accounts table (pg_dump)"));
        assert!(summary.contains("[✗] 2. Apply alter table migration (sql)"));
        assert!(summary.contains("[▶] 3. Verify index status (sql)"));
    }
}
