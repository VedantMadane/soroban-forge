#![no_std]

//! # Soroban Forge — Vesting contract
//!
//! A token-vesting contract that releases a beneficiary's tokens linearly
//! over time, optionally behind a cliff.
//!
//! Timings are expressed as **durations in seconds measured from the schedule
//! start** (the ledger timestamp recorded at creation):
//!
//! ```text
//! start ......... start+cliff ................... start+duration
//!   |             (claims become possible)        (fully vested)
//!   |  Locked    |            Vesting (linear)  |
//! ```
//!
//! The vested amount at ledger time `t` is:
//! - `0` when `t < start + cliff`,
//! - `total_amount` when `t >= start + duration`,
//! - otherwise `total_amount * (t - (start + cliff)) / (duration - cliff)`,
//!   using integer (floor) division so claims never round up.
//!
//! Authorization model:
//! - `create_schedule` requires the beneficiary.
//! - `claim` requires the beneficiary.
//! - `claimable` and `get_status` are read-only views.
//!
//! The `Revoked` status is reserved for a revocation method that lands in a
//! follow-up; it is not reachable through the current public interface. Token
//! settlement (SAC transfers) is intentionally out of scope for this
//! iteration: the contract tracks state and authorization, not balances.

#[cfg(test)]
extern crate std;

use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::{contract, contractclient, contractimpl, contracttype, Address, Env};

/// Public interface for the Soroban Forge vesting contract.
///
/// Declared as a `contractclient` trait so SDK consumers (and the TypeScript
/// SDK generator) get a strongly-typed client without coupling to the
/// implementation crate.
#[contractclient(name = "SorobanForgeVestingClient")]
pub trait SorobanForgeVesting {
    /// Create a new vesting schedule for `beneficiary`.
    ///
    /// `cliff` and `duration` are seconds measured from creation
    /// (`cliff <= duration`, `duration > 0`, `total_amount > 0`). Returns the
    /// stable schedule id.
    fn create_schedule(
        env: Env,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        cliff: u64,
        duration: u64,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Claim tokens that have vested as of the current ledger time.
    ///
    /// Requires the beneficiary. Returns the exact vested-but-unclaimed
    /// amount, or `0` when there is nothing to claim.
    fn claim(env: Env, schedule_id: u64) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Return the amount currently claimable by `schedule_id` (read-only).
    fn claimable(
        env: Env,
        schedule_id: u64,
    ) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Read the current lifecycle status of `schedule_id` (read-only).
    fn get_status(
        env: Env,
        schedule_id: u64,
    ) -> Result<VestingStatus, soroban_forge_shared_utils::ForgeError>;
}

/// Lifecycle state of a vesting schedule.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VestingStatus {
    /// Before the cliff has been reached.
    Locked,
    /// Past the cliff; tokens are vesting linearly.
    Vesting,
    /// Fully vested and claimed.
    Completed,
    /// Schedule was terminated before completion (reserved).
    Revoked,
}

/// A single token-vesting schedule.
#[contracttype]
#[derive(Clone, Debug)]
pub struct VestingSchedule {
    /// Recipient of the vested tokens.
    pub beneficiary: Address,
    /// Token contract whose balance is drawn down.
    pub token: Address,
    /// Total amount to vest linearly between `cliff` and `duration`.
    pub total_amount: i128,
    /// Ledger timestamp at which vesting begins (creation time).
    pub start: u64,
    /// Seconds after `start` at which claims become possible.
    pub cliff: u64,
    /// Seconds after `start` at which the schedule is fully vested.
    pub duration: u64,
    /// Amount already claimed by the beneficiary.
    pub claimed: i128,
    /// Current lifecycle state.
    pub status: VestingStatus,
}

/// Instance-storage keys.
#[contracttype]
enum DataKey {
    /// The vesting record for `u64` id.
    Schedule(u64),
    /// Monotonic id counter.
    Count,
}

/// The deployable vesting contract.
#[contract]
pub struct Vesting;

