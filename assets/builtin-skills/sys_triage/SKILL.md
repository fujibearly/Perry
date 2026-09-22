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
  `sre`: task="Perform a unified 5-pillar host health triage (host_env, host_service, host_resource, host_net, host_logs). Invoke all 5 actuators in parallel within a single turn, correlate cross-pillar findings (e.g. service failures vs resource anomalies, network drops vs log errors), and synthesize an anchored health scorecard adhering to the sys_triage runbook. Explicitly enumerate concrete entities rather than just counting: include the Top 3 CPU processes (w/ PIDs), Top 3 Memory processes (w/ PIDs), Top 3 Swap processes (w/ PIDs), listening ports/services with PIDs, and active remote outbound connections (src -> dst ip:port w/ PIDs). Surface all local artifacts produced by actuators (e.g. host_logs dump_file and query_file) as clickable markdown links with file:// URLs."
- **Do NOT shatter triage into separate subagents per pillar**: Spawning multiple subagents for individual pillars destroys causal correlation, multiplies token consumption (+30%+ framing overhead), and creates analytical blind spots.
- Once the SRE specialist returns the correlated assessment, review and present the synthesized scorecard, preserving all local artifact `file://` hyperlinks in the final user-facing report.

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
3. **Resource Saturation, Bottlenecks & Top Consumers (`host_resource`)**:
   - Call `host_resource` with `action='summary'`.
   - Audit CPU utilization against known core count, and extract the **Top 3 CPU-consuming processes** (name, PID, %CPU).
   - Audit memory used vs capacity, and extract the **Top 3 Memory-consuming processes** (name, PID, RSS MB, %MEM).
   - Audit swap used vs capacity, and extract the **Top 3 Swap-consuming processes** (name, PID, Swap MB).
   - Audit root storage mount/capacity, and GPU compute/temperature.
4. **Network Health, Throughput, Listeners & Active Connections (`host_net`)**:
   - Call `host_net` with `action='interfaces'`.
   - Check all physical and virtual interfaces for packet drops (`tx_dropped`, `rx_dropped`), errors, and throughput (`rx_mb`, `tx_mb`).
   - Extract and enumerate bound **listening ports/services** (port, protocol, service, PID).
   - Extract and enumerate active **remote outbound connections** with endpoints (source port -> remote IP:port, service, PID).
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

2. **Terminal-Native 5-Column Box-Drawing Panel (Sized to Display Width Minus 5%)**:
   Directly below the cross-column host identification header, render an operational 5-column box panel inside a fenced text block (```text ... ```) to guarantee exact monospace alignment across all terminal environments.
   - **Fit Available Display Minus 5% (38 Characters Wide per Column)**:
     - Sized to fit the available terminal display minus 5%: on the standard host terminal display (206 columns), total table width is **196 characters** (5 columns × 38 characters = 190 characters + 6 vertical border characters = 196 characters).
     - Each of the 5 columns is exactly **38 characters wide**.
     - Column headers: `ENVIRONMENT`, `SERVICES`, `RESOURCES`, `NETWORK`, `LOGS` (each padded to 38 chars).
   - **Color-Coded Status Tags (Red [FAIL], Yellow [WARN])**:
     - Color-code `[FAIL]` status tags in **red** and `[WARN]` status tags in **yellow**.
     - `[OK]` status tags remain standard/green.
     - Tie status tags directly to the specific items they refer to (e.g., `• thermald.service [FAIL]`, `Disk /: 292G/340G (90%) [WARN]`, `GPU: 90°C [WARN]`, `wlan0: 34 drops [WARN]`). Do NOT paint an entire column header with `[FAIL]` or `[WARN]`.
   - **Variable Row Depths & Natural In-Column Text Wrapping**:
     - Columns take as many rows as needed. If text exceeds 38 characters, wrap it naturally onto the next line within that column cell.
     - Pillars take as many rows as needed; number of rows does NOT need to be the same across columns.
   - **Unicode Box Framing**:
     - Use standard box characters: top `┌─┬─┐`, header divider `├─┼─┤`, cell borders `│`, bottom `└─┴─┘`.

3. **Explicit Enumeration Rule (Enumerate, Never Just Count)**:
   Never replace actionable technical diagnostics with vague aggregates or counts. Telemetry must be enumerated so subsequent investigations have immediate actionable handles (PIDs, process names, endpoints):
   - **CPU**: Enumerate overall busy % of cores, followed by the **Top 3 CPU consumers**: `• <comm> (<pid>): <cpu_pct>%`.
   - **Memory**: Enumerate used / total ratio (used %), followed by the **Top 3 Memory consumers**: `• <comm> (<pid>): <rss_mb>M (<mem_pct>%)`.
   - **Swap**: Enumerate swap used / total ratio (used %), followed by the **Top 3 Swap consumers**: `• <comm> (<pid>): <swap_mb>M`.
   - **Network**: Enumerate per-interface throughput (`<rx_mb>M rx, <tx_mb>M tx`) and packet drops. Do not merely state listener or connection counts: **list listening ports/services with PIDs** (e.g. `• :<port>/<proto> <svc> (<pid>)`) and **list active remote outbound connections with source and dest sockets and PIDs** (e.g. `• :<lport> -> <rem_ip>:<rport> <svc> (<pid>)`).
   - **Services**: Enumerate failed units with load state and failure reason (`• <unit> [FAIL]: <reason>`) and active guest container/VM names.
   - **Logs**: Enumerate critical singleton errors, fail signatures, and crash dumps with PIDs, units, and timestamps.

