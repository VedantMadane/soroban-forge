# Contracts Overview

| Contract | Path | Status |
|----------|------|--------|
| Escrow | crates/escrow | **Flagship** — real SEP-41 settlement, disputes, events, persistent storage · 27 tests |
| Vesting | crates/vesting | State machine + tests · **no token settlement** |
| Multi-Sig Wallet | crates/multi-sig-wallet | State machine + tests · no execution dispatch |
| DAO Governance | crates/dao-governance | State machine + tests · executes nothing on-chain |
| Subscription Payments | crates/subscription-payments | State machine + tests · charges nothing |
| Marketplace Royalties | crates/marketplace-royalties | State machine + tests · pays no recipients |

Per-entrypoint detail lives in the [Feature Status Matrix](../FEATURE-STATUS.md);
the aggregate gaps (token settlement, events, storage TTL, deployments) are
documented in [Known Limitations](../KNOWN-LIMITATIONS.md).

## Adding a New Contract

1. Add a new crate under `crates/`.
2. Register it in workspace `Cargo.toml`.
3. Add a document in `docs/contracts/`.
4. Add CI checks.
5. Tag a release.
