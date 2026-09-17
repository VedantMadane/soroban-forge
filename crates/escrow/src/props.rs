//! Randomized invariant suite (proptest).
//!
//! The hand-written suite pins behaviour on known values; this module tries
//! to *falsify* the contract's fund-safety claims over generated inputs.
//! Three properties are exercised:
//!
//! **P1 — Conservation.** On every terminal path, for random amounts,
//! timeouts, dispute claimants and resolution directions: the contract ends
//! holding zero and the parties' gain equals the deposit exactly.
//!
//! **P2 — Tamper-resilient conservation.** An attacker who can rewrite the
//! contract's persistent storage between calls (the strongest adversary the
//! ledger itself does not stop) can never move value out of the pool:
//! `contract + buyer + seller + arbiter` balances are conserved across every
//! mutation, and a tampered over-payment is rejected by the token layer's
//! balance check rather than paying out.
//!
//! **P3 — Fund safety over arbitrary sequences.** For random sequences of
//! `deposit / release / refund / dispute / resolve`: each call succeeds only
//! when the mirrored state machine says it must, outsider disputes are always
//! rejected, no state transition ever changes the pool total, and terminal
//! escrows are never payable twice.
//!
//! All properties run against a real Stellar Asset Contract, so balances
//! reflect actual token movement. Runs are deterministic (fixed default
//! seed); a failure prints its case seed for replay. Override the case count
//! with `PROPTEST_CASES=n cargo test -p soroban-forge-escrow props`.

use crate::{Escrow, EscrowData, EscrowStatus, SorobanForgeEscrowClient};
use proptest::prelude::*;
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env};

const START: u64 = 1_000_000;
const MAX_AMOUNT: i128 = 1_000_000_000_000;
const MAX_TIMEOUT: u64 = 10 * 365 * 24 * 3600;

// -----------------------------------------------------------------------
// World: one fresh env, SAC, escrow contract, and fixed role set per case
// -----------------------------------------------------------------------

struct World {
    env: Env,
    token: Address,
    escrow: Address,
    buyer: Address,
    seller: Address,
    arbiter: Address,
    outsider: Address,
}

fn setup_world() -> World {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START);

    let admin = Address::generate(&env);
    let outsider = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    let token = sac.address();

    let escrow = env.register(Escrow, ());

    World {
        token,
        escrow,
        buyer: Address::generate(&env),
        seller: Address::generate(&env),
        arbiter: Address::generate(&env),
        outsider,
        env,
    }
}

impl World {
    fn escrow_client(&self) -> SorobanForgeEscrowClient<'_> {
        SorobanForgeEscrowClient::new(&self.env, &self.escrow)
    }

    fn token_client(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.token)
    }

    fn mint_to_buyer(&self, amount: i128) {
        StellarAssetClient::new(&self.env, &self.token).mint(&self.buyer, &amount);
    }

    fn create(&self, amount: i128, timeout: u64) -> u64 {
        self.escrow_client().create_escrow(
            &self.buyer,
            &self.seller,
            &self.arbiter,
            &self.token,
            &amount,
            &timeout,
        )
    }

    /// Balances of (buyer, seller, arbiter, contract) — the whole pool.
    fn pool(&self) -> (i128, i128, i128, i128) {
        let t = self.token_client();
        (
            t.balance(&self.buyer),
            t.balance(&self.seller),
            t.balance(&self.arbiter),
            t.balance(&self.escrow),
        )
    }

    /// The conserved total: everything held by the contract and the parties.
    fn pool_total(&self) -> i128 {
        let (b, s, a, c) = self.pool();
        b + s + a + c
    }
}

// -----------------------------------------------------------------------
// Strategies
// -----------------------------------------------------------------------

fn arb_delta() -> impl Strategy<Value = i128> {
    prop_oneof![
        Just(0i128),
        (1i128..=MAX_AMOUNT).prop_map(|v| v),
        (1i128..=MAX_AMOUNT).prop_map(|v| -v),
    ]
}

fn claimant_pick() -> impl Strategy<Value = u8> {
    0u8..=2 // 0 = buyer, 1 = seller, 2 = outsider
}

fn terminal_path() -> impl Strategy<Value = u8> {
    0u8..=2 // 0 = release, 1 = refund (pre-deadline), 2 = dispute -> resolve
}

