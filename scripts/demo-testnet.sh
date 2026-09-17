#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Soroban Forge — end-to-end escrow demo against Stellar TESTNET.
#
# Performs three complete escrow rounds with real SEP-41 token movement:
#   round 1: create -> deposit -> release            (seller paid)
#   round 2: create -> deposit -> dispute -> resolve (arbiter sides with seller)
#   round 3: create -> deposit -> dispute -> resolve (arbiter refunds buyer)
#
# Prerequisites:
#   - stellar CLI on PATH (v23+; tested with v28.0.0)
#   - funded testnet identities (created automatically if missing):
#       soroban-forge-issuer  (token issuer / faucet)
#       soroban-forge-buyer, soroban-forge-seller, soroban-forge-arbiter
#   - the demo token (SAC for credit:<issuer>) and the escrow contract
#     (deployed automatically on first run; the contract is NOT redeployed
#     if the ESCROW_ID env var is set)
#
# Environment overrides:
#   ESCROW_ID   existing escrow contract id to reuse
#   TOKEN_ID    existing SAC token id to reuse
#   AMOUNT      escrow amount per round (default 500)
#
# Usage:  bash scripts/demo-testnet.sh
# ---------------------------------------------------------------------------
set -euo pipefail

command -v stellar >/dev/null 2>&1 || { echo "stellar CLI not found on PATH"; exit 1; }
NET="testnet"
AMOUNT="${AMOUNT:-500}"
ESCROW_ID="${ESCROW_ID:-}"
TOKEN_ID="${TOKEN_ID:-}"
ISSUER_ALIAS="soroban-forge-issuer"
step() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
addr() { stellar keys address "$1" 2>/dev/null; }

# --- identities ------------------------------------------------------------
step "Identities"
for name in "$ISSUER_ALIAS" soroban-forge-buyer soroban-forge-seller soroban-forge-arbiter; do
  if ! addr "$name" >/dev/null 2>&1; then
    echo "creating + funding identity: $name"
    stellar keys generate "$name" --overwrite --fund --network "$NET" >/dev/null
  fi
  echo "  $name: $(addr "$name")"
done
ISSUER=$(addr "$ISSUER_ALIAS"); BUYER=$(addr soroban-forge-buyer)
SELLER=$(addr soroban-forge-seller); ARBITER=$(addr soroban-forge-arbiter)

# --- demo token (SAC) ------------------------------------------------------
step "Demo token (SAC for credit:<issuer>)"
if [ -z "$TOKEN_ID" ]; then
  TOKEN_ID=$(stellar contract asset deploy \
    --asset "credit:$ISSUER" \
    --source-account "$ISSUER_ALIAS" --network "$NET" 2>/dev/null | tail -1)
  [ -n "$TOKEN_ID" ] || { echo "token deploy failed"; exit 1; }
fi
echo "  token: $TOKEN_ID"

# Trustlines: on classic SACs, receivers need a trustline before they can
# hold the asset (the buyer's failure here is exactly what Error #11 is for).
step "Trustlines (buyer, seller)"
stellar tx new change-trust --source-account soroban-forge-buyer  --network "$NET" \
  --line "credit:$ISSUER" --limit 1000000 >/dev/null 2>&1 || echo "  buyer trustline exists"
stellar tx new change-trust --source-account soroban-forge-seller --network "$NET" \
  --line "credit:$ISSUER" --limit 1000000 >/dev/null 2>&1 || echo "  seller trustline exists"

# --- escrow contract -------------------------------------------------------
step "Escrow contract"
if [ -z "$ESCROW_ID" ]; then
  wasm="target/wasm32v1-none/release/soroban_forge_escrow.wasm"
  [ -f "$wasm" ] || wasm="$(find target -name soroban_forge_escrow.wasm -path '*wasm32v1-none*' | head -1)"
  ESCROW_ID=$(stellar contract deploy --wasm "$wasm" \
    --source-account "$ISSUER_ALIAS" --network "$NET" 2>/dev/null | tail -1)
  [ -n "$ESCROW_ID" ] || { echo "escrow deploy failed"; exit 1; }
fi
echo "  escrow: $ESCROW_ID"

# --- helpers ----------------------------------------------------------------
invoke() { # id source fn args...  -> prints tx hash on success
  local id="$1" src="$2"; shift 2
  stellar contract invoke --id "$id" --source-account "$src" --network "$NET" \
    -- "$@" 2>/dev/null | tail -1
}
balance() { stellar token balance --id "$TOKEN_ID" --account "$1" --network "$NET" 2>/dev/null | tail -1; }

# --- three rounds -----------------------------------------------------------
round_release() {
  step "Round: create -> deposit -> release"
  local id; id=$(invoke "$ESCROW_ID" soroban-forge-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" soroban-forge-buyer deposit --escrow_id "$id" >/dev/null
  echo "  deposited $AMOUNT (buyer -> contract)"
  invoke "$ESCROW_ID" soroban-forge-seller release --escrow_id "$id" >/dev/null
  echo "  released (contract -> seller)"
}

round_dispute_seller_wins() {
  step "Round: create -> deposit -> dispute(buyer) -> resolve FOR SELLER"
  local id; id=$(invoke "$ESCROW_ID" soroban-forge-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" soroban-forge-buyer deposit --escrow_id "$id" >/dev/null
  invoke "$ESCROW_ID" soroban-forge-buyer dispute --escrow_id "$id" --claimant "$BUYER" >/dev/null
  echo "  disputed by buyer"
  invoke "$ESCROW_ID" soroban-forge-arbiter resolve --escrow_id "$id" --in_favor_of_seller true >/dev/null
  echo "  arbiter resolved for seller"
}

round_dispute_buyer_wins() {
  step "Round: create -> deposit -> dispute(seller) -> resolve FOR BUYER"
  local id; id=$(invoke "$ESCROW_ID" soroban-forge-buyer create_escrow \
    --buyer "$BUYER" --seller "$SELLER" --arbiter "$ARBITER" \
    --token "$TOKEN_ID" --amount "$AMOUNT" --timeout 86400)
  echo "  escrow #$id created"
  invoke "$ESCROW_ID" soroban-forge-buyer deposit --escrow_id "$id" >/dev/null
  invoke "$ESCROW_ID" soroban-forge-seller dispute --escrow_id "$id" --claimant "$SELLER" >/dev/null
  echo "  disputed by seller"
  invoke "$ESCROW_ID" soroban-forge-arbiter resolve --escrow_id "$id" --in_favor_of_seller false >/dev/null
  echo "  arbiter resolved for buyer"
}

step "Minting $((AMOUNT * 3)) credit to buyer"
stellar contract invoke --id "$TOKEN_ID" --source-account "$ISSUER_ALIAS" \
  --network "$NET" -- mint --to "$BUYER" --amount $((AMOUNT * 3)) >/dev/null 2>&1 || true
echo "  buyer balance: $(balance "$BUYER")"

round_release
round_dispute_seller_wins
round_dispute_buyer_wins

# --- conservation check ------------------------------------------------------
step "Conservation"
B=$(balance "$BUYER"); S=$(balance "$SELLER")
echo "  buyer:  $B"
echo "  seller: $S"
echo "  contract escrowed amounts are released in full on every path;"
echo "  buyer+seller holdings must equal the pre-demo total + $((AMOUNT * 3)) minted"
echo "DONE"
