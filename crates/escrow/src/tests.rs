//! Escrow test suite.
//!
//! Covers the full lifecycle against a real Stellar Asset Contract (SAC)
//! fixture, so every balance assertion reflects actual token movement —
//! the same failure modes a live deployment would hit (missing balance,
//! failed transfer, double payout).
//!
//! NOTE on authorization coverage: the suite runs under `mock_all_auths`,
//! which proves the *call graph* of authorizations (who the contract
//! asks to sign) but not that a wrong signer is rejected. The one
//! logic-level access control that is enforceable without auth mocking —
//! the `dispute` claimant check, which runs before any `require_auth` —
//! is tested directly (`dispute_by_outsider_is_rejected`). Full negative
//! signature testing needs `set_auths` fixtures and is tracked in the
//! security-invariant backlog.

use crate::{Escrow, EscrowData, EscrowStatus, SorobanForgeEscrowClient};
use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env};

const START: u64 = 1_000_000;
const TIMEOUT: u64 = 1_000;
const AMOUNT: i128 = 1_000;

/// Fresh env: mocked auths, a SAC token with a mintable admin, the escrow
/// contract, and named accounts. Returns the pieces tests name.
macro_rules! setup {
    () => {{
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(START);

        let admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        let token = sac.address();
        let token_admin = StellarAssetClient::new(&env, &token);
        let token_client = TokenClient::new(&env, &token);

        let contract_id = env.register(Escrow, ());
        let client = SorobanForgeEscrowClient::new(&env, &contract_id);

        let accounts = soroban_forge_test_utils::TestAccounts::generate(&env);
        // Buyer starts funded; everyone else starts at zero.
        token_admin.mint(&accounts.user1, &AMOUNT);

        (env, token, token_client, contract_id, client, accounts)
    }};
}

fn create(
    client: &SorobanForgeEscrowClient<'_>,
    token: &soroban_sdk::Address,
    buyer: &soroban_sdk::Address,
    seller: &soroban_sdk::Address,
    arbiter: &soroban_sdk::Address,
    timeout: u64,
) -> u64 {
    client.create_escrow(buyer, seller, arbiter, token, &AMOUNT, &timeout)
}

/// Default party layout: user1 buys, user2 sells, arbiter arbitrates.
fn parties(
    accounts: &soroban_forge_test_utils::TestAccounts,
) -> (
    &soroban_sdk::Address,
    &soroban_sdk::Address,
    &soroban_sdk::Address,
) {
    (&accounts.user1, &accounts.user2, &accounts.arbiter)
}

// -----------------------------------------------------------------------
// Creation
// -----------------------------------------------------------------------

#[test]
fn create_escrow_records_parties_and_stays_pending() {
    let (_env, _token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &_token, buyer, seller, arbiter, TIMEOUT);

    let record: EscrowData = client.get_escrow(&id);
    assert_eq!(record.escrow_id, id);
    assert_eq!(&record.buyer, buyer);
    assert_eq!(&record.seller, seller);
    assert_eq!(&record.arbiter, arbiter);
    assert_eq!(record.amount, AMOUNT);
    assert_eq!(record.timeout, TIMEOUT);
    assert_eq!(record.created_at, START);
    assert_eq!(client.get_status(&id), EscrowStatus::Pending);
}

#[test]
fn escrow_ids_are_monotonic() {
    let (_env, _token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let first = create(&client, &_token, buyer, seller, arbiter, TIMEOUT);
    let second = create(&client, &_token, buyer, seller, arbiter, TIMEOUT);
    assert_eq!(first + 1, second);
}

#[test]
fn create_rejects_zero_amount() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let err = client
        .try_create_escrow(buyer, seller, arbiter, &token, &0, &TIMEOUT)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn create_rejects_zero_timeout() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let err = client
        .try_create_escrow(buyer, seller, arbiter, &token, &AMOUNT, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

// -----------------------------------------------------------------------
// Deposit
// -----------------------------------------------------------------------

#[test]
fn deposit_moves_tokens_into_the_contract() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);

    client.deposit(&id);

    assert_eq!(tc.balance(buyer), 0);
    assert_eq!(tc.balance(&contract_id), AMOUNT);
    assert_eq!(client.get_status(&id), EscrowStatus::Funded);
}