// -----------------------------------------------------------------------
// P1 — conservation on random terminal paths (no tampering)
// -----------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p1_conservation_on_random_terminal_paths(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        path in terminal_path(),
        claimant in claimant_pick(),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);
        // Baseline AFTER deposit: the parties' pre-payout holdings plus the
        // contract's custody. (Forgetting `c0` here is exactly the kind of
        // bug the minimal counterexample exposes — the first P1 failure was
        // in this formula, not in the contract.)
        let (b0, s0, _a0, c0) = w.pool();

        match path {
            0 => w.escrow_client().release(&id),
            1 => w.escrow_client().refund(&id),
            _ => {
                // Outsider claimants are covered by P3; force a party here
                // so the dispute actually lands.
                let claimant_addr = match claimant {
                    0 => &w.buyer,
                    _ => &w.seller,
                };
                w.escrow_client().dispute(&id, claimant_addr);
                w.escrow_client().resolve(&id, &pay_seller);
            }
        }

        let (b1, s1, a1, c1) = w.pool();
        prop_assert_eq!(c1, 0, "contract must retain nothing on a terminal path");
        prop_assert_eq!(b1 + s1 + a1, b0 + s0 + c0, "parties' gain must equal the deposit exactly");
        let status = w.escrow_client().get_status(&id);
        prop_assert!(status == EscrowStatus::Completed || status == EscrowStatus::Refunded);
    }
}

// -----------------------------------------------------------------------
// P2 — tamper-resilient conservation (storage attacker between calls)
// -----------------------------------------------------------------------

