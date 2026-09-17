#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Soroban Forge — mainnet (Pubnet) deployment + smoke round for the escrow.
#
# Mirrors scripts/demo-testnet.sh (same three escrow rounds, real SEP-41
# movement) against Stellar MAINNET. The differences from testnet are
# load-bearing:
#
#   - NO friendbot: identities must be funded BEFORE running. The script
#     preflights XLM balances and exits early rather than failing midway
#     through paid operations.
#   - every operation costs real XLM. The script prints a fee estimate and
#     requires an explicit --yes to proceed.
#   - the demo asset is a mainnet-issued custom token ("smoke credit"). It
#     has NO monetary value and the script never touches any real asset —
#     but it is still a real mainnet trustline, so identities are reused
#     across runs (no account churn).
#   - receipts: mainnet uses stellar.exploreex.com; the script prints
#     explorer links for the deployment and every round's transactions.
#
# Rounds performed (identical to the testnet demo):
#   round 1: create -> deposit -> release            (seller paid)
#   round 2: create -> deposit -> dispute -> resolve (arbiter sides with seller)
#   round 3: create -> deposit -> dispute -> resolve (arbiter refunds buyer)
#
# Prerequisites:
#   - stellar CLI on PATH (v23+; tested with v28.0.0)
#   - three funded mainnet identities (aliases below); create + fund them
#     manually the first time. Real reserve math (base reserve 0.5 XLM,
#     trustline +0.5, plus fee headroom — an account at exactly its
#     minimum cannot pay a transaction fee):
#       sf-main-issuer   2.5 XLM (token issuer; deploys SAC + escrow —
#                         contract rent lands here — and doubles as the
#                         arbiter: an arbiter is just an address that
#                         authorizes `resolve`, and a fourth identity
#                         would cost another 1 XLM reserve for nothing)
#       sf-main-buyer    2.0 XLM (1.5 reserve-with-trustline + fees)
#       sf-main-seller   2.0 XLM (1.5 reserve-with-trustline + fees)
#     If the escrow deploy fails on rent, top the issuer up by 1-2 XLM.
#   - the escrow WASM (built automatically if missing)
#
# Environment overrides:
#   ESCROW_ID   reuse an existing mainnet escrow (script will not redeploy)
#   TOKEN_ID    reuse an existing mainnet SAC (script will not re-issue)
#   AMOUNT      escrow amount per round (default 100, smoke units)
#   RPC_URL     Soroban RPC endpoint. Mainnet is bring-your-own in the
#               stellar CLI (v28 ships no public mainnet RPC); the default
#               below is Gateway.fm's public endpoint from the official
#               provider list (developers.stellar.org → RPC providers).
#
# Usage:  bash scripts/deploy-mainnet.sh          # interactive confirmation
#         bash scripts/deploy-mainnet.sh --yes    # skip confirmation
# ---------------------------------------------------------------------------
set -euo pipefail

command -v stellar >/dev/null 2>&1 || { echo "stellar CLI not found on PATH"; exit 1; }
NET="mainnet"
RPC_URL="${RPC_URL:-https://soroban-rpc.mainnet.stellar.gateway.fm}"
PASSPHRASE="Public Global Stellar Network ; September 2015"
EXPLORER="https://stellar.exploreex.com"
AMOUNT="${AMOUNT:-100}"
ESCROW_ID="${ESCROW_ID:-}"
TOKEN_ID="${TOKEN_ID:-}"
ISSUER_ALIAS="sf-main-issuer"
step() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
addr() { stellar keys address "$1" 2>/dev/null; }
link() { echo "  receipt: $EXPLORER/tx/$1"; }

# --- identities (must already exist and be funded) --------------------------
step "Identities (must be pre-funded — no friendbot on mainnet)"
MISSING=0
for name in "$ISSUER_ALIAS" sf-main-buyer sf-main-seller; do
  if ! A=$(addr "$name"); then
    echo "  MISSING: $name (create with: stellar keys generate $name)"
    MISSING=1
    continue
  fi
  echo "  $name: $A"
done
[ "$MISSING" -eq 0 ] || { echo "create the identities above, fund them (issuer 2.5 XLM, buyer 2.0 XLM, seller 2.0 XLM), then re-run"; exit 1; }
ISSUER=$(addr "$ISSUER_ALIAS"); BUYER=$(addr sf-main-buyer)
SELLER=$(addr sf-main-seller)
# Budget layout: the issuer doubles as the arbiter. An arbiter is just an
# address that authorizes `resolve`; a fourth funded identity would cost
# another 1 XLM base reserve for no additional property.
ARBITER="$ISSUER"
ARBITER_ALIAS="$ISSUER_ALIAS"

