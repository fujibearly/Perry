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
   - Establish hostname, OS distribution, kernel version, CPU architecture/model/cores, total physical RAM, and virtualization/container boundary.
   - All subsequent metric interpretations must be anchored to this baseline.
2. **Service Lifecycle & Process Topology (`host_service`)**:
   - Call `host_service` with `action='failed'` to detect degraded units.
   - If degraded units exist, note unit name, load state, description, and unit file path (`unit_file`).
3. **Resource Saturation & Bottlenecks (`host_resource`)**:
   - Call `host_resource` with `action='summary'`.
   - Audit CPU utilization against known core count, memory used vs total capacity, swap, and root storage device/mount.
4. **Network Health & Packet Integrity (`host_net`)**:
   - Call `host_net` with `action='interfaces'`.
   - Check all physical and virtual interfaces for packet drops (`tx_dropped`, `rx_dropped`) and errors.
5. **Log Signatures & Anomaly Spotting (`host_logs`)**:
   - Call `host_logs` with `action='recent_errors'` (or with `since='24h'`).
   - Spot critical singleton anomalies (OOM kills, panics, segfaults) and volume surges (restart loops).
   - Retain verbatim key evidence and materialize log query/dump artifacts.

## Incident Synthesis & Reporting Requirements

Synthesize an executive and diagnostic health report adhering to these rules:
1. **Anchor to Host Baseline**: State the host, OS, CPU model/cores, and memory baseline. Never report floating percentages in a vacuum (e.g. state "26.1% CPU utilization across 4 logical cores of Intel Core i7-7Y75 on host mordor" and "4,345 MB used out of 15,883 MB total system RAM").
2. **Concrete Technical Evidence**: Cite verbatim evidence lines, affected PIDs, process names, unit files, and interface names.
3. **Semantic Key-Value Extraction**: Dynamically extract all diagnostic attributes relevant to the findings without assuming a fixed schema.
4. **Artifact Hyperlinks**: Surface any generated log queries or log dump artifacts as clickable markdown links with `file://` URLs (e.g. `[Log Dump](file:///tmp/perry-host_logs-dump-....log)`).
