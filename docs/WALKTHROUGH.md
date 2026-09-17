# Reviewer walkthrough — verify everything in ~5 minutes

This is the script a reviewer can follow top to bottom. Every step lists
its **expected output** so you know what "working" looks like before you
run it. Wall-clock time: ~8 minutes (most of it is one-time compiles).

**What you will have verified by the end:**

1. The test suite (107 tests, including randomized property tests) passes
   on your machine.
2. The escrow contract moves **real tokens on Stellar testnet** through
   three complete rounds — release, and both dispute outcomes.
3. The on-chain receipts match the [README proof table](../README.md#proof-at-a-glance).

---

## Step 0 — Prerequisites (one-time)

- Rust (stable): https://rustup.rs
- Stellar CLI — a long compile, start it first:

  ```bash
  cargo install stellar-cli --locked
  stellar --version   # expect: stellar 28.x.x (or newer)
  ```

## Step 1 — Clone and run the test suite (~2 min)

```bash
git clone https://github.com/Meet-hybrid/soroban-forge
cd soroban-forge
cargo test --workspace --all-targets --locked
```

**Expected:** the run ends with all crates passing and a line like:

```text
test result: ok. 30 passed; 0 failed; 0 ignored ... (escrow)
test result: ok. ... passed; 0 failed; ...     (each other crate)
```

107 tests total, zero failures. The escrow suite includes the randomized
property tests (`props::…`) — they run in the default `cargo test`.

## Step 2 — Build the contract WASM (~30 s)

```bash
cargo build --locked --release --target wasm32v1-none -p soroban-forge-escrow
```

**Expected:** a clean build and an artifact at
`target/wasm32v1-none/release/soroban_forge_escrow.wasm` (~18 KB).
If you want to check the size budget CI enforces:

```bash
stat -c%s target/wasm32v1-none/release/soroban_forge_escrow.wasm
# expected: under 150000 bytes (ours: ~18000)
```

## Step 3 — Run the live testnet demo (~3 min)

```bash
bash scripts/demo-testnet.sh
```

The script is idempotent: it creates and funds four throwaway testnet
identities (via the friendbot faucet), deploys a demo SAC token and the
escrow contract from *your* freshly built WASM, then runs three rounds.

**Expected output (abridged):**

```text
== Identities ==
  soroban-forge-issuer: G…  soroban-forge-buyer: G…
  soroban-forge-seller: G…  soroban-forge-arbiter: G…
== Demo token (SAC for credit:<issuer>) ==
  token: C…
== Escrow contract ==
  escrow: C…
== Minting 1500 credit to buyer ==
  buyer balance: 1500
== Round: create -> deposit -> release ==
  escrow #0 created
  deposited 500 (buyer -> contract)
  released (contract -> seller)
== Round: create -> deposit -> dispute(buyer) -> resolve FOR SELLER ==
  escrow #1 created … disputed by buyer … arbiter resolved for seller
== Round: create -> deposit -> dispute(seller) -> resolve FOR BUYER ==
  escrow #2 created … disputed by seller … arbiter resolved for buyer
== Conservation ==
  buyer:  500
  seller: 1000
DONE
```

**What to notice:**

- **Conservation is exact**: you minted 1500; afterwards buyer + seller
  hold 500 + 1000 = 1500 and the contract holds nothing — on a live
  network, across all three terminal paths.
- The two dispute rounds resolve in **opposite directions**, proving the
  arbiter's decision is real and not hardcoded.
- Your contract ID will differ from the README's (you deployed your own
  copy — that's the point: the receipts are reproducible by anyone).

## Step 4 — Verify on-chain (2 min, no tools)

Open the round-1 create transaction from the README proof table:

<https://stellar.expert/explorer/testnet/tx/817950c8ad95ecad9636783e5e8e8b8e515f94e3364b63c2c02328ff3b675bb9>

**Expected:** the transaction's event section shows (a) a contract event
from `CC227UDF…4GZX` (`EscrowCreated`) and (b) a SAC `transfer` event of
**500 credit** moving from the buyer to the contract — the on-chain
footprint of `deposit`, visible to anyone.

For your own run, look up your `escrow: C…` id from step 3 in the
[Stellar explorer](https://stellar.expert/explorer/testnet) and check the
same events on your transactions.

## If something bites

| Symptom | Cause / fix |
|---|---|
| `stellar CLI not found` | finish step 0's `cargo install stellar-cli --locked` |
| escrow deploy failed / wasm not found | you skipped step 2 — build the wasm first |
| identity funding errors | friendbot rate-limit; wait a minute and re-run (idempotent) |
| a round fails with `Error(Contract, #11)` | a party lacks a trustline — the script sets them; on custom tokens ensure receivers have trustlines (this is the `TokenTransferFailed` bucket doing its job) |

---

*Honest notes: this demo runs on testnet; the identities it creates are
throwaway demo keys, not protocol fixtures. Full limitations:
[KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md).*