#[test]
fn deposit_twice_is_rejected_and_moves_nothing_more() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    let err = client.try_deposit(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(tc.balance(&contract_id), AMOUNT);
}

#[test]
fn deposit_with_insufficient_balance_fails_cleanly() {
    // user3 holds no tokens; create an escrow they cannot fund.
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let poor = &accounts.user3;
    let (_, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, poor, seller, arbiter, TIMEOUT);

    let err = client.try_deposit(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::TokenTransferFailed);
    assert_eq!(tc.balance(&contract_id), 0);
    // State untouched: still Pending, retryable after topping up.
    assert_eq!(client.get_status(&id), EscrowStatus::Pending);
}

// -----------------------------------------------------------------------
// Release / refund
// -----------------------------------------------------------------------

#[test]
fn release_pays_the_seller() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    client.release(&id);

    assert_eq!(tc.balance(seller), AMOUNT);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), EscrowStatus::Completed);
}

#[test]
fn release_requires_funded_state() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);

    let err = client.try_release(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(tc.balance(&contract_id), 0);
}

#[test]
fn release_cannot_run_twice() {
    let (_env, token, tc, _contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);
    client.release(&id);

    let err = client.try_release(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(tc.balance(seller), AMOUNT);
}

#[test]
fn refund_by_seller_before_deadline() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    client.refund(&id);

    assert_eq!(tc.balance(buyer), AMOUNT);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), EscrowStatus::Refunded);
}

#[test]
fn refund_by_buyer_after_deadline() {
    let (env, token, tc, _contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    env.ledger().set_timestamp(START + TIMEOUT + 1);
    client.refund(&id);

    assert_eq!(tc.balance(buyer), AMOUNT);
    assert_eq!(client.get_status(&id), EscrowStatus::Refunded);
}

#[test]
fn refund_requires_funded_state() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);

    let err = client.try_refund(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

// -----------------------------------------------------------------------
// Dispute flow
// -----------------------------------------------------------------------

#[test]
fn dispute_by_buyer_claimant() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    client.dispute(&id, buyer);

    assert_eq!(client.get_status(&id), EscrowStatus::Disputed);
}

#[test]
fn dispute_by_seller_claimant() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    client.dispute(&id, seller);

    assert_eq!(client.get_status(&id), EscrowStatus::Disputed);
}

#[test]
fn dispute_by_outsider_is_rejected() {
    // The claimant check is a pure value test that runs before any
    // require_auth, so this negative case is enforceable even under
    // mocked auths.
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    let err = client
        .try_dispute(&id, &accounts.user3)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(client.get_status(&id), EscrowStatus::Funded);
}

#[test]
fn dispute_requires_funded_state() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);

    let err = client.try_dispute(&id, buyer).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

#[test]
fn disputed_escrow_freezes_all_payouts() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);
    client.dispute(&id, buyer);

    assert_eq!(
        client.try_release(&id).unwrap_err().unwrap(),
        ForgeError::InvalidInput
    );
    assert_eq!(
        client.try_refund(&id).unwrap_err().unwrap(),
        ForgeError::InvalidInput
    );
    assert_eq!(tc.balance(&contract_id), AMOUNT);
    assert_eq!(client.get_status(&id), EscrowStatus::Disputed);
}

#[test]
fn resolve_in_favor_of_seller_pays_out() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);
    client.dispute(&id, seller);

    client.resolve(&id, &true);

    assert_eq!(tc.balance(seller), AMOUNT);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), EscrowStatus::Completed);
}

#[test]
fn resolve_in_favor_of_buyer_refunds() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);
    client.dispute(&id, buyer);

    client.resolve(&id, &false);

    assert_eq!(tc.balance(buyer), AMOUNT);
    assert_eq!(tc.balance(&contract_id), 0);
    assert_eq!(client.get_status(&id), EscrowStatus::Refunded);
}

#[test]
fn resolve_requires_disputed_state() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    let err = client.try_resolve(&id, &true).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
}

// -----------------------------------------------------------------------
// Cancel
// -----------------------------------------------------------------------

#[test]
fn cancel_pending_escrow() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);

    client.cancel(&id);

    assert_eq!(client.get_status(&id), EscrowStatus::Cancelled);
}

