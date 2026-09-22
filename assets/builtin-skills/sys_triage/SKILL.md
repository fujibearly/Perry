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

## Triage Workflow

The 5 investigation pillars are mutually independent. Execute or delegate them **concurrently in parallel** (either directly using the host tools or by delegating to specialist subagents such as `sre`) to minimize latency and turn overhead:

### Multi-Agent Orchestration Mode
When operating as an orchestrator, plan upfront using `_plan` and delegate the 5 pillars in parallel to the `sre` specialist subagent:
- `sre`: task="Audit host environment and hardware baseline using host_env"
- `sre`: task="Audit degraded services and unit states using host_service"
- `sre`: task="Audit CPU, memory, and storage saturation using host_resource"
- `sre`: task="Audit network interfaces and packet drops using host_net"
- `sre`: task="Audit recent error logs and anomalies using host_logs"
Once the subagents complete, synthesize their findings into the final report.

### Standalone Direct Execution Mode
When operating standalone with direct tool access, invoke the 5 tool actions in parallel:
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
   - Audit CPU utilization against known core count, memory used vs total capacity, swap, and root storage device/mount.
4. **Network Health, Packet Integrity & Listening Services (`host_net`)**:
   - Call `host_net` with `action='interfaces'` (or `action='listeners'` to audit bound ports).
   - Check all physical and virtual interfaces for packet drops (`tx_dropped`, `rx_dropped`), errors, and enumerate active listening services/ports.
5. **Log Signatures & Anomaly Spotting (`host_logs`)**:
   - Call `host_logs` with `action='recent_errors'` (or with `since='24h'`).
   - Spot critical singleton anomalies (OOM kills, panics, segfaults) and volume surges (restart loops).
   - Retain verbatim key evidence and materialize log query/dump artifacts.

## Incident Synthesis & Reporting Requirements

Synthesize a high-density, glanceable SRE health scorecard adhering strictly to these rules:

1. **Host Identity & Hardware Anchor**:
   Start with a compact single-line header establishing host identity, hardware bounds, and IP address:
   `### Host Triage: <hostname> (<OS> <kernel> | <primary_ip> | <cores>c/<threads>t <cpu_model> | <total_ram> RAM | Up: <uptime> | virt: <virt>)`
   (If discrete/integrated GPU is detected, include its model in the header or Resources. If hosting containers or VMs, note active guest count).

2. **High-Density Telemetry Scorecard Table**:
   Render an operational summary table. Use text status badges `[OK]`, `[WARN]`, `[FAIL]` (NO emojis):
   | Pillar | Status | Telemetry Summary (Bounded Metrics & Ratios) |
   |:---|:---:|:---|
   | **Resources** | `[OK]` or `[WARN]` | CPU: `<busy>% of <cores>c (idle <idle>%)` \| Mem: `<used>/<total> (<pct>%)` \| Swap: `<used>/<total> (<pct>%)` \| Disk `/`: `<used>/<total> (<pct>% - <avail> free)` (include `\| GPU: <busy>% (<used>/<total> VRAM, <temp>°C)` if present) |
   | **Services** | `[OK]` or `[FAIL]` | Degraded unit list with state or `None degraded` |
   | **Network** | `[OK]` or `[WARN]` | Interface packet drop ratios and listening services: `<iface>: <tx_drop> tx_drop / <tx_mb> MB (listeners: <svcs/ports>)` |
   | **Logs** | `[OK]` or `[FAIL]` | Active volume surges and notable singletons |

3. **Bounding Rule (Capacity / Denominator)**:
   Relative percentages MUST always be presented with their absolute capacity bounds as compact ratios: `used / total (pct%)` (e.g. `3.2G / 15.5G (20.6%)`, `315G / 340G (90%)`, `24.6% of 4 cores`). Never output floating percentages in a vacuum.

4. **Semantic Anomaly Correlation & Technical Evidence**:
   Below the scorecard, list ONLY anomalous/degraded items and cross-pillar correlations:
   - Correlate network drops (e.g. `wlan0`) with relevant driver or daemon logs (e.g. `wpa_supplicant`).
   - Cite verbatim error lines, PIDs, unit files, and interface names.
   - Surface any artifacts as clickable markdown links with `file://` URLs (e.g. `[Log Dump](file:///tmp/perry-host_logs-dump-....log)`).

5. **Zero Conversational Prose**:
   Strictly avoid conversational narrative ("This report details a comprehensive...", "In conclusion...", "As we can see..."). Every line must convey dense telemetry readable in a 1-second glance.
