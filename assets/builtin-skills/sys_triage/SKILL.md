---
name: sys_triage
description: Comprehensive 5-pillar host triage and incident diagnosis runbook
compatibility:
  os: [linux]
  tools: [host_env, host_resource, host_service, host_net, host_logs, sre, _plan]
allowed_tools: [host_env, host_resource, host_service, host_net, host_logs, sre, _plan]
---

# Comprehensive Host SRE Triage Procedure

This runbook defines the canonical SRE procedure for triaging a host system across environment identity, service state, resource saturation, network integrity, and diagnostic log anomalies.

## Triage Workflow: Holistic Context over Disparate Silos

Triage accuracy requires **cross-pillar context awareness**. When investigation pillars are executed in isolated silos across multiple subagents, vital causal links are lost (e.g., a failed thermal daemon causing GPU throttling/high temperatures, or network packet drops caused by driver errors in system logs).

To ensure complete situational awareness, the 5 diagnostic pillars must be evaluated within a **single unified agent context** where all telemetry streams are co-located:

### 1. Multi-Agent Orchestration Mode (Orchestrator Role)
When operating as an orchestrator, formulate your strategy with `_plan` and delegate the entire 5-pillar sweep to a **single dedicated `sre` specialist subagent**:
- **Single Holistic Delegation**:
  Delegate the complete triage to a single `sre` subagent so it possesses all telemetry simultaneously:
  `sre`: task="Perform a unified 5-pillar host health triage (host_env, host_service, host_resource, host_net, host_logs). Invoke all 5 actuators in parallel within a single turn, correlate cross-pillar findings (e.g. service failures vs resource anomalies, network drops vs log errors), and synthesize an anchored health scorecard adhering to the sys_triage runbook."
- **Do NOT shatter triage into separate subagents per pillar**: Spawning multiple subagents for individual pillars destroys causal correlation, multiplies token consumption (+30%+ framing overhead), and creates analytical blind spots.
- Once the SRE specialist returns the correlated assessment, review and present the synthesized scorecard.

### 2. Specialist / Direct Execution Mode (`sre` Role or Standalone)
When executing the triage (as the `sre` agent or operating standalone), invoke all 5 actuator tools **concurrently in a single turn**:
1. **Environment & Hardware Baseline Anchor (`host_env`)**:
   - Call `host_env` with `action='summary'`.
   - Establish hostname, OS distribution, kernel version, CPU architecture/model/cores, total physical RAM, virtualization/container boundary, and hosted virtualization workloads (Docker/Podman/KVM/LXC).
   - All subsequent metric interpretations must be anchored to this baseline.
2. **Service Lifecycle & Process Topology (`host_service`)**:
   - Call `host_service` with `action='failed'` to detect degraded units.
   - If degraded units exist, note unit name, load state, description, and unit file path (`unit_file`).
   - If the host runs virtualized workloads, call `action='guests'` to audit active containers and VMs.
3. **Resource Saturation & Bottlenecks (`host_resource`)**:
   - Call `host_resource` with `action='summary'`.
   - Audit CPU utilization against known core count, memory used vs total capacity, swap, root storage device/mount, and GPU compute/temperature.
4. **Network Health, Packet Integrity & Listening Services (`host_net`)**:
   - Call `host_net` with `action='interfaces'` (or `action='listeners'` to audit bound ports, `action='connections'` to audit active remote IPs/connections).
   - Check all physical and virtual interfaces for packet drops (`tx_dropped`, `rx_dropped`), errors, and enumerate active listening services/ports and connected remote IPs.
5. **Log Signatures & Anomaly Spotting (`host_logs`)**:
   - Call `host_logs` with `action='recent_errors'` (or with `since='24h'`).
   - Spot critical singleton anomalies (OOM kills, panics, segfaults) and volume surges (restart loops).
   - Retain verbatim key evidence and materialize log query/dump artifacts.

## Cross-Pillar Correlation Matrix

The diagnosing agent must actively cross-reference findings across co-located pillar data:
- **Services $\leftrightarrow$ Resources**: Does a failed daemon (e.g., `thermald`, `irqbalance`) explain elevated temperatures, fan noise, or CPU core affinity imbalance?
- **Services $\leftrightarrow$ Logs**: For any failed or degraded service, inspect `host_logs` to retrieve the exact exit code, missing binary path, or panicking stack trace.
- **Network $\leftrightarrow$ Logs**: If interface packet drops (`rx_dropped`, `tx_dropped`) or error counters are elevated, correlate with kernel driver events, link flapping, or firewall conntrack saturation in logs.
- **Environment $\leftrightarrow$ Workloads**: If `hosting` indicates active container or VM workloads, cross-check virtual bridges (`docker0`, `br-`) and cgroup resource caps.

## Incident Synthesis & Reporting Requirements

Synthesize a high-density, glanceable SRE health scorecard adhering strictly to these rules:

1. **Host Identity & Hardware Anchor (Cross-Column Single Row)**:
   Start with a compact cross-column single-line header establishing host identity, hardware bounds, and IP address spanning across the full width:
   `### Host Triage: <hostname> (<OS> <kernel> | <primary_ip> | <cores>c/<threads>t <cpu_model> | <total_ram> RAM | Up: <uptime> | virt: <virt>)`
   (If discrete/integrated GPU is detected, include its model in the header or Resources. If hosting containers or VMs, note active guest count).

2. **Terminal-Native 5-Column Box-Drawing Panel**:
   Directly below the cross-column host identification header, render an operational 5-column box panel inside a fenced text block (```text ... ```) to guarantee exact monospace alignment across all terminal environments.
   - **Equal-Width Columns (31 characters wide each)**:
     - 5 equal-width columns of exactly 31 characters each (total panel width = 161 characters).
     - Column headers are clean pillar names without column-wide badges: `ENVIRONMENT`, `SERVICES`, `RESOURCES`, `NETWORK`, `LOGS`.
   - **Granular Inline Alert Badges (NO Column-Wide Painting)**:
     - Do NOT brand an entire pillar header with `[FAIL]` or `[WARN]`.
     - Tie status tags (`[OK]`, `[WARN]`, `[FAIL]`) directly to the specific items they refer to (e.g., `• thermald.service [FAIL]`, `Disk /: 292G/340G (90%) [WARN]`, `GPU: 90°C [WARN]`, `wlan0: 34 drops [WARN]`).
   - **Variable Row Depths & Natural In-Column Text Wrapping**:
     - Columns take as many rows as needed. If text exceeds 31 characters, wrap it naturally onto the next line within that column cell.
     - Pillars with fewer items leave their trailing cells blank (` `). The number of rows per pillar does not need to be equal.
   - **Unicode Box Framing**:
     - Use standard box characters: top `┌─┬─┐`, header divider `├─┼─┤`, cell borders `│`, bottom `└─┴─┘`.

```text
┌───────────────────────────────┬───────────────────────────────┬───────────────────────────────┬───────────────────────────────┬───────────────────────────────┐
│ ENVIRONMENT                   │ SERVICES                      │ RESOURCES                     │ NETWORK                       │ LOGS                          │
├───────────────────────────────┼───────────────────────────────┼───────────────────────────────┼───────────────────────────────┼───────────────────────────────┤
│ Host: <hostname> [OK]         │ Failed Units: <count> [FAIL]  │ CPU: <busy>% of <c>c [OK]     │ <iface>: <drop> tx_drop [WARN]│ <count>x <service> [FAIL]     │
│ OS: <OS> <kernel> [OK]        │ • <failed_unit1> [FAIL]       │ Mem: <used>/<total> (%) [OK]  │ <iface>: <drop> tx_drop [WARN]│   <failure_description>       │
│ CPU: <cores>c/<threads>t      │ • <failed_unit2>-             │ Swap: <used>/<total> [OK]     │ Listeners: <count> [OK]       │   Reason: <missing_binary>    │
│ RAM: <total_ram> total [OK]   │   <service_name> [FAIL]       │ Disk /: <used>/<total> [WARN] │ Remote IPs: <count> [OK]      │ Kernel: <warning_msg> [WARN]  │
│ Uptime: <uptime> [OK]         │ Active Guests: <count> [OK]   │ GPU: <temp>°C (thermal!) [WARN│ Established: <count> conns    │ Core Dumps: <count> [OK]      │
│ Hosting: <virt/hosting> [OK]  │                               │ GPU Compute: <busy>% [OK]     │                               │                               │
└───────────────────────────────┴───────────────────────────────┴───────────────────────────────┴───────────────────────────────┴───────────────────────────────┘
```

3. **Bounding Rule (Capacity / Denominator)**:
   Relative percentages MUST always be presented with their absolute capacity bounds as compact ratios: `used / total (pct%)` (e.g. `3.2G / 15.5G (20.6%)`, `315G / 340G (90%)`, `24.6% of 4 cores`). Never output floating percentages in a vacuum.

4. **Semantic Anomaly Correlation & Technical Evidence**:
   Below the scorecard, list ONLY anomalous/degraded items and cross-pillar correlations:
   - Correlate network drops (e.g. `wlan0`) with relevant driver or daemon logs (e.g. `wpa_supplicant`).
   - Cite verbatim error lines, PIDs, unit files, and interface names.
   - Surface any artifacts as clickable markdown links with `file://` URLs (e.g. `[Log Dump](file:///tmp/perry-host_logs-dump-....log)`).

5. **Zero Conversational Prose**:
   Strictly avoid conversational narrative ("This report details a comprehensive...", "In conclusion...", "As we can see..."). Every line must convey dense telemetry readable in a 1-second glance.
