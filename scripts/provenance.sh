#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Soroban Forge — WASM provenance manifest generator/verifier.
#
# Produces `provenance-manifest.json`: a SHA-256 manifest of every contract
# WASM artifact produced by the pinned toolchain at a named git revision.
# Reviewers and integrators can reproduce the build and confirm the deployed
# WASM hash matches the source they are reading.
#
# Usage:
#   scripts/provenance.sh build              # build artifacts + write manifest
#   scripts/provenance.sh verify [MANIFEST]  # rebuild + compare hashes (exit 1 on drift)
#
# The manifest is intentionally dependency-free (sha256sum + awk), so it runs
# in CI and on a plain checkout with no jq/python requirement.
#
# NOTE: keep PACKAGES in sync with the contract crates in the wasm-size job
# in .github/workflows/ci.yml.
# ---------------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET="wasm32v1-none"
MANIFEST="${2:-$ROOT/provenance-manifest.json}"

PACKAGES=(
  soroban-forge-escrow
  soroban-forge-vesting
  soroban-forge-multi-sig-wallet
  soroban-forge-dao-governance
  soroban-forge-subscription-payments
  soroban-forge-marketplace-royalties
)

build_artifacts() {
  cargo build --locked --release --target "$TARGET" \
    --package soroban-forge-escrow \
    --package soroban-forge-vesting \
    --package soroban-forge-multi-sig-wallet \
    --package soroban-forge-dao-governance \
    --package soroban-forge-subscription-payments \
    --package soroban-forge-marketplace-royalties
}

write_manifest() {
  local rev generated_at
  rev="$(git rev-parse HEAD 2>/dev/null || echo "unknown")"
  generated_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  {
    printf '{\n'
    printf '  "schema": "soroban-forge/provenance@1",\n'
    printf '  "git_commit": "%s",\n' "$rev"
    printf '  "generated_at": "%s",\n' "$generated_at"
    printf '  "target": "%s",\n' "$TARGET"
    printf '  "artifacts": [\n'
    local first=1
    # Glob is alphabetical, so the artifact list is deterministic per tree.
    for wasm in target/"$TARGET"/release/*.wasm; do
      local name hash bytes
      name="$(basename "$wasm")"
      hash="$(sha256sum "$wasm" | cut -d' ' -f1)"
      bytes="$(stat -c%s "$wasm")"
      if [ "$first" -eq 1 ]; then
        first=0
      else
        printf ',\n'
      fi
      printf '    { "name": "%s", "sha256": "%s", "bytes": %s }' "$name" "$hash" "$bytes"
    done
    printf '\n  ]\n'
    printf '}\n'
  } > "$MANIFEST"

  echo "wrote $MANIFEST"
  cat "$MANIFEST"
}

verify_manifest() {
  local failures=0
  # Each artifact is a single line: { "name": "...", "sha256": "...", "bytes": N }
  while IFS= read -r line; do
    local name hash actual
    name="$(printf '%s' "$line" | sed -n 's/.*"name": "\([^"]*\)".*/\1/p')"
    hash="$(printf '%s' "$line" | sed -n 's/.*"sha256": "\([a-f0-9]*\)".*/\1/p')"
    if [ -z "$name" ] || [ -z "$hash" ]; then
      continue
    fi
    if [ ! -f "target/$TARGET/release/$name" ]; then
      echo "MISSING artifact: $name"
      failures=$((failures + 1))
      continue
    fi
    actual="$(sha256sum "target/$TARGET/release/$name" | cut -d' ' -f1)"
    if [ "$actual" = "$hash" ]; then
      echo "OK       $name $hash"
    else
      echo "MISMATCH $name manifest=$hash actual=$actual"
      failures=$((failures + 1))
    fi
  done < <(grep '"sha256"' "$MANIFEST")

  if [ "$failures" -ne 0 ]; then
    echo "provenance verification FAILED: $failures artifact(s) drifted"
    exit 1
  fi
  echo "provenance verification passed"
}

case "${1:-}" in
  build)
    build_artifacts
    write_manifest
    ;;
  verify)
    build_artifacts
    verify_manifest
    ;;
  *)
    echo "usage: $0 {build|verify} [MANIFEST_PATH]" >&2
    exit 2
    ;;
esac