#[contractimpl]
impl Vesting {
    /// Create a new vesting schedule and return its stable id.
    ///
    /// Requires `total_amount > 0`, `duration > 0`, and `cliff <= duration`.
    /// The beneficiary is authorized at creation time.
    pub fn create_schedule(
        env: Env,
        beneficiary: Address,
        token: Address,
        total_amount: i128,
        cliff: u64,
        duration: u64,
    ) -> Result<u64, ForgeError> {
        if total_amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        if duration == 0 {
            return Err(ForgeError::InvalidInput);
        }
        if cliff > duration {
            return Err(ForgeError::InvalidInput);
        }
        beneficiary.require_auth();

        let id = Self::next_id(&env)?;
        let start = env.ledger().timestamp();
        let mut schedule = VestingSchedule {
            beneficiary,
            token,
            total_amount,
            start,
            cliff,
            duration,
            claimed: 0,
            status: VestingStatus::Locked,
        };
        // Derive the initial status from time (cliff == 0 starts `Vesting`).
        schedule.status = Self::current_status(&schedule, start)?;
        env.storage()
            .instance()
            .set(&DataKey::Schedule(id), &schedule);
        Ok(id)
    }

    /// Claim the vested-but-unclaimed amount.
    ///
    /// Requires the beneficiary. Returns exactly what vested since the last
    /// claim (or `0` when nothing is claimable), so repeated claims can never
    /// overpay or underpay.
    pub fn claim(env: Env, schedule_id: u64) -> Result<i128, ForgeError> {
        let mut schedule = Self::get_schedule(&env, schedule_id)?;
        // NOTE: when a revocation method lands, `claim` must be gated on
        // `schedule.status != Revoked`; the status is currently unreachable.
        schedule.beneficiary.require_auth();

        let now = env.ledger().timestamp();
        let amount = Self::claimable_amount(&schedule, now)?;
        if amount == 0 {
            return Ok(0);
        }

        schedule.claimed = schedule
            .claimed
            .checked_add(amount)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        schedule.status = Self::current_status(&schedule, now)?;
        env.storage()
            .instance()
            .set(&DataKey::Schedule(schedule_id), &schedule);
        Ok(amount)
    }

    /// Amount currently claimable (read-only view; no state change).
    pub fn claimable(env: Env, schedule_id: u64) -> Result<i128, ForgeError> {
        let schedule = Self::get_schedule(&env, schedule_id)?;
        Self::claimable_amount(&schedule, env.ledger().timestamp())
    }

    /// Read the current lifecycle status (read-only view).
    ///
    /// The status is derived from the ledger time and claimed amount rather
    /// than the stored field, so it is always current between claims.
    pub fn get_status(env: Env, schedule_id: u64) -> Result<VestingStatus, ForgeError> {
        let schedule = Self::get_schedule(&env, schedule_id)?;
        Self::current_status(&schedule, env.ledger().timestamp())
    }

    /// Allocate the next monotonic schedule id.
    fn next_id(env: &Env) -> Result<u64, ForgeError> {
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        let id = count.checked_add(1).ok_or(ForgeError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::Count, &id);
        Ok(id)
    }

