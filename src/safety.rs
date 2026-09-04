//! Tool safety modes & actuation governance (backlog #6).
//!
//! This module is the deterministic core of the safety decision funnel:
//!
//! ```text
//! [Protected Policy File]  non-pardonable        ──(forbid)──► BLOCKED (policy_forbidden)
//!    │
//!    ▼
//! blast-radius classify {Safe..Catastrophic} + reversibility PROVEN?
//!    │   required_authority = f(tier, proven_reversible)
//!    ├─ ≤ agent ceiling ─────────────────────────────────────► EXECUTE
//!    ▼ (> ceiling, or unclassified)
//! escalate (#6c evaluator / #6d channel) — or BLOCK before those land
//! ```
//!
//! #6b implements the **deterministic** part: the blast-radius tiers (in
//! [`crate::function::BlastRadius`]), the Protected Policy File, the orthogonal
//! proven-reversibility combination ([`required_authority`]), and the
//! root-favoring authority ceiling. No LLM is involved — the `%assess-risk%`
//! evaluator (#6c) and the escalation channel (#6d) layer on top later.
//!
//! Everything here is pure and deterministic so it can be exhaustively unit
//! tested offline. `agent_loop.rs` calls into it for enforcement.

use crate::function::{BlastRadius, FunctionDeclaration, StaticTier};

use anyhow::{bail, Context, Result};

// ---------------------------------------------------------------------------
// Required authority
// ---------------------------------------------------------------------------

/// The authority an action requires before it may be performed autonomously.
///
/// Compared against an agent's ceiling ([`AuthorityCeiling`]): an action is
/// autonomously executable iff its `RequiredAuthority` is within the ceiling.
/// `Human` sits above every autonomous ceiling — it always escalates to a human
/// (or blocks, before #6d's escalation channel exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredAuthority {
    /// The action requires (at most) authority for this blast-radius tier.
    Tier(BlastRadius),
    /// Reserved to a human — unclassified actions, and anything a policy or the
    /// ceiling configuration pins to the human tier.
    Human,
}

/// The maximum blast-radius tier an agent may actuate **autonomously**.
///
/// Ceilings increase toward the root of the delegation tree (the orchestrator
/// holds the highest), on the grounds that higher agents carry more context.
/// A parent may only *lower* the ceiling it grants a child, never raise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityCeiling {
    /// The agent may autonomously actuate up to and including this tier.
    UpTo(BlastRadius),
}

impl AuthorityCeiling {
    /// The lowest possible ceiling: only `Safe` (read-only) actions.
    pub const MINIMAL: AuthorityCeiling = AuthorityCeiling::UpTo(BlastRadius::Safe);

    /// Whether an action with the given `RequiredAuthority` may be actuated
    /// autonomously under this ceiling. `Human` is never within any ceiling.
    pub fn permits(&self, required: RequiredAuthority) -> bool {
        match required {
            RequiredAuthority::Human => false,
            RequiredAuthority::Tier(t) => {
                let AuthorityCeiling::UpTo(max) = *self;
                t <= max
            }
        }
    }

    /// The tier this ceiling permits up to (for lowering / min operations).
    pub fn tier(&self) -> BlastRadius {
        let AuthorityCeiling::UpTo(t) = *self;
        t
    }

    /// Grant a child a ceiling: a parent may only *lower*, never raise. The
    /// child's effective ceiling is the min of its own and what's granted.
    /// (Reserved for #6d per-agent ceiling grants; #6b passes the parent's
    /// ceiling down unchanged.)
    #[allow(dead_code)]
    pub fn lower_to(self, granted: AuthorityCeiling) -> AuthorityCeiling {
        AuthorityCeiling::UpTo(self.tier().min(granted.tier()))
    }
}

