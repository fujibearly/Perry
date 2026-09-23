# Project Perry: High-Assurance Agentic Harness for SRE

> **Status:** Canonical Vision Document & Architectural Manifesto  
> **Target Audience:** System operators, SRE maintainers, contributors, and AI agents collaborating on the Perry codebase.  
> **Core Purpose:** The defining North Star for why Project Perry exists, how it differentiates from standard AI agents, and the unshakeable engineering creed governing its evolution.

---

## 1. The Vision: AI Built for Real Infrastructure

Most AI agents today are engineered for toy environments. They are either coding copilots artificially constrained to a Git repository worktree, or heavyweight, multi-gigabyte Python and Docker frameworks that crumble the moment they touch a stripped-down production server.

**Project Perry is built for the operational reality of Site Reliability Engineering.**

It is an ultra-compact, zero-dependency, 64MB static binary designed to drop onto any machine—from a modern cloud Kubernetes node to a 15-year-old bare-metal bastion—and safely diagnose, contain, and remediate live infrastructure incidents across root filesystems, daemons, kernels, and network stacks.

---

## 2. The Engineering Creed: Four Architectural Pillars

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                               THE PERRY ENGINEERING CREED                              │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ 1. MECHANICAL SAFETY OVER MODEL PROMISES                                              │
│    The LLM is an auditor, never a pardoner. Safety gates fail closed to unyielding    │
│    deterministic code, static policy files, and capability boundaries.                 │
│                                                                                        │
│ 2. PROCESS ISOLATION OVER THREAD SOUP                                                 │
│    Subagents are independent OS child processes with dedicated PIDs and budgets.       │
│    No shared memory leaks. No cascading async task crashes. Clean Unix pipes.         │
│                                                                                        │
│ 3. TRIAGE IN PARALLEL, ACTUATE IN SEQUENCE                                             │
│    Diagnostic swarms inspect host telemetry concurrently at 1x latency. State-changing │
│    mutations are strictly gated, staged, and proven reversible before execution.       │
│                                                                                        │
│ 4. ZERO-DEPENDENCY BASTION UBIQUITY                                                    │
│    Statically compiled against musl libc with pure Rust cryptography. Runs comfortably  │
│    in 64MB RAM on Linux 2.6.32+ without Python, Node.js, glibc, or Docker runtimes.   │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Market Differentiation: Why SRE Demands Perry

| Dimension | Typical AI Coding Agents | Heavy AI Orchestrators | Project Perry (SRE Harness) |
| :--- | :--- | :--- | :--- |
| **Operational Scope** | Single Git worktree / `$CWD` | Abstract cloud APIs / Web | **Full OS Scope:** Filesystems, systemd, sockets, K8s, bastions |
| **Execution Model** | Single continuous thread | In-memory coroutines / threads | **Process-Isolated:** Independent child OS subprocesses (PIDs) |
| **Safety Governance** | Prompt instructions / "Vibes" | Model self-reflection | **Deterministic Floor:** Non-pardonable policy files & 0600 WAL |
| **Host Footprint** | Heavy IDE / Electron app | Gigabytes of Python/Node venvs | **Zero-Dependency:** Single static 64MB musl binary |
| **Reversibility** | `git checkout` / `git revert` | None / User responsibility | **Transactional:** Pre-flight Option B backups & undo tombstones |
| **Telemetry & Pipes** | Pollutes standard stdout/err | Interleaved logging frameworks | **Out-of-Band:** Dedicated `/dev/tty` (preserves `perry ... \| jq`) |
| **Context Hygiene** | Unconstrained tool dumping | Window overflow / Auto-truncation | **Stream Protection:** 16KB auto-capping & direct tool piping |

---

## 4. The Strategic Horizon (Roadmap Trajectory)

Perry is systematically advancing from an incident triage harness into an autonomous, transactional operational platform:

1. **Zero-Token Pre-Flight Triage:** Probing host health and baselines using deterministic shell heuristics before spending a single model token.
2. **Transactional Staged Actuation:** Staging configuration mutations in `/tmp/staging/` and validating them with host syntax checkers (`nginx -t`, `sshd -t`, `kubectl diff`) prior to atomic commit.
3. **Flight Recorder & Session Replay:** Tamper-evident, hash-chained event journals that allow any autonomous incident run to be deterministically replayed in post-mortems.
4. **Episodic Incident Memory:** Single-node, lightweight incident history capturing symptoms, root causes, and verified remediations so the system learns from outages without complex vector infrastructure.

---

## 5. The Core Tenet

> **Project Perry replaces blind autonomous execution with mechanical, fail-closed systems engineering—giving operators an AI harness powerful enough to triage an entire cluster, yet disciplined enough to run on a production bastion without fear.**