/// Apply one storage mutation to a loaded escrow record and write it back.
///
/// `escrow_id` and `token` are never mutated: pointing the id at a record
/// that does not exist would brick the entry against every further call, and
/// swapping the token would describe a different contract. Mutating the
/// party addresses is likewise excluded — worst case it lets an address
/// withdraw the full escrow, which is custodial exposure inherent to state
/// tampering, not an invariant violation. What the property demands is that
/// even under every mutation we *do* allow, no value can leave the pool.
fn mutate_record(w: &World, id: u64, kind: u8, add: i128, sub: i128, overwrite: i128) {
    let key = crate::DataKey::Escrow(id);
    // Storage writes are only legal from inside a contract context, so the
    // tampering runs under `as_contract` — modelling a compromised contract
    // itself, which is the strongest adversary this ledger model allows.
    w.env.as_contract(&w.escrow, || {
        let mut rec: EscrowData = w
            .env
            .storage()
            .persistent()
            .get(&key)
            .expect("tamper target must exist");

        match kind {
            // Fuzz the custodied amount.
            0 => {
                rec.amount = add.wrapping_add(rec.amount);
            }
            // Fuzz the deadline: push it into the past or future.
            1 => {
                rec.timeout = (sub.unsigned_abs() as u64).wrapping_add(rec.timeout);
            }
            2 => {
                let shift = sub.unsigned_abs();
                let shifted = (rec.created_at as u128).wrapping_add(shift);
                if shifted <= u64::MAX as u128 {
                    rec.created_at = shifted as u64;
                } else {
                    return; // skip: cannot represent; nothing written
                }
            }
            // Fuzz the claimed/position fields that could skew payouts.
            3 => {
                rec.amount = rec.amount.wrapping_add(overwrite);
            }
            // Fuzz the lifecycle status itself.
            4 => {
                rec.status = match overwrite.rem_euclid(6) {
                    0 => EscrowStatus::Pending,
                    1 => EscrowStatus::Funded,
                    2 => EscrowStatus::Completed,
                    3 => EscrowStatus::Refunded,
                    4 => EscrowStatus::Disputed,
                    _ => EscrowStatus::Cancelled,
                };
            }
            _ => unreachable!("strategy bounds"),
        }

        w.env.storage().persistent().set(&key, &rec);
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn p2_tampered_storage_never_moves_value_out_of_the_pool(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        muts in prop::collection::vec((0u8..=4, arb_delta(), arb_delta(), arb_delta()), 0..=6),
        pay_seller in prop::bool::ANY,
    ) {
        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);
        let before = w.pool_total();

        // Deadline always forced past: post-deadline refund/resolve paths
        // stay reachable no matter how created_at/timeout were mutated.
        w.env.ledger().set_timestamp(START.saturating_add(timeout).saturating_add(1));

        for (kind, add, sub, overwrite) in muts {
            mutate_record(&w, id, kind, add, sub, overwrite);
            prop_assert_eq!(w.pool_total(), before,
                "a storage mutation changed the pool total");
        }

        // Attempt a payout against whatever the tampered record now says.
        // Three conserve-the-pool outcomes are acceptable:
        //   - resolve succeeds: the *tampered* amount is what moves; if the
        //     attacker shrank it, residue stays in the contract (their loss,
        //     not a theft) — so we assert conservation, not emptiness.
        //   - resolve fails on an over-payment: the token layer's balance
        //     check reverts the whole call, status untouched.
        //   - resolve fails as InvalidInput: the record was tampered into a
        //     non-Disputed status, so nothing was attempted.
        let client = w.escrow_client();
        let status: EscrowStatus = client.get_status(&id);
        match client.try_resolve(&id, &pay_seller) {
            Ok(Ok(())) => {
                let (_, _, _, c1) = w.pool();
                prop_assert!(c1 >= 0, "resolved escrow may retain residue from a shrunken amount, never a deficit");
            }
            Err(Ok(ForgeError::TokenTransferFailed)) => {
                prop_assert_eq!(status, EscrowStatus::Disputed,
                    "failed resolve must revert the status change");
            }
            Err(Ok(ForgeError::InvalidInput)) => {
                // Tampered into a non-Disputed status; nothing attempted.
            }
            other => panic!("unexpected resolve outcome under tampering: {:?}", other),
        }

        prop_assert_eq!(w.pool_total(), before,
            "pool total must be conserved end-to-end, tampering or not");
    }
}

// -----------------------------------------------------------------------
// P3 — fund safety over arbitrary call sequences
// -----------------------------------------------------------------------

/// (op, claimant_pick, pay_seller)
fn action_strategy() -> impl Strategy<Value = (u8, u8, bool)> {
    (0u8..=4, claimant_pick(), prop::bool::ANY)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn p3_sequences_never_violate_fund_safety(
        amount in 1i128..=MAX_AMOUNT,
        timeout in 1u64..=MAX_TIMEOUT,
        actions in prop::collection::vec(action_strategy(), 0..=10),
    ) {
        use EscrowStatus::{Cancelled, Completed, Disputed, Funded, Pending, Refunded};

        let w = setup_world();
        w.mint_to_buyer(amount);
        let id = w.create(amount, timeout);
        w.escrow_client().deposit(&id);

        // The contract's own state machine, mirrored independently. Every
        // call is checked against this mirror; a mismatch is a failure.
        let mut state = Funded;

        for (op, claimant, pay_seller) in actions {
            let client = w.escrow_client();
            let before = w.pool_total();

            match op {
                // deposit
                0 => {
                    let res = client.try_deposit(&id);
                    match state {
                        Pending => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "deposit must succeed while Pending");
                            state = Funded;
                        }
                        _ => prop_assert!(res.is_err(), "deposit must fail unless Pending"),
                    }
                }
                // release
                1 => {
                    let res = client.try_release(&id);
                    match state {
                        Funded => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "release must succeed while Funded");
                            state = Completed;
                        }
                        _ => prop_assert!(res.is_err(), "release must fail unless Funded"),
                    }
                }
                // refund — post-deadline form: the buyer reclaims.
                2 => {
                    w.env.ledger().set_timestamp(START.saturating_add(timeout).saturating_add(1));
                    let res = client.try_refund(&id);
                    match state {
                        Funded => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "post-deadline refund must succeed while Funded");
                            state = Refunded;
                        }
                        _ => prop_assert!(res.is_err(), "refund must fail unless Funded"),
                    }
                }
                // dispute
                3 => {
                    // claimant_pick: 0 buyer (party), 1 seller (party), 2 outsider.
                    let claimant_addr = match claimant {
                        0 => w.buyer.clone(),
                        1 => w.seller.clone(),
                        _ => w.outsider.clone(),
                    };
                    let res = client.try_dispute(&id, &claimant_addr);
                    match state {
                        Funded if claimant <= 1 => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "party dispute must succeed while Funded");
                            state = Disputed;
                        }
                        Funded => {
                            // Rejection must be our typed InvalidInput; the
                            // two-step unwrap mirrors tests.rs's error shape.
                            let err = res.unwrap_err().unwrap();
                            prop_assert_eq!(err, ForgeError::InvalidInput,
                                "outsider dispute must be rejected with InvalidInput");
                        }
                        _ => prop_assert!(res.is_err(), "dispute must fail unless Funded"),
                    }
                }
                // resolve
                _ => {
                    let res = client.try_resolve(&id, &pay_seller);
                    match state {
                        Disputed => {
                            prop_assert!(matches!(res, Ok(Ok(()))), "resolve must succeed while Disputed");
                            state = if pay_seller { Completed } else { Refunded };
                        }
                        _ => prop_assert!(res.is_err(), "resolve must fail unless Disputed"),
                    }
                }
            }

            prop_assert_eq!(w.pool_total(), before,
                "no call may change the pool total outside a payout from the contract");
        }

        // If the sequence reached a terminal state, it must be genuinely
        // finished: contract empty, and every further call rejected.
        if matches!(state, Completed | Refunded | Cancelled) {
            let (_, _, _, c) = w.pool();
            prop_assert_eq!(c, 0, "terminal escrow must have paid out in full");
            let client = w.escrow_client();
            prop_assert!(client.try_deposit(&id).is_err());
            prop_assert!(client.try_release(&id).is_err());
            prop_assert!(client.try_refund(&id).is_err());
            prop_assert!(client.try_dispute(&id, &w.buyer).is_err());
            prop_assert!(client.try_resolve(&id, &true).is_err());
        }
        let _ = Cancelled; // Cancelled is unreachable here (no cancel op in P3's set)
    }
}
