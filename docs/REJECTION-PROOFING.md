# Rejection-Proofing: what must be true before applying again

This document is the operating checklist for not getting rejected twice.
It states what the v0.1.0 rejection was actually about, what is already
fixed, what still has to happen **before** the next application goes in,
and what would get the project rejected again. Every "done" item links
its evidence; every "todo" item names its owner (code = agent/maintainer,
human = only you can do it).

**Status marker:** ✅ done · 🟡 in progress · ❌ not started

---

## 1. Why v0.1.0 was rejected (root causes, not symptoms)

1. **No token settlement** — contracts tracked state and never moved a
   single token. On a payments campaign this alone is disqualifying.
2. **Two major SDK versions behind** — soroban-sdk 21.5.1 pinned to a
   Rust 1.96.0 toolchain that breaks on current stable.
3. **Nothing deployed** — no testnet contract ID, no WASM hash, no
   receipts, no demo.
4. **Oversold breadth** — six contracts advertised as "implemented"
   while flagship features were reserved-but-unreachable.
5. **No honesty artifacts** — gaps scattered in code comments, status
   language inflated, no consolidated limitations doc.

## 2. Root causes → current state

| # | Root cause | Status | Evidence |
|---|---|---|---|
| 1 | No token settlement | ✅ Fixed | Escrow performs real SEP-41 transfers with transfer-before-state ordering; verified on testnet in three rounds |
| 2 | Outdated stack | ✅ Fixed | soroban-sdk 27.0.6, stable Rust, `wasm32v1-none`; escrow WASM 18 KB |
| 3 | Nothing deployed | ✅ Fixed | Testnet contract `CC227UDF…4GZX`, WASM sha256, explorer links, `scripts/demo-testnet.sh` — README "Proof at a glance" |
| 4 | Oversold breadth | ✅ Fixed | FEATURE-STATUS.md labels every entrypoint; other five honestly "state machine" |
| 5 | No honesty artifacts | ✅ Fixed | KNOWN-LIMITATIONS.md, FEATURE-STATUS.md, GRANT-APPLICATION.md; a wrong CHANGELOG claim was corrected publicly when CI caught it |
| — | *(new)* Shallow testing vs. accepted cohort | ✅ Fixed | proptest suite: conservation over random paths, tamper-resilient pool conservation, fund safety over arbitrary call sequences (escrow 30 tests, workspace 107) |

## 3. Blocking items — do these before submitting

### 3.1 Named design partner (human) ❌ — **the single highest-leverage item**

Sub Rosa's public resubmission playbook is explicit: a *named* pilot or
design partner is what reviewers look for. No amount of code replaces
this line.

- [ ] Pick 2–3 campaign-cohort projects whose products would embed an
      escrow primitive (bounty, milestone/freelance, batch-payment
      projects are natural fits).
- [ ] Send the outreach (draft: `docs/DESIGN-PARTNER-OUTREACH.md`).
- [ ] Get **one** commitment: a single create→deposit→payout pilot round
      plus one public sentence of feedback. That is the whole ask.

### 3.2 Demo reproducibility from a clean machine (human verify) 🟡

- [ ] On a machine (or container) that is not this one: clone, install
      the stellar CLI, run `bash scripts/demo-testnet.sh`, confirm three
      rounds complete. Fix whatever bites. Reviewers *will* try this.

### 3.3 Application packaging (human) ❌

- [ ] Fill the relevant funding application form from `docs/GRANT-APPLICATION.md`, linking
      the README proof table.
- [ ] Tranche-scoped ask only (escrow hardening + indexer + shipped SDK
      client, 4 weeks). Do not ask for the other five contracts.
- [ ] Name the design partner from 3.1 before submitting.

## 4. Non-blocking but scheduled (say so in the application)

These are **deliverables inside the ask**, not prerequisites. Doing them
before submitting adds weeks for zero review benefit.

- Negative authorization test coverage (`set_auths` fixtures) — tranche 1
- Event indexer service ("show me my escrows") — tranche 1
- SDK client test suite + npm publish — tranche 1
- Vesting settlement + TTL keeper — tranche 2
- Multi-sig execution dispatch — tranche 3
- Mainnet deployment + external audit — post-award, never before

## 5. What would get it rejected again

1. **Re-inflating the six contracts.** Ever again describe the five
   state machines as "implemented." Flagship-first framing, always.
2. **Unverified claims.** The RUSTSEC-2026-0009 incident (a CHANGELOG
   claim about the lockfile that CI proved false) is the cautionary
   example: every claim in the application must have a link, a hash, or
   a command behind it. If you cannot point at evidence, delete it.
3. **Process theater.** No manufactured community signals. Solo
   maintainer is stated honestly; contributor interest is shown through
   the scoped issue backlog, not roleplay.
4. **Resubmitting before the demo is re-runnable** (3.2). One broken
   quickstart undoes the entire credibility story.
5. **Skipping the partner line** (3.1). An application without a pilot
   is a weaker application than its own repo deserves.
6. **Letting CI go red.** All 7 checks green on `f9459ce` is the new
   floor; a red main branch at application time is a silent rejection.

## 6. Pre-submit checklist (run top to bottom, in order)

- [ ] `cargo build --workspace --all-targets --locked` green
- [ ] `cargo test --workspace --all-targets --locked` green (107 tests;
      proptest suite included)
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` green
- [ ] `cargo fmt --all -- --check` green
- [ ] `cargo audit` warnings-only (no failing advisories, no ignores)
- [ ] `cargo build --locked --release --target wasm32v1-none -p soroban-forge-escrow` + size within budget
- [ ] All 7 GitHub check runs green on latest `main`
- [ ] `scripts/demo-testnet.sh` re-run from a clean environment
- [ ] README proof table regenerated against the current deployment
      (contract ID, WASM hash, tx links)
- [ ] FEATURE-STATUS.md and KNOWN-LIMITATIONS.md re-verified against the
      current code (no stale claims)
- [ ] Design partner named in GRANT-APPLICATION.md
- [ ] Tranche ask matches RESUBMISSION.md §Phase-1/2 deliverables
- [ ] Final read of the application as a reviewer: every sentence either
      links evidence or states a limitation

---

**Bottom line:** the code gaps that caused the rejection are closed and
evidenced. What remains is one human action (a named design partner),
one verification action (re-run the demo from a clean machine), and the
discipline to submit without inflating anything.