    fn get_schedule(env: &Env, schedule_id: u64) -> Result<VestingSchedule, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Schedule(schedule_id))
            .ok_or(ForgeError::NotFound)
    }

    /// Derive the lifecycle status from ledger time and claimed amount.
    fn current_status(schedule: &VestingSchedule, now: u64) -> Result<VestingStatus, ForgeError> {
        if schedule.claimed >= schedule.total_amount {
            return Ok(VestingStatus::Completed);
        }
        let cliff_time = schedule
            .start
            .checked_add(schedule.cliff)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        if now < cliff_time {
            return Ok(VestingStatus::Locked);
        }
        Ok(VestingStatus::Vesting)
    }

    /// Vested amount at ledger time `now`, using floor division so claims
    /// never round up.
    fn vested_amount(schedule: &VestingSchedule, now: u64) -> Result<i128, ForgeError> {
        let cliff_time = schedule
            .start
            .checked_add(schedule.cliff)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        if now < cliff_time {
            return Ok(0);
        }
        let end_time = schedule
            .start
            .checked_add(schedule.duration)
            .ok_or(ForgeError::ArithmeticOverflow)?;
        if now >= end_time {
            return Ok(schedule.total_amount);
        }

        // `cliff <= duration` is enforced at creation, so the period is
        // non-negative; a zero period (cliff == duration) means everything
        // vests at once, which the `now >= end_time` branch above already
        // returned. Guard defensively against division by zero.
        let period = end_time - cliff_time;
        if period == 0 {
            return Ok(schedule.total_amount);
        }
        let elapsed = now - cliff_time;
        let vested = schedule
            .total_amount
            .checked_mul(elapsed as i128)
            .ok_or(ForgeError::ArithmeticOverflow)?
            / period as i128;
        Ok(vested)
    }

    /// Claimable amount at ledger time `now` (vested minus claimed).
    fn claimable_amount(schedule: &VestingSchedule, now: u64) -> Result<i128, ForgeError> {
        let vested = Self::vested_amount(schedule, now)?;
        // By construction `claimed` never exceeds `vested`, so the subtraction
        // cannot underflow; use checked arithmetic to fail loudly if the
        // invariant is ever broken.
        vested
            .checked_sub(schedule.claimed)
            .ok_or(ForgeError::ArithmeticOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_forge_test_utils::TestAccounts;
    use soroban_sdk::testutils::Ledger as _;
    use soroban_sdk::Env;

    const START: u64 = 1_000_000;
    const CLIFF: u64 = 1_000;
    const DURATION: u64 = 4_000;
    const TOTAL: i128 = 10_000;

    /// Build a fresh env with mocked auths, a registered contract, and named
    /// accounts. The generated client borrows the env, so it cannot be
    /// returned from a helper.
    macro_rules! setup {
        () => {{
            let env = Env::default();
            env.mock_all_auths();
            env.ledger().set_timestamp(START);
            let contract_id = env.register(Vesting, ());
            let client = SorobanForgeVestingClient::new(&env, &contract_id);
            let accounts = TestAccounts::generate(&env);
            (env, client, accounts)
        }};
    }

    // NOTE: negative authorization tests (calling `require_auth` without a
    // matching signature) are not runnable in-process with soroban-sdk 21.5.1:
    // the host raises a non-unwinding panic that aborts the test binary. They
    // are tracked in the security-invariant test backlog (Issue 7).

    fn create(client: &SorobanForgeVestingClient<'_>, accounts: &TestAccounts) -> u64 {
        client.create_schedule(
            &accounts.user1,
            &accounts.validator,
            &TOTAL,
            &CLIFF,
            &DURATION,
        )
    }

    #[test]
    fn create_schedule_succeeds_and_is_locked() {
        let (_env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        assert_eq!(client.get_status(&id), VestingStatus::Locked);
        assert_eq!(client.claimable(&id), 0);
    }

    #[test]
    fn create_schedule_assigns_distinct_ids() {
        let (_env, client, accounts) = setup!();
        let id1 = create(&client, &accounts);
        let id2 = create(&client, &accounts);
        assert_ne!(id1, id2);
    }

    #[test]
    fn create_schedule_without_cliff_starts_vesting() {
        let (_env, client, accounts) = setup!();
        let id = client.create_schedule(
            &accounts.user1,
            &accounts.validator,
            &TOTAL,
            &0_u64,
            &DURATION,
        );
        assert_eq!(client.get_status(&id), VestingStatus::Vesting);
    }

    #[test]
    fn create_schedule_rejects_zero_total() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_create_schedule(
                &accounts.user1,
                &accounts.validator,
                &0_i128,
                &CLIFF,
                &DURATION,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn create_schedule_rejects_zero_duration() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_create_schedule(&accounts.user1, &accounts.validator, &TOTAL, &CLIFF, &0_u64)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn create_schedule_rejects_cliff_after_duration() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_create_schedule(
                &accounts.user1,
                &accounts.validator,
                &TOTAL,
                &5_000_u64,
                &4_000_u64,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn claimable_before_cliff_is_zero() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        // Halfway between start and the cliff.
        env.ledger().set_timestamp(START + CLIFF / 2);
        assert_eq!(client.claimable(&id), 0);
    }

    #[test]
    fn claimable_at_cliff_is_zero() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger().set_timestamp(START + CLIFF);
        assert_eq!(client.claimable(&id), 0);
    }

    #[test]
    fn claim_before_cliff_returns_zero() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger().set_timestamp(START + CLIFF / 2);
        assert_eq!(client.claim(&id), 0);
    }

    #[test]
    fn claimable_midway_is_half() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        // Halfway through the vesting window (cliff .. duration).
        env.ledger()
            .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
        assert_eq!(client.claimable(&id), TOTAL / 2);
    }

    #[test]
    fn claimable_at_duration_is_full() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger().set_timestamp(START + DURATION);
        assert_eq!(client.claimable(&id), TOTAL);
    }

    #[test]
    fn claimable_after_duration_is_full() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger().set_timestamp(START + DURATION + 1);
        assert_eq!(client.claimable(&id), TOTAL);
    }

    #[test]
    fn claim_pays_exact_amount() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger()
            .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
        assert_eq!(client.claim(&id), TOTAL / 2);
    }

    #[test]
    fn repeated_claims_never_overpay_or_underpay() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);

        // Claim half at the midway point.
        env.ledger()
            .set_timestamp(START + CLIFF + (DURATION - CLIFF) / 2);
        assert_eq!(client.claim(&id), TOTAL / 2);
        assert_eq!(client.claimable(&id), 0);

        // Advance past the end; the remaining half becomes claimable.
        env.ledger().set_timestamp(START + DURATION + 100);
        assert_eq!(client.claim(&id), TOTAL - TOTAL / 2);
        assert_eq!(client.claimable(&id), 0);

        // A further claim is a no-op.
        assert_eq!(client.claim(&id), 0);
        assert_eq!(client.get_status(&id), VestingStatus::Completed);
    }

    #[test]
    fn claim_after_end_completes_status() {
        let (env, client, accounts) = setup!();
        let id = create(&client, &accounts);
        env.ledger().set_timestamp(START + DURATION + 1);
        assert_eq!(client.get_status(&id), VestingStatus::Vesting);
        assert_eq!(client.claim(&id), TOTAL);
        assert_eq!(client.get_status(&id), VestingStatus::Completed);
    }

    #[test]
    fn claim_without_cliff_vests_from_start() {
        let (env, client, accounts) = setup!();
        let id = client.create_schedule(
            &accounts.user1,
            &accounts.validator,
            &TOTAL,
            &0_u64,
            &DURATION,
        );
        env.ledger().set_timestamp(START + DURATION / 2);
        assert_eq!(client.claimable(&id), TOTAL / 2);
    }

    #[test]
    fn cliff_equals_duration_vests_at_once() {
        let (env, client, accounts) = setup!();
        let id = client.create_schedule(
            &accounts.user1,
            &accounts.validator,
            &TOTAL,
            &DURATION,
            &DURATION,
        );
        env.ledger().set_timestamp(START + DURATION - 1);
        assert_eq!(client.claimable(&id), 0);
        env.ledger().set_timestamp(START + DURATION);
        assert_eq!(client.claimable(&id), TOTAL);
    }

    #[test]
    fn claim_missing_schedule_is_not_found() {
        let (_env, client, _accounts) = setup!();
        let err = client.try_claim(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn claimable_missing_schedule_is_not_found() {
        let (_env, client, _accounts) = setup!();
        let err = client.try_claimable(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn get_status_missing_schedule_is_not_found() {
        let (_env, client, _accounts) = setup!();
        let err = client.try_get_status(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn claimable_overflow_is_reported() {
        let (env, client, accounts) = setup!();
        // A huge total with a non-trivial elapsed time overflows the
        // intermediate `total * elapsed` product.
        let id = client.create_schedule(
            &accounts.user1,
            &accounts.validator,
            &i128::MAX,
            &0_u64,
            &1_000_u64,
        );
        env.ledger().set_timestamp(START + 500);
        let err = client.try_claimable(&id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::ArithmeticOverflow);
    }
}
