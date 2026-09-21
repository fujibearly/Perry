---
name: host_stamp
description: Timestamped host identity verification stamp
compatibility:
  os: [linux]
  tools: [get_current_time, fs_cat, fs_write]
allowed_tools: [get_current_time, fs_cat, fs_write]
---

# Host Stamp Procedure

This runbook provides a structured workflow for capturing system identity and timestamps in a deterministic fashion.

## Steps

1. **Capture Timestamp**: Call `get_current_time` and record the result. This establishes the audit moment.
2. **Read System Hostname**: Call `fs_cat` on `/etc/hostname` to retrieve the system's registered identity.
3. **Write Summary Report to File**: Call `fs_write` to persist the one-line summary `HOST_STAMP_VERIFIED: <hostname> at <timestamp>` to the designated output file.
4. **Output to Terminal**: Print the exact one-line summary `HOST_STAMP_VERIFIED: <hostname> at <timestamp>` directly to the terminal as your response.

## Success Criteria

All operations complete without error. Both the designated output file and the terminal output contain the line:

HOST_STAMP_VERIFIED: <hostname> at <timestamp>

## Notes

- This skill is intrinsically reversible (all operations on `/tmp/` or similar transient paths).
- Do NOT modify system files like `/etc/hostname` or `/etc/fstab`.
- Each run produces a unique output filename to avoid conflicts.
