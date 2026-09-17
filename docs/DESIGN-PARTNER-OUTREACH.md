# Design-Partner Outreach (draft)

Reusable template for approaching campaign-cohort projects about a pilot.
Send as an issue on their repo or a DM, whichever they prefer. Keep it
under 150 words; every claim links evidence; the ask is one round.

---

**Subject: escrow primitive pilot — deployed, receipt-verified, one round to try**

Hi [name/team] — I maintain Soroban Forge, an open-source escrow
primitive on Stellar, deployed and receipt-verified on testnet:

- Contract: `CC227UDF6WBLRTOKKVRIJN7BGSBK67ZGV6IDARJ2AMATGSQ7UZNBZHSB`
- Three live rounds with real SEP-41 movement (release + both dispute
  outcomes), conservation verified on-chain:
  https://github.com/Meet-hybrid/soroban-forge#proof-at-a-glance
- Generated TS client (`@soroban-forge/escrow-client`), lifecycle events,
  arbiter dispute flow, per-record storage with TTL keeping.
  soroban-sdk 27.0.6. 107 tests incl. randomized property suites.

**The ask:** one pilot — a single create → deposit → payout round through
your product's flow, and one public sentence of feedback. Integration is
one client import; you host no custody code and we take no fees.

**What you get:** early say in the interface, a named mention as design
partner, and priority on the feature you need first.

Interested? I'll send a 15-minute integration sketch for your stack.

---

## Notes for the sender (not part of the message)

- Personalize the **first line** per project: reference one concrete
  feature of theirs that maps to escrow (bounties → dispute flow,
  milestones → partial payout roadmap, batch pay → event indexing).
- Send to **2–3 projects max**, then stop; one yes is the goal.
- If someone asks for mainnet/audit status, answer from
  KNOWN-LIMITATIONS.md verbatim: testnet only, audit planned before
  mainnet. Never improvise around it.
- Log the outcome (sent / replied / committed) below so the application
  can name the partner accurately.

| Project | Contact | Sent | Replied | Outcome |
|---|---|---|---|---|
| | | | | |
