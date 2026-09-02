#!/usr/bin/env bash
#
# run-sast.sh — Best-effort static analysis (SAST) with Semgrep.
#
# Semgrep is an OPTIONAL, NON-GATING check. This script:
#   * exits 0 and prints a notice if semgrep is not installed (never a hard dependency),
#   * runs the offline Rust security pack (p/rust) when semgrep IS present,
#   * reports findings informationally and ALWAYS exits 0 (findings do not fail the build).
#
# Rationale: aichat ships as a zero-dependency static Rust binary. SAST is a
# developer/CI convenience, not a build requirement. Findings are surfaced for
# human review, not enforced as a gate.
#
# Usage:
#   scripts/run-sast.sh            # scan src/
#   scripts/run-sast.sh path ...   # scan specific paths
#
# Set SAST_STRICT=1 to make findings fail (exit non-zero) — off by default.

set -u

TARGETS=("${@:-src/}")

# Resolve the semgrep binary. Prefer PATH; fall back to the common pipx
# user-install location (~/.local/bin), which is where it lives on this system
# but may not be on PATH in every CI/shell environment.
SEMGREP=""
if command -v semgrep >/dev/null 2>&1; then
  SEMGREP="$(command -v semgrep)"
elif [ -x "${HOME}/.local/bin/semgrep" ]; then
  SEMGREP="${HOME}/.local/bin/semgrep"
fi

if [ -z "$SEMGREP" ]; then
  echo "SAST: semgrep not found on PATH or in ~/.local/bin — skipping (optional check, not a build dependency)."
  echo "      Install: https://semgrep.dev/docs/getting-started/  (or: pipx install semgrep)"
  exit 0
fi

echo "SAST: running semgrep ($SEMGREP, config p/rust) on: ${TARGETS[*]}"

# --config p/rust is the offline Rust security pack.
# We do NOT pass --error here: findings are informational by default.
semgrep_args=(scan --config p/rust --disable-version-check --quiet)

if [ "${SAST_STRICT:-0}" = "1" ]; then
  echo "SAST: SAST_STRICT=1 — findings will fail this run."
  semgrep_args+=(--error)
fi

if "$SEMGREP" "${semgrep_args[@]}" "${TARGETS[@]}"; then
  status=0
else
  status=$?
fi

if [ "${SAST_STRICT:-0}" = "1" ]; then
  # Strict mode: propagate semgrep's exit status (findings fail).
  exit "$status"
fi

# Default (non-gating) mode: always succeed, regardless of findings.
if [ "$status" -ne 0 ]; then
  echo "SAST: semgrep reported findings above (informational — not failing the build)."
fi
exit 0