/// One blast-radius tier less dangerous (saturating at `Safe`).
///
/// Proven reversibility lowers the *authority required* by exactly one step —
/// e.g. a `Destructive` action that is provably reversible requires only
/// `Disruptive` authority. It never changes the action's *tier* (impact axis).
fn one_step_down(tier: BlastRadius) -> BlastRadius {
    match tier {
        BlastRadius::Catastrophic => BlastRadius::Destructive,
        BlastRadius::Destructive => BlastRadius::Disruptive,
        BlastRadius::Disruptive => BlastRadius::Reversible,
        BlastRadius::Reversible => BlastRadius::Safe,
        BlastRadius::Safe => BlastRadius::Safe,
    }
}

/// Whether this invocation's action is *proven* reversible (backlog #6b, FR-6b.2).
///
/// Reversibility must be **proven, not asserted**: it counts only when either
/// (a) the tool declares intrinsic reversibility (`reversible: true`), or
/// (b) a real rollback artifact has been registered for this invocation (a
/// backup / staged copy / git worktree — recorded out-of-band, consumed from
/// #9/#10). #6b handles (a) via the declared flag; `artifact_registered`
/// carries (b) once those items exist (always `false` for now).
pub fn proven_reversible(decl: &FunctionDeclaration, artifact_registered: bool) -> bool {
    decl.reversible == Some(true) || artifact_registered
}

/// Compute the authority an action requires, combining the (policy-raised)
/// static blast-radius tier with the orthogonal proven-reversibility axis.
///
/// - **Unclassified** tools (no `risk`/`mode`) → [`RequiredAuthority::Human`]
///   (reserved to humans, for now — sits above every autonomous ceiling).
/// - Otherwise the base tier is the **more dangerous** of the tool's static
///   tier and any policy-imposed floor (`policy_tier`, which can only *raise*).
/// - Proven reversibility then lowers the required authority by one step —
///   except at `Catastrophic`, which is a hard human-only floor that
///   reversibility cannot discount.
///
/// This is the pure heart of #6b. The `#6c` evaluator may later *raise* the base
/// or *withhold* the reversibility credit, never the reverse (stricter-only).
pub fn required_authority(
    static_tier: StaticTier,
    policy_tier: Option<PolicyOutcome>,
    proven_reversible: bool,
) -> RequiredAuthority {
    // A policy `Forbid` is absolute and non-pardonable — it maps to Human here
    // (the caller treats an explicit Forbid distinctly as `policy_forbidden`,
    // but for authority purposes it is at least human-only).
    let policy_floor = match policy_tier {
        Some(PolicyOutcome::Forbid) => return RequiredAuthority::Human,
        Some(PolicyOutcome::Raise(t)) => Some(t),
        None => None,
    };

    let base = match static_tier {
        StaticTier::Unclassified => {
            // Even if a policy raises an unclassified tool to a concrete tier,
            // the absence of any declared classification keeps it human-reserved
            // for now (the most conservative disposition, FR-6b.3).
            return RequiredAuthority::Human;
        }
        StaticTier::Tier(t) => match policy_floor {
            Some(p) => t.max(p), // policy can only raise
            None => t,
        },
    };

    // Proven reversibility lowers the required authority by one step — EXCEPT at
    // `Catastrophic`, which is a hard floor: a catastrophic action is reserved to
    // a human regardless of any reversibility claim. This keeps a policy-imposed
    // `Catastrophic` raise (or an intrinsically-catastrophic tool) from being
    // silently discounted into an autonomous ceiling by a `reversible: true` flag
    // — the flag is set by the tool author and must not be able to undercut the
    // non-pardonable catastrophic floor.
    let effective = if proven_reversible && base != BlastRadius::Catastrophic {
        one_step_down(base)
    } else {
        base
    };
    RequiredAuthority::Tier(effective)
}

// ---------------------------------------------------------------------------
// Protected Policy File (loader + matcher land in 6b.4)
// ---------------------------------------------------------------------------

/// Outcome of matching an action against the Protected Policy File.
///
/// The policy is **non-pardonable** and can only make things *stricter*:
/// it may `Raise` an action's effective tier or `Forbid` it outright. It can
/// never lower a tier or grant permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyOutcome {
    /// Raise the action's effective floor to at least this tier.
    Raise(BlastRadius),
    /// The action is forbidden outright (never actuatable, not even by a human
    /// verdict — the deterministic non-pardonable floor).
    Forbid,
}