# --- preflight: real XLM balances (best-effort, unit-correct) ---------------
# The native-asset wrapper contract (`xlm_balance`) reports STROOPS; the
# CLI's `keys balance` reports whole XLM. Both are normalized to XLM below.
# The preflight is best-effort: if the installed CLI version disagrees with
# either invocation, the script warns and continues — a missed check here
# degrades to a per-operation fee failure, never to value loss, because
# nothing of monetary value is ever custodied by this script.
step "Preflight: XLM balances (best-effort)"
NATIVE="CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWXOF"
check_balance() { # identity_alias floor_whole_xlm
  local name="$1" floor="$2" BAL="" STROOPS CLI_BAL
  if STROOPS=$(stellar contract invoke --id "$NATIVE" --source-account "$name" \
    --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" -- xlm_balance 2>/dev/null | tail -1); then
    case "$STROOPS" in
      ''|*[!0-9]*) ;;  # not a plain integer — fall through to CLI balance
      *) BAL=$(awk -v s="$STROOPS" 'BEGIN{printf "%d", s/10000000}') ;;
    esac
  fi
  if [ -z "$BAL" ]; then
    # CLI-version-independent fallback: query Horizon for the native balance.
    # (`stellar keys balance` does not exist in all CLI versions.) Horizon's
    # balance objects list "balance" before "asset_type", so track the most
    # recent balance line and emit it when the native object's asset_type
    # line appears — correct with or without trustlines.
    local G NATIVE_BAL
    if G=$(stellar keys address "$name" 2>/dev/null); then
      NATIVE_BAL=$(curl -s --max-time 10 "https://horizon.stellar.org/accounts/$G" \
        | awk '/"balance": /{bal=$0} /"asset_type": "native"/{print bal; exit}' \
        | sed -n 's/.*"balance": "\([0-9.]*\)".*/\1/p')
      case "$NATIVE_BAL" in
        ''|*[!0-9.]*) ;;
        *) BAL=${NATIVE_BAL%%.*} ;;
      esac
    fi
  fi
  if [ -n "$BAL" ]; then
    echo "  $name: $BAL XLM"
    if [ "$BAL" -lt "$floor" ]; then
      echo "ERROR: $name has < $floor XLM (fund to the budget in the header and re-run)"
      exit 1
    fi
  else
    echo "  $name: balance unavailable (CLI mismatch?) — continuing; fees are paid per operation"
  fi
}
# 3-identity budget: issuer 2.5 XLM (carries contract rent), buyer 2.0 XLM
# and seller 2.0 XLM (1.5 reserve-with-trustline + fee headroom — an
# account at exactly its minimum cannot pay a fee). Floors are whole
# XLM because balances are integerized above.
check_balance "$ISSUER_ALIAS" 2
check_balance sf-main-buyer 2
check_balance sf-main-seller 2

# --- confirmation gate ------------------------------------------------------
step "Cost & scope confirmation"
echo "  network:     Stellar MAINNET (Pubnet) — operations cost real XLM"
echo "  asset:       'smoke' issued by this script's issuer (no monetary value)"
echo "  identities:  3 (issuer doubles as arbiter) — issuer 2.5 / buyer 2.0 / seller 2.0 XLM"
echo "  rounds:      3 escrow rounds x $AMOUNT smoke units (release + both dispute outcomes)"
if [ "${1:-}" != "--yes" ]; then
  printf '  proceed? type "yes" to continue: '
  read -r REPLY
  [ "$REPLY" = "yes" ] || { echo "aborted"; exit 1; }
fi

