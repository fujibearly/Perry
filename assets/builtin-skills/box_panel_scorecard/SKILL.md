---
name: box_panel_scorecard
description: Presentation-only formatting runbook for rendering structured multi-pillar SRE telemetry JSON into an adaptive 5-column Unicode terminal scorecard
compatibility:
  os: [linux, macos, windows]
  tools: []
allowed_tools: []
---

# Terminal 5-Column Scorecard Presentation Runbook

> [!IMPORTANT]
> **Pure Presentation Skill (No Tool Calls)**: This skill contains presentation instructions for text rendering. Do NOT invoke `box_panel_scorecard` as a tool. Render the final formatted scorecard directly in your final response text.

This presentation skill defines how an invoking agent formats structured 5-pillar SRE diagnostic telemetry JSON into an adaptive, glanceable 5-column Unicode terminal scorecard.

## 1. Input Contract

This skill operates on structured telemetry JSON produced by diagnostic specialists (such as `sre`). The JSON contains:
- `host_header`: Global host identity, OS, kernel, CPU, RAM, uptime, and virtualization identity.
- `pillars`: Telemetry dictionaries for `environment`, `services`, `resources`, `network`, and `logs`.
- `anomalies`: List of correlated anomalies with entity names, status, reason, and cross-pillar correlation.
- `artifacts`: Local file paths (e.g. `log_dump`, `log_query`).

---

## 2. Terminal Scorecard Layout & Monospace Alignment

Render the scorecard inside a fenced code block (```text ... ```) to guarantee exact monospace alignment across all terminal emulators and log viewers.

### A. Host Identity & Baseline Header
Directly above the box panel, render a single-line cross-column anchor establishing host identity:
`### Host Triage: <hostname> (<os> | <primary_ip> | <cores>c/<threads>t <cpu_model> | <total_ram> RAM | Up: <uptime> | virt: <virt>)`

### B. 5-Column Box Sizing (Terminal Display Minus 5%)
- Standard display width: Sized to fit terminal display minus 5%: **38 characters wide per column**.
- Total table width: **196 characters** (5 columns × 38 characters = 190 characters + 6 vertical borders = 196 characters).
- Column headers: `ENVIRONMENT`, `SERVICES`, `RESOURCES`, `NETWORK`, `LOGS` (each padded to 38 chars).
- Box framing characters:
  - Top border: `┌──────────────────────────────────────┬───...───┐`
  - Header separator: `├──────────────────────────────────────┼───...───┤`
  - Vertical border: `│`
  - Bottom border: `└──────────────────────────────────────┴───...───┘`

### C. Color-Coded Status Tags
- Mark degraded items with `[FAIL]` (red) or `[WARN]` (yellow). Normal states are `[OK]`.
- Attach status tags directly to specific entities (e.g., `• thermald.service [FAIL]`, `Disk /: 292G/340G (90%) [WARN]`, `GPU: 90°C [WARN]`, `wlan0: 34 tx_drop [WARN]`).
- Do NOT label entire column headers with `[FAIL]` or `[WARN]`.

### D. Variable Row Depths & In-Column Text Wrapping
- Columns take as many rows as needed to represent all entities.
- If text exceeds 38 characters, wrap it naturally onto subsequent lines within that column cell. Rows do not need to be uniform across columns.

### E. Explicit Enumeration & Bounding
- Never replace actionable technical telemetry with vague aggregates or pure counts.
- **CPU**: Busy % of cores, plus Top 3 processes with PIDs (`• <comm> (<pid>): <pct>%`).
- **Memory**: Used / capacity ratio (`<used_mb>M / <total_mb>M (<pct>%)`), plus Top 3 RSS processes with PIDs.
- **Swap**: Used / capacity ratio, plus Top 3 swap processes with PIDs.
- **Network**: Throughput and drop counts per interface, bound listeners with PIDs (`• :<port>/<proto> <svc> (<pid>)`), and active remote outbound connections (`• :<lport> -> <rem_ip>:<rport> <svc> (<pid>)`).
- **Services**: Degraded units with scope, state, and reason (`• <unit> (<scope>) [FAIL]: <reason>`) and active container/VM guests.
- **Logs**: Error counts over lookback window, critical singleton errors, and surge signatures with failure descriptions.

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

---

## 3. Anomaly Callouts & Telemetry Hyperlinks

Directly below the fenced box panel:
1. **Cross-Pillar Anomalies**:
   - List each degraded finding from the JSON `anomalies` array with root cause, impacted service or interface, and cross-pillar explanation.
2. **Local Artifact Hyperlinks**:
   - Surface all files in the JSON `artifacts` object as clickable markdown links with absolute `file://` URLs (e.g., `[Log Dump](file:///tmp/perry-host_logs-dump-....log)` and `[Log Query](file:///tmp/perry-host_logs-query-....sh)`).
3. **Zero Conversational Prose**:
   - Do NOT include conversational filler ("Here is the report you requested...", "In summary...", "I hope this helps"). Present the header, box table, anomaly callouts, and artifact links directly.
