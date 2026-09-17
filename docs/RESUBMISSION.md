# Resubmission Plan & Narrative

> **Status (updated):** Phase 1 ✅ complete — workspace on soroban-sdk
> 27.0.6 / stable Rust, escrow rebuilt with SEP-41 settlement, dispute
> flow, events, persistent storage + TTL, 104 tests green. Phase 2 ✅
> complete — escrow deployed on testnet with a verified three-round
> receipt and on-chain conservation; see the README proof table and
> `scripts/demo-testnet.sh`. Phase 3 (application narrative + design
> partner) remains open.

This is the working plan for resubmitting to an ecosystem funding campaign after
the rejection of the v0.1.0 application, written **before** code changes
so the ask and the deliverables are fixed first. It follows the
resubmission pattern Sub Rosa published publicly: named deliverables per
tranche, proof artifacts, honest limitations, and a capped scope.

## Why the first application was rejected (own assessment)

The application proposed six contracts pitched as a finished foundations
library. Reviewer-visible reality:

1. **No token settlement** — contracts track state, never move value. On
   a payments campaign this is disqualifying on its own.
2. **Two major SDK versions behind** — soroban-sdk 21.5.1 pinned to a
   Rust 1.96.0 toolchain that breaks on current stable.
3. **Nothing deployed** — no testnet contract IDs, no WASM hashes, no
   runnable demo.
4. **Six contracts × 20% deep** — reserved-but-unreachable states
   (`Disputed`, `Revoked`) where the flagship features should be.
5. **No honesty artifact** — the gaps were scattered across code
   comments instead of consolidated; the status language
   ("implemented") oversold state-only entrypoints.

What the accepted cohort shows (Fluxora, Talenttrust, Sub Rosa):
one deep primitive, real SEP-41 movement with documented ordering,
deployed proof with receipts, a formal limitations doc, and a
tranche-scoped ask.

## Repositioning

**From:** "community-driven library of six reusable contracts"
**To:** "solo-maintained foundations project shipping one
production-grade escrow primitive on current SDK, with five more
contracts queued behind it as follow-up tranches."

The README framing change is part of Phase 3. The scoped issue backlog
stays — it is the strongest contributor asset in the repo.

---

## Phase 1 — Flagship escrow primitive (≈3 weeks)

**Goal:** one contract a reviewer would hold real USDC in.

| Deliverable | Detail |
|---|---|
| SDK 27 port | Workspace on soroban-sdk 27.x, current stable Rust, `wasm32v1-none` target where applicable |
| SEP-41 settlement | `deposit`/`release`/`refund` perform real `TokenClient::transfer`; **transfer-before-state-write ordering** so a failed transfer never corrupts accounting |
| Arbiter dispute path | `dispute` (buyer or seller) and `resolve` (arbiter, releases or refunds) — makes the stored `arbiter` reachable and `Disputed` live |
| Events | `escrow_created`, `deposited`, `released`, `refunded`, `disputed`, `resolved`, `cancelled` |
| Storage | Per-escrow **persistent** entries; instance storage only for the id counter; explicit TTL extension on every write |
| Property tests | `deposited == released + refunded` invariant at every reachable state; monotonicity across call orderings; negative auth tests re-landed on SDK 27 |
| Honesty docs | `KNOWN-LIMITATIONS.md` and `FEATURE-STATUS.md` updated to the new state |

**Explicitly out of Phase 1:** the other five contracts' settlement work,
weighted voting, plan management, multi-recipient royalties, audit.

## Phase 2 — Proof (≈1 week)

| Deliverable | Detail |
|---|---|
| Testnet deployment | Escrow deployed to Stellar testnet; **contract ID + WASM hash published in the README** |
| Receipts | A scripted `create → deposit → release` round on testnet with transaction hashes in the README (Sub Rosa's "proof at a glance" pattern) |
| Runnable demo | One copy-paste script: `make demo` performs a full lifecycle against the deployed contract |
| Real TS bindings | Generated bindings for the escrow contract only; the stub SDK is replaced or deleted |
| Demo walkthrough | Short recorded or scripted walkthrough a reviewer can follow in five minutes |

## Phase 3 — Resubmit (≈2–3 days)

| Deliverable | Detail |
|---|---|
| Repositioned README | Solo-maintained, flagship-first, five contracts queued honestly; delete "community-driven" until external contributions exist |
| Application narrative | This document, updated to past tense with links |
| Tranche-scoped ask | See below |

### The ask (tranche 1)

- **Scope:** the Phase 1 + Phase 2 deliverables, nothing more.
- **Success criteria:** deployed escrow holds and moves a real testnet
  USDC round-trip end-to-end; property suite proves the conservation
  invariant; receipts public.
- **Roadmap after:** tranche 2 = vesting settlement + TTL keeper; tranche
  3 = multi-sig execution dispatch; each scoped the same way.
- **Design partner:** target at least one project from the campaign
  cohort (streaming/payments projects need escrow primitives) to commit
  to an integration attempt. This is the highest-leverage single line in
  the application.

---

## Reviewer-response checklist (pre-submit)

- [ ] `cargo test --workspace --all-targets --locked` green on current stable Rust
- [ ] Property suite + negative auth tests green
- [ ] Testnet contract ID + WASM hash in README
- [ ] `make demo` runs end-to-end from a clean clone
- [ ] `KNOWN-LIMITATIONS.md` and `FEATURE-STATUS.md` re-verified against the new code
- [ ] No claim in the README that the repo cannot demonstrate
- [ ] Git identity normalized (`.mailmap`) so contributor counts are accurate