/// A single deterministic policy rule.
///
/// Matches when the tool name matches `tool` (glob) AND — if `arg_contains` /
/// `arg_glob` are present — some string-valued argument matches. A matching rule
/// contributes its `raise` tier (or `forbid`) to the action's floor.
///
/// YAML shape:
/// ```yaml
/// - tool: "fs_*"          # glob over tool name (default "*" = any tool)
///   arg_glob: "/etc/**"   # optional: any string arg matches this glob
///   raise: destructive    # OR: forbid: true
/// - tool: "*"
///   arg_contains: "prod"  # optional: any string arg contains this substring
///   forbid: true
/// ```
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PolicyRule {
    /// Glob over the tool name. Absent → `"*"` (matches any tool).
    #[serde(default = "default_tool_glob")]
    pub tool: String,
    /// Optional: match if any string argument matches this glob.
    #[serde(default)]
    pub arg_glob: Option<String>,
    /// Optional: match if any string argument contains this substring.
    #[serde(default)]
    pub arg_contains: Option<String>,
    /// Raise the effective floor to this tier when the rule matches.
    #[serde(default)]
    pub raise: Option<BlastRadius>,
    /// Forbid the action outright when the rule matches (overrides `raise`).
    #[serde(default)]
    pub forbid: bool,
}

fn default_tool_glob() -> String {
    "*".to_string()
}

/// The parsed Protected Policy File: an ordered list of deterministic,
/// non-pardonable rules. Absent file → an empty policy (no extra floors; note
/// that unclassified tools are already human-reserved via [`StaticTier`], so an
/// empty policy is still safe by default).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PolicyFile {
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

impl PolicyFile {
    /// Load and parse the policy file at `path`, enforcing owner-only access.
    ///
    /// The file MUST be owner-readable-only (no group/other permission bits) on
    /// Unix — a world- or group-readable policy is a tampering surface, so it is
    /// **rejected** rather than trusted. On non-Unix the permission check is
    /// skipped (best-effort). A missing file is NOT an error: it yields an empty
    /// policy (safe default).
    pub fn load(path: &std::path::Path) -> Result<PolicyFile> {
        if !path.exists() {
            return Ok(PolicyFile::default());
        }
        enforce_owner_only(path)?;
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read policy file {}", path.display()))?;
        let policy: PolicyFile = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse policy file {}", path.display()))?;
        Ok(policy)
    }

    /// Evaluate this policy against an action, returning the **strictest**
    /// matching outcome, or `None` if no rule matches.
    ///
    /// `Forbid` always wins. Among `Raise` rules, the highest (most dangerous)
    /// tier wins. Rules never lower anything — this is the raise-only floor.
    pub fn evaluate(&self, tool_name: &str, string_args: &[&str]) -> Option<PolicyOutcome> {
        let mut highest_raise: Option<BlastRadius> = None;
        for rule in &self.rules {
            if !rule_matches(rule, tool_name, string_args) {
                continue;
            }
            if rule.forbid {
                return Some(PolicyOutcome::Forbid); // Forbid short-circuits — strictest possible.
            }
            if let Some(tier) = rule.raise {
                highest_raise = Some(match highest_raise {
                    Some(h) => h.max(tier),
                    None => tier,
                });
            }
        }
        highest_raise.map(PolicyOutcome::Raise)
    }
}

/// Whether a rule matches an action (tool name glob AND, if present, an argument
/// predicate against any string argument).
fn rule_matches(rule: &PolicyRule, tool_name: &str, string_args: &[&str]) -> bool {
    if !glob_match(&rule.tool, tool_name) {
        return false;
    }
    // Argument predicates are ANDed with the tool match; if any arg predicate is
    // set, at least one string argument must satisfy it.
    if let Some(g) = &rule.arg_glob {
        if !string_args.iter().any(|a| glob_match(g, a)) {
            return false;
        }
    }
    if let Some(sub) = &rule.arg_contains {
        if !string_args.iter().any(|a| a.contains(sub.as_str())) {
            return false;
        }
    }
    true
}