```text
┌──────────────────────────────────────┬──────────────────────────────────────┬──────────────────────────────────────┬──────────────────────────────────────┬──────────────────────────────────────┐
│ ENVIRONMENT                          │ SERVICES                             │ RESOURCES                            │ NETWORK                              │ LOGS                                 │
├──────────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────┤
│ Host: <hostname> [OK]                │ Failed Units: <count> [FAIL]         │ CPU: <busy>% of <c>c [OK]            │ <iface>: <rx>M rx, <tx>M tx          │ <count>x <service> [FAIL]            │
│ OS: <OS> <kernel> [OK]               │ • <failed_unit1> [FAIL]              │ Top CPU (pid):                       │ • <drop> tx_drop [WARN]              │   <failure_description>              │
│ CPU: <cores>c/<threads>t             │   Reason: <reason>                   │ • <proc1> (<pid>): <pct>%            │ Listeners:                           │   Reason: <missing_binary>           │
│ RAM: <total_ram> total [OK]          │ • <failed_unit2> [FAIL]              │ • <proc2> (<pid>): <pct>%            │ • :<port>/<proto> <svc> (<pid>)      │ • <failed_service2> [FAIL]           │
│ Uptime: <uptime> [OK]                │ Active Guests: <count> [OK]          │ • <proc3> (<pid>): <pct>%            │ • :<port>/<proto> <svc> (<pid>)      │   Reason: <exit_status>              │
│ Hosting: <virt/hosting> [OK]         │ • <guest1> (<runtime>) [OK]          │ Mem: <used>/<total> (%) [OK]         │ Connections (src -> dst):            │ Kernel: <warning_msg> [WARN]         │
│ Display: <cols>c, panel <w>c [OK]    │                                      │ Top Mem (pid):                       │ • :<lport> -> <rem_ip>:<rport>       │ Core Dumps: <count> [OK]             │
│                                      │                                      │ • <proc1> (<pid>): <mb>M (<%>%)      │   <service> (<pid>)                  │                                      │
│                                      │                                      │ • <proc2> (<pid>): <mb>M (<%>%)      │ • :<lport> -> <rem_ip>:<rport>       │                                      │
│                                      │                                      │ • <proc3> (<pid>): <mb>M (<%>%)      │   <service> (<pid>)                  │                                      │
│                                      │                                      │ Swap: <used>/<total> [OK]            │                                      │                                      │
│                                      │                                      │ Top Swap (pid):                      │                                      │                                      │
│                                      │                                      │ • <proc1> (<pid>): <mb>M             │                                      │                                      │
│                                      │                                      │ • <proc2> (<pid>): <mb>M             │                                      │                                      │
│                                      │                                      │ • <proc3> (<pid>): <mb>M             │                                      │                                      │
│                                      │                                      │ Disk /: <used>/<total> [WARN]        │                                      │                                      │
│                                      │                                      │ GPU: <temp>°C (thermal!) [WARN]      │                                      │                                      │
│                                      │                                      │ GPU Compute: <busy>% [OK]            │                                      │                                      │
└──────────────────────────────────────┴──────────────────────────────────────┴──────────────────────────────────────┴──────────────────────────────────────┴──────────────────────────────────────┘
```

4. **Bounding Rule (Capacity / Denominator)**:
   Relative percentages MUST always be presented with their absolute capacity bounds as compact ratios: `used / total (pct%)` (e.g. `3.2G / 15.5G (20.6%)`, `315G / 340G (90%)`, `24.6% of 4 cores`). Never output floating percentages in a vacuum.

5. **Semantic Anomaly Correlation & Technical Evidence**:
   Below the scorecard, list ONLY anomalous/degraded items and cross-pillar correlations:
   - Correlate network drops (e.g. `wlan0`) with relevant driver or daemon logs (e.g. `wpa_supplicant`).
   - Cite verbatim error lines, PIDs, unit files, and interface names.
   - **Local Artifact Links**: Always surface any artifacts generated by actuators (such as `log_query`, `log_dump` from `host_logs`) as clickable markdown hyperlinks with absolute `file://` URLs (e.g. `[Log Dump](file:///tmp/perry-host_logs-dump-....log)` and `[Log Query](file:///tmp/perry-host_logs-query-....sh)`).

6. **Zero Conversational Prose**:
   Strictly avoid conversational narrative ("This report details a comprehensive...", "In conclusion...", "As we can see..."). Every line must convey dense telemetry readable in a 1-second glance.