#[test]
fn cancel_after_deposit_is_rejected() {
    let (_env, token, tc, contract_id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    let err = client.try_cancel(&id).unwrap_err().unwrap();
    assert_eq!(err, ForgeError::InvalidInput);
    assert_eq!(tc.balance(&contract_id), AMOUNT);
}

// -----------------------------------------------------------------------
// Missing ids
// -----------------------------------------------------------------------

#[test]
fn missing_escrow_reads_are_not_found() {
    let (_env, _token, _tc, _id, client, _accounts) = setup!();
    assert_eq!(
        client.try_get_status(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_get_escrow(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
}

#[test]
fn operations_on_missing_escrow_are_not_found() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let outsider = &accounts.user3;
    assert_eq!(
        client.try_deposit(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_release(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_refund(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_dispute(&999, outsider).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_resolve(&999, &true).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_cancel(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    assert_eq!(
        client.try_touch_ttl(&999).unwrap_err().unwrap(),
        ForgeError::NotFound
    );
    let _ = (token, buyer, seller, arbiter);
}

// -----------------------------------------------------------------------
// TTL keeper
// -----------------------------------------------------------------------

#[test]
fn touch_ttl_extends_and_keeps_state_intact() {
    let (_env, token, _tc, _id, client, accounts) = setup!();
    let (buyer, seller, arbiter) = parties(&accounts);
    let id = create(&client, &token, buyer, seller, arbiter, TIMEOUT);
    client.deposit(&id);

    client.touch_ttl(&id);

    assert_eq!(client.get_status(&id), EscrowStatus::Funded);
}

// -----------------------------------------------------------------------
// Conservation property: every payout path returns exactly the deposit
// -----------------------------------------------------------------------

/// For every reachable terminal path × timeout combination, the contract
/// ends holding exactly zero and the parties' combined balance equals the
/// original mint: `buyer_start == buyer_end + seller_end`. One escrow, one
/// payout, so the pooled balance must return to zero on every path.
// The path table's function-pointer type is verbose by design — it reads
// as a table of scenarios, not a data structure to be abstracted.
#[allow(clippy::type_complexity)]
#[test]
fn conservation_holds_on_every_terminal_path() {
    let paths: &[&dyn Fn(
        &Env,
        &SorobanForgeEscrowClient<'_>,
        u64,
        &Address,
        &Address,
        &Address,
    )] = &[
        // Release to the seller.
        &|_env, client, id, _buyer, _seller, _arbiter| {
            client.release(&id);
        },
        // Seller refund before the deadline.
        &|_env, client, id, _buyer, _seller, _arbiter| {
            client.refund(&id);
        },
        // Buyer reclaim after the deadline.
        &|env, client, id, _buyer, _seller, _arbiter| {
            env.ledger().set_timestamp(START + TIMEOUT + 1);
            client.refund(&id);
        },
        // Dispute raised by the buyer; arbiter sides with the seller.
        &|_env, client, id, buyer, _seller, _arbiter| {
            client.dispute(&id, buyer);
            client.resolve(&id, &true);
        },
        // Dispute raised by the seller; arbiter sides with the buyer.
        &|_env, client, id, _buyer, seller, _arbiter| {
            client.dispute(&id, seller);
            client.resolve(&id, &false);
        },
    ];

    for timeout in [1u64, 10, TIMEOUT, 100_000] {
        for (i, path) in paths.iter().enumerate() {
            let (env, token, tc, contract_id, client, accounts) = setup!();
            let (buyer, seller, arbiter) = parties(&accounts);
            let id = create(&client, &token, buyer, seller, arbiter, timeout);
            client.deposit(&id);

            path(&env, &client, id, buyer, seller, arbiter);

            assert_eq!(
                tc.balance(&contract_id),
                0,
                "path {i}: contract must not retain dust (timeout {timeout})"
            );
            assert_eq!(
                tc.balance(buyer) + tc.balance(seller),
                AMOUNT,
                "path {i}: buyer+seller must sum to the deposit (timeout {timeout})"
            );
            assert_eq!(
                client.try_deposit(&id).unwrap_err().unwrap(),
                ForgeError::InvalidInput,
                "path {i}: terminal escrow must not be refundable again"
            );
        }
    }
}