/// Minimal glob matcher supporting `*` (any run, including `/`) and `?` (one
/// char). Deliberately dependency-free and deterministic. `**` behaves like `*`
/// (we do not distinguish path-segment semantics — `*` already spans `/`).
fn glob_match(pattern: &str, text: &str) -> bool {
    // Collapse `**` to `*` since our `*` already matches across `/`.
    let pattern: String = {
        let mut out = String::with_capacity(pattern.len());
        let mut prev_star = false;
        for c in pattern.chars() {
            if c == '*' {
                if !prev_star {
                    out.push('*');
                }
                prev_star = true;
            } else {
                out.push(c);
                prev_star = false;
            }
        }
        out
    };
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pat: &[u8], text: &[u8]) -> bool {
    // Classic iterative wildcard match with backtracking.
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star_p, mut star_t): (Option<usize>, usize) = (None, 0);
    while t < text.len() {
        if p < pat.len() && (pat[p] == b'?' || pat[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pat.len() && pat[p] == b'*' {
            star_p = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star_p {
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == b'*' {
        p += 1;
    }
    p == pat.len()
}

/// Reject a policy file that is readable by group or others (Unix). A tamperable
/// policy is worse than none, so this is an error, not a warning.
#[cfg(unix)]
fn enforce_owner_only(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("Failed to stat policy file {}", path.display()))?
        .permissions()
        .mode();
    // Reject any group/other permission bits (0o077).
    if mode & 0o077 != 0 {
        bail!(
            "Protected policy file {} must be owner-only (found mode {:o}); \
             run `chmod 600 {}`",
            path.display(),
            mode & 0o7777,
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_owner_only(_path: &std::path::Path) -> Result<()> {
    // Best-effort: no POSIX permission model to enforce.
    Ok(())
}

// ---------------------------------------------------------------------------
// Escalation / verdict message schema (reserved for #6d — defined here so the
// wire format is stable across increments; not yet used by any transport).
// ---------------------------------------------------------------------------

/// A verdict verb the invoking agent issues in response to an escalation (#6d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictDecision {
    /// Stop before the pending action, gracefully.
    Halt,
    /// Undo the already-performed action by replaying its durable rollback
    /// journal entry (executed by the child).
    Revert,
    /// Proceed with / resume the action.
    Continue,
}

/// Escalation request sent **child → parent** over the #6d WebSocket channel when
/// an action exceeds the agent's ceiling or the evaluator hesitates.
///
/// Reserved in #6b so the message schema (including the security-relevant
/// `tree_id` / `challenge` fields for #6d's mutual-auth handshake) is fixed
/// before the transport is built. Not yet constructed anywhere.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)] // wired up in #6d
pub struct EscalationMsg {
    /// Correlates this escalation with its verdict.
    pub id: String,
    /// The escalating agent's identity within the tree.
    pub agent_id: String,
    /// Root-orchestrator tree identity (also the mutual-auth trust root).
    pub tree_id: String,
    /// The tool + resolved arguments the agent proposes to run.
    pub action: serde_json::Value,
    /// Why the agent hesitated (human-readable).
    pub reason: String,
    /// Small context payload to enrich the upstream agent's decision/retry.
    pub enrichment: serde_json::Value,
    /// The action's classified blast-radius tier.
    pub blast_radius: BlastRadius,
    /// Whether the action is proven-reversible.
    pub reversible: bool,
    /// Per-connection channel-binding nonce for the mutual-auth handshake (#6d).
    pub challenge: String,
}

/// Verdict sent **parent → child** in response to an [`EscalationMsg`] (#6d).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)] // wired up in #6d
pub struct VerdictMsg {
    /// Must match the originating [`EscalationMsg::id`].
    pub escalation_id: String,
    /// The decision verb.
    pub decision: VerdictDecision,
    /// Optional context the parent adds to guide a `Continue` retry.
    #[serde(default)]
    pub added_context: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::BlastRadius::*;

    fn decl(reversible: Option<bool>) -> FunctionDeclaration {
        serde_json::from_value(serde_json::json!({
            "name": "t",
            "description": "d",
            "parameters": {"type": "object"},
            "reversible": reversible,
        }))
        .unwrap()
    }

    // --- one_step_down ---

    #[test]
    fn one_step_down_saturates_at_safe() {
        assert_eq!(one_step_down(Catastrophic), Destructive);
        assert_eq!(one_step_down(Destructive), Disruptive);
        assert_eq!(one_step_down(Disruptive), Reversible);
        assert_eq!(one_step_down(Reversible), Safe);
        assert_eq!(one_step_down(Safe), Safe);
    }

    // --- proven_reversible ---

    #[test]
    fn proven_reversible_requires_real_proof() {
        // Declared intrinsic reversibility counts.
        assert!(proven_reversible(&decl(Some(true)), false));
        // A registered rollback artifact counts (even without the declared flag).
        assert!(proven_reversible(&decl(None), true));
        assert!(proven_reversible(&decl(Some(false)), true));
        // Neither → not proven. An *asserted-false* or *absent* flag is not proof.
        assert!(!proven_reversible(&decl(None), false));
        assert!(!proven_reversible(&decl(Some(false)), false));
    }

    // --- required_authority: the orthogonal combination ---

    #[test]
    fn unclassified_requires_human_regardless_of_reversibility_or_policy() {
        assert_eq!(
            required_authority(StaticTier::Unclassified, None, false),
            RequiredAuthority::Human
        );
        // Proven reversibility does not rescue an unclassified tool.
        assert_eq!(
            required_authority(StaticTier::Unclassified, None, true),
            RequiredAuthority::Human
        );
        // Neither does a policy raise.
        assert_eq!(
            required_authority(
                StaticTier::Unclassified,
                Some(PolicyOutcome::Raise(Safe)),
                true
            ),
            RequiredAuthority::Human
        );
    }

    #[test]
    fn classified_tier_maps_straight_through_without_reversibility() {
        assert_eq!(
            required_authority(StaticTier::Tier(Destructive), None, false),
            RequiredAuthority::Tier(Destructive)
        );
    }

    #[test]
    fn proven_reversibility_lowers_required_authority_one_step() {
        // Destructive but provably reversible → only Disruptive authority needed.
        assert_eq!(
            required_authority(StaticTier::Tier(Destructive), None, true),
            RequiredAuthority::Tier(Disruptive)
        );
        // Safe stays Safe (saturating).
        assert_eq!(
            required_authority(StaticTier::Tier(Safe), None, true),
            RequiredAuthority::Tier(Safe)
        );
    }

    #[test]
    fn policy_can_only_raise_the_base_tier() {
        // Policy raises a Disruptive tool to Catastrophic.
        assert_eq!(
            required_authority(
                StaticTier::Tier(Disruptive),
                Some(PolicyOutcome::Raise(Catastrophic)),
                false
            ),
            RequiredAuthority::Tier(Catastrophic)
        );
        // Policy "raise" to a LOWER tier than static is a no-op (max wins) —
        // policy never lowers.
        assert_eq!(
            required_authority(
                StaticTier::Tier(Destructive),
                Some(PolicyOutcome::Raise(Safe)),
                false
            ),
            RequiredAuthority::Tier(Destructive)
        );
    }

    #[test]
    fn policy_forbid_is_human_reserved() {
        assert_eq!(
            required_authority(
                StaticTier::Tier(Safe),
                Some(PolicyOutcome::Forbid),
                true
            ),
            RequiredAuthority::Human
        );
    }

    #[test]
    fn reversibility_applies_after_policy_raise() {
        // Policy raises Disruptive→Destructive, then proven reversibility lowers
        // Destructive→Disruptive. Order matters: raise first, then the one-step
        // reversibility discount on the raised floor.
        assert_eq!(
            required_authority(
                StaticTier::Tier(Disruptive),
                Some(PolicyOutcome::Raise(Destructive)),
                true
            ),
            RequiredAuthority::Tier(Disruptive)
        );
    }

    #[test]
    fn catastrophic_is_a_hard_floor_reversibility_cannot_discount() {
        // A policy raise to Catastrophic must NOT be discounted by proven
        // reversibility — catastrophic is human-only regardless. (Regression:
        // a reversible `safe` tool policy-raised to catastrophic previously
        // dropped to Destructive and slipped under a Destructive ceiling.)
        assert_eq!(
            required_authority(
                StaticTier::Tier(Safe),
                Some(PolicyOutcome::Raise(Catastrophic)),
                true
            ),
            RequiredAuthority::Tier(Catastrophic)
        );
        // Same for an intrinsically-catastrophic tool that claims reversibility.
        assert_eq!(
            required_authority(StaticTier::Tier(Catastrophic), None, true),
            RequiredAuthority::Tier(Catastrophic)
        );
        // And a Destructive ceiling must NOT permit it.
        let ceiling = AuthorityCeiling::UpTo(Destructive);
        assert!(!ceiling.permits(required_authority(
            StaticTier::Tier(Safe),
            Some(PolicyOutcome::Raise(Catastrophic)),
            true
        )));
    }

    // --- AuthorityCeiling ---

    #[test]
    fn ceiling_permits_within_and_denies_above() {
        let ceiling = AuthorityCeiling::UpTo(Disruptive);
        assert!(ceiling.permits(RequiredAuthority::Tier(Safe)));
        assert!(ceiling.permits(RequiredAuthority::Tier(Reversible)));
        assert!(ceiling.permits(RequiredAuthority::Tier(Disruptive)));
        assert!(!ceiling.permits(RequiredAuthority::Tier(Destructive)));
        assert!(!ceiling.permits(RequiredAuthority::Tier(Catastrophic)));
        // Human is never within any ceiling.
        assert!(!ceiling.permits(RequiredAuthority::Human));
        assert!(!AuthorityCeiling::UpTo(Catastrophic).permits(RequiredAuthority::Human));
    }

    #[test]
    fn ceiling_lowering_is_monotone_min() {
        let orch = AuthorityCeiling::UpTo(Destructive);
        // Granting a lower ceiling lowers it.
        assert_eq!(
            orch.lower_to(AuthorityCeiling::UpTo(Reversible)),
            AuthorityCeiling::UpTo(Reversible)
        );
        // "Granting" a higher ceiling cannot raise it (min wins).
        assert_eq!(
            orch.lower_to(AuthorityCeiling::UpTo(Catastrophic)),
            AuthorityCeiling::UpTo(Destructive)
        );
        assert_eq!(AuthorityCeiling::MINIMAL.tier(), Safe);
    }

    // --- glob matcher ---

    #[test]
    fn glob_matches_wildcards_and_literals() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("fs_*", "fs_write"));
        assert!(!glob_match("fs_*", "web_search"));
        assert!(glob_match("/etc/*", "/etc/passwd"));
        assert!(glob_match("/etc/**", "/etc/nginx/nginx.conf")); // ** spans /
        assert!(glob_match("/etc/*", "/etc/nginx/nginx.conf")); // * also spans /
        assert!(glob_match("db?", "db1"));
        assert!(!glob_match("db?", "db")); // ? requires exactly one char
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exacts"));
    }

    // --- policy file: parse + evaluate ---

    fn policy(yaml: &str) -> PolicyFile {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn policy_forbid_rule_beats_everything() {
        let p = policy(
            "rules:\n  - tool: '*'\n    arg_contains: prod\n    forbid: true\n  - tool: '*'\n    raise: safe\n",
        );
        assert_eq!(
            p.evaluate("db_query", &["SELECT * FROM prod.users"]),
            Some(PolicyOutcome::Forbid)
        );
    }

    #[test]
    fn policy_raise_takes_highest_matching_tier() {
        let p = policy(
            "rules:\n  - tool: 'fs_*'\n    raise: disruptive\n  - tool: '*'\n    arg_glob: '/etc/**'\n    raise: catastrophic\n",
        );
        // Both rules match fs_write on /etc/... → highest (catastrophic) wins.
        assert_eq!(
            p.evaluate("fs_write", &["/etc/passwd"]),
            Some(PolicyOutcome::Raise(Catastrophic))
        );
        // Only the first matches for a non-/etc path.
        assert_eq!(
            p.evaluate("fs_write", &["/home/user/x"]),
            Some(PolicyOutcome::Raise(Disruptive))
        );
    }

    #[test]
    fn policy_no_match_returns_none() {
        let p = policy("rules:\n  - tool: 'fs_*'\n    raise: destructive\n");
        assert_eq!(p.evaluate("web_search", &["cats"]), None);
    }

    #[test]
    fn policy_arg_glob_must_match_some_string_arg() {
        let p = policy("rules:\n  - tool: '*'\n    arg_glob: '/etc/**'\n    forbid: true\n");
        assert_eq!(p.evaluate("fs_write", &["/etc/hosts"]), Some(PolicyOutcome::Forbid));
        // No arg matches the glob → rule does not fire.
        assert_eq!(p.evaluate("fs_write", &["/tmp/scratch"]), None);
        // No string args at all → arg-predicate rule cannot fire.
        assert_eq!(p.evaluate("fs_write", &[]), None);
    }

    #[test]
    fn empty_policy_evaluates_to_none() {
        let p = PolicyFile::default();
        assert_eq!(p.evaluate("anything", &["x"]), None);
    }

    #[test]
    fn missing_policy_file_loads_empty_not_error() {
        let missing = std::env::temp_dir().join(format!(
            "aichat-nonexistent-policy-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!missing.exists());
        let p = PolicyFile::load(&missing).unwrap();
        assert!(p.rules.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn policy_file_rejects_group_or_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "aichat-policy-perm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("policy.yaml");
        std::fs::write(&path, "rules: []\n").unwrap();

        // World-readable (0644) must be rejected.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            PolicyFile::load(&path).is_err(),
            "world/group-readable policy must be rejected"
        );

        // Owner-only (0600) must load.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(PolicyFile::load(&path).is_ok(), "owner-only policy must load");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Backlog #6b: reserved #6d message schema (round-trips now) ---

    #[test]
    fn escalation_and_verdict_messages_round_trip() {
        let esc = EscalationMsg {
            id: "esc-1".into(),
            agent_id: "researcher".into(),
            tree_id: "tree-abc".into(),
            action: serde_json::json!({"tool": "wipe_disk", "args": {"path": "/dev/sda"}}),
            reason: "destructive, over my ceiling".into(),
            enrichment: serde_json::json!({"observed": "disk 90% full"}),
            blast_radius: Destructive,
            reversible: false,
            challenge: "nonce-xyz".into(),
        };
        let round: EscalationMsg =
            serde_json::from_str(&serde_json::to_string(&esc).unwrap()).unwrap();
        assert_eq!(round.id, "esc-1");
        assert_eq!(round.blast_radius, Destructive);
        assert!(!round.reversible);
        assert_eq!(round.challenge, "nonce-xyz");

        for (verb, wire) in [
            (VerdictDecision::Halt, "halt"),
            (VerdictDecision::Revert, "revert"),
            (VerdictDecision::Continue, "continue"),
        ] {
            let v = VerdictMsg {
                escalation_id: "esc-1".into(),
                decision: verb,
                added_context: None,
            };
            let s = serde_json::to_string(&v).unwrap();
            assert!(s.contains(&format!("\"{wire}\"")), "verb {verb:?} serializes as {wire}");
            let back: VerdictMsg = serde_json::from_str(&s).unwrap();
            assert_eq!(back.decision, verb);
        }
    }
}