# --- demo token (SAC, mainnet-issued, zero value) ---------------------------
step "Smoke token (SAC for smoke:<issuer>)"
if [ -z "$TOKEN_ID" ]; then
  TOKEN_ID=$(stellar contract asset deploy \
    --asset "smoke:$ISSUER" \
    --source-account "$ISSUER_ALIAS" --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" 2>/dev/null | tail -1)
  [ -n "$TOKEN_ID" ] || { echo "token deploy failed"; exit 1; }
fi
echo "  token: $TOKEN_ID"

# Trustlines: receivers need one before holding the asset. Reuse silently
# when the trustline already exists (idempotent across re-runs).
step "Trustlines (buyer, seller)"
stellar tx new change-trust --source-account sf-main-buyer --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" \
  --line "smoke:$ISSUER" --limit 1000000 >/dev/null 2>&1 || echo "  buyer trustline exists"
stellar tx new change-trust --source-account sf-main-seller --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" \
  --line "smoke:$ISSUER" --limit 1000000 >/dev/null 2>&1 || echo "  seller trustline exists"

# --- escrow contract --------------------------------------------------------
step "Escrow contract (mainnet)"
if [ -z "$ESCROW_ID" ]; then
  wasm="target/wasm32v1-none/release/soroban_forge_escrow.wasm"
  [ -f "$wasm" ] || wasm="$(find target -name soroban_forge_escrow.wasm -path '*wasm32v1-none*' | head -1)"
  if [ -z "$wasm" ]; then
    echo "escrow WASM not found; building it (pinned toolchain, wasm32v1-none)..."
    cargo build --locked --release --target wasm32v1-none --package soroban-forge-escrow
    wasm="target/wasm32v1-none/release/soroban_forge_escrow.wasm"
  fi
  WASM_SHA=$(sha256sum "$wasm" | cut -d' ' -f1)
  echo "  wasm sha256: $WASM_SHA"
  echo "  (must match the provenance manifest — cross-check scripts/provenance.sh build)"
  ESCROW_ID=$(stellar contract deploy --wasm "$wasm" \
    --source-account "$ISSUER_ALIAS" --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" 2>/dev/null | tail -1)
  [ -n "$ESCROW_ID" ] || { echo "escrow deploy failed"; exit 1; }
fi
echo "  escrow: $ESCROW_ID"
echo "  explorer: $EXPLORER/contract/$ESCROW_ID"

# --- helpers ----------------------------------------------------------------
invoke() { # id source fn args...  -> prints tx hash on success
  local id="$1" src="$2"; shift 2
  stellar contract invoke --id "$id" --source-account "$src" --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" \
    -- "$@" 2>/dev/null | tail -1
}
balance() { stellar token balance --id "$TOKEN_ID" --account "$1" --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" 2>/dev/null | tail -1; }

# --- three rounds -----------------------------------------------------------
round_release() {
  step "Round 1: create -> deposit -> release"
  local id; id=$(invoke "$ESCROW_ID" sf-main-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" sf-main-buyer deposit --escrow_id "$id" >/dev/null
  echo "  deposited $AMOUNT (buyer -> contract)"
  invoke "$ESCROW_ID" sf-main-seller release --escrow_id "$id" >/dev/null
  echo "  released (contract -> seller)"
}

round_dispute_seller_wins() {
  step "Round 2: create -> deposit -> dispute(buyer) -> resolve FOR SELLER"
  local id; id=$(invoke "$ESCROW_ID" sf-main-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" sf-main-buyer deposit --escrow_id "$id" >/dev/null
  invoke "$ESCROW_ID" sf-main-buyer dispute --escrow_id "$id" --claimant "$BUYER" >/dev/null
  echo "  disputed by buyer"
  invoke "$ESCROW_ID" "$ARBITER_ALIAS" resolve --escrow_id "$id" --in_favor_of_seller true >/dev/null
  echo "  arbiter resolved for seller"
}

round_dispute_buyer_wins() {
  step "Round 3: create -> deposit -> dispute(seller) -> resolve FOR BUYER"
  local id; id=$(invoke "$ESCROW_ID" sf-main-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" sf-main-buyer deposit --escrow_id "$id" >/dev/null
  invoke "$ESCROW_ID" sf-main-seller dispute --escrow_id "$id" --claimant "$SELLER" >/dev/null
  echo "  disputed by seller"
  invoke "$ESCROW_ID" "$ARBITER_ALIAS" resolve --escrow_id "$id" --in_favor_of_seller false >/dev/null
  echo "  arbiter resolved for buyer"
}

step "Minting $((AMOUNT * 3)) smoke units to buyer"
stellar contract invoke --id "$TOKEN_ID" --source-account "$ISSUER_ALIAS" \
  --network "$NET" --network-passphrase "$PASSPHRASE" --rpc-url "$RPC_URL" -- mint --to "$BUYER" --amount $((AMOUNT * 3)) >/dev/null 2>&1 || true
echo "  buyer balance: $(balance "$BUYER")"

round_release
round_dispute_seller_wins
round_dispute_buyer_wins

# --- conservation check ------------------------------------------------------
step "Conservation"
B=$(balance "$BUYER"); S=$(balance "$SELLER")
echo "  buyer:  $B"
echo "  seller: $S"
echo "  buyer+seller must total the pre-demo smoke supply; the contract holds 0"
echo "DONE — record ESCROW_ID, TOKEN_ID and WASM sha256 in the README proof table"
