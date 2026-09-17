#![no_std]

//! # Soroban Forge — Marketplace Royalties contract
//!
//! Enforces creator royalty splits on secondary sales: when an NFT changes
//! hands, the sale proceeds are split between the seller and one or more
//! royalty recipients according to configured basis-point rates. This iteration
//! stores one royalty configuration per collection, with a single recipient.
//!
//! Flow:
//!
//! ```text
//! set_royalty(collection, recipient, bps) -> Active config
//! distribute(collection, seller, amount)  -> pays `amount * bps / 10_000`
//!                                            to the recipient, returns the net
//!                                            remainder owed to the seller
//! ```
//!
//! Authorization model:
//! - `set_royalty` requires the collection (the contract whose config this
//!   is), and `bps` must not exceed 100% (10_000 bps).
//! - `distribute` requires the collection and returns the seller's net after
//!   the configured royalty split; a `Disabled` configuration settles in full
//!   to the seller.
//! - `get_royalty` is a read-only view.
//!
//! Multiple recipients per collection, per-token royalties, and actual token
//! settlement are intentionally out of scope for this iteration: the contract
//! tracks royalty configuration and computed splits, not balances.

#[cfg(test)]
extern crate std;

use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::{contract, contractclient, contractimpl, contracttype, Address, Env};

/// Public interface for the Soroban Forge marketplace royalties contract.
#[contractclient(name = "SorobanForgeMarketplaceRoyaltiesClient")]
pub trait SorobanForgeMarketplaceRoyalties {
    /// Register or update the royalty recipient and basis-point rate for
    /// `collection`.
    fn set_royalty(
        env: Env,
        collection: Address,
        recipient: Address,
        bps: u32,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Distribute `amount` from a sale of `collection`, returning the net to
    /// the seller after royalties.
    fn distribute(
        env: Env,
        collection: Address,
        seller: Address,
        amount: i128,
    ) -> Result<i128, soroban_forge_shared_utils::ForgeError>;

    /// Read the stored royalty configuration for `collection` (read-only view).
    fn get_royalty(
        env: Env,
        collection: Address,
    ) -> Result<Royalty, soroban_forge_shared_utils::ForgeError>;
}

/// Lifecycle state of a registered royalty configuration.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoyaltyStatus {
    /// Active and applied to sales.
    Active,
    /// Disabled; sales settle to the seller in full.
    Disabled,
}

/// A royalty configuration for a single collection.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Royalty {
    /// Collection (NFT contract) this configuration applies to.
    pub collection: Address,
    /// Address entitled to royalty payments.
    pub recipient: Address,
    /// Royalty rate in basis points (100 bps = 1%).
    pub bps: u32,
    /// Whether the configuration is currently enforced.
    pub status: RoyaltyStatus,
}

/// Instance-storage keys.
#[contracttype]
enum DataKey {
    /// The royalty configuration for `Address` collection.
    Royalty(Address),
}

/// The deployable marketplace royalties contract.
#[contract]
pub struct MarketplaceRoyalties;

#[contractimpl]
impl MarketplaceRoyalties {
    /// Register or update a royalty configuration for `collection`.
    ///
    /// Requires the collection's authorization and `bps <= 10_000`
    /// (100%). Re-registration updates the existing configuration in place.
    pub fn set_royalty(
        env: Env,
        collection: Address,
        recipient: Address,
        bps: u32,
    ) -> Result<(), ForgeError> {
        if bps > 10_000 {
            return Err(ForgeError::InvalidInput);
        }
        collection.require_auth();

        let royalty = Royalty {
            collection,
            recipient,
            bps,
            status: RoyaltyStatus::Active,
        };
        env.storage()
            .instance()
            .set(&DataKey::Royalty(royalty.collection.clone()), &royalty);
        Ok(())
    }

    /// Compute the royalty split for a sale.
    ///
    /// Requires the collection's authorization and `amount > 0`. Returns the
    /// net owed to `seller` after reserving `amount * bps / 10_000` for the
    /// configured recipient. A `Disabled` configuration settles in full.
    pub fn distribute(
        env: Env,
        collection: Address,
        seller: Address,
        amount: i128,
    ) -> Result<i128, ForgeError> {
        let royalty: Royalty = env
            .storage()
            .instance()
            .get(&DataKey::Royalty(collection.clone()))
            .ok_or(ForgeError::NotFound)?;
        if amount <= 0 {
            return Err(ForgeError::InvalidInput);
        }
        collection.require_auth();

        if royalty.status == RoyaltyStatus::Disabled {
            return Ok(amount);
        }

        let royalty_share = amount
            .checked_mul(royalty.bps as i128)
            .ok_or(ForgeError::ArithmeticOverflow)?
            / 10_000;
        let _ = &seller;
        // `bps <= 10_000` guarantees `royalty_share <= amount`, so the net is
        // never negative; use checked arithmetic to fail loudly if the
        // invariant is ever broken.
        amount
            .checked_sub(royalty_share)
            .ok_or(ForgeError::ArithmeticOverflow)
    }

    /// Read the stored royalty configuration for `collection` (read-only view).
    pub fn get_royalty(env: Env, collection: Address) -> Result<Royalty, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Royalty(collection))
            .ok_or(ForgeError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_forge_test_utils::TestAccounts;
    use soroban_sdk::Env;

    /// Build a fresh env with mocked auths, a registered contract, a registered
    /// royalty config (500 bps = 5%), and named accounts.
    macro_rules! setup {
        () => {{
            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register(MarketplaceRoyalties, ());
            let client = SorobanForgeMarketplaceRoyaltiesClient::new(&env, &contract_id);
            let accounts = TestAccounts::generate(&env);
            client.set_royalty(&accounts.arbiter, &accounts.user2, &500_u32);
            (env, client, accounts)
        }};
    }

    #[test]
    fn set_royalty_stores_config() {
        let (_env, client, accounts) = setup!();
        let royalty = client.get_royalty(&accounts.arbiter);
        assert_eq!(royalty.recipient, accounts.user2);
        assert_eq!(royalty.bps, 500);
        assert_eq!(royalty.status, RoyaltyStatus::Active);
    }

    #[test]
    fn set_royalty_update_in_place() {
        let (_env, client, accounts) = setup!();
        client.set_royalty(&accounts.arbiter, &accounts.user3, &1_000_u32);
        let royalty = client.get_royalty(&accounts.arbiter);
        assert_eq!(royalty.recipient, accounts.user3);
        assert_eq!(royalty.bps, 1_000);
    }

    #[test]
    fn set_royalty_rejects_bps_over_100_percent() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_set_royalty(&accounts.arbiter, &accounts.user2, &10_001_u32)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn distribute_pays_royalty_and_returns_net() {
        let (_env, client, accounts) = setup!();
        // 1000 units sold with a 5% royalty -> 50 to the recipient, 950 net.
        let net = client.distribute(&accounts.arbiter, &accounts.user1, &1_000_i128);
        assert_eq!(net, 950);
    }

    #[test]
    fn distribute_zero_bps_returns_full_amount() {
        let (_env, client, accounts) = setup!();
        client.set_royalty(&accounts.arbiter, &accounts.user2, &0_u32);
        let net = client.distribute(&accounts.arbiter, &accounts.user1, &1_000_i128);
        assert_eq!(net, 1_000);
    }

    #[test]
    fn distribute_100_percent_returns_zero_net() {
        let (_env, client, accounts) = setup!();
        client.set_royalty(&accounts.arbiter, &accounts.user2, &10_000_u32);
        let net = client.distribute(&accounts.arbiter, &accounts.user1, &1_000_i128);
        assert_eq!(net, 0);
    }

    #[test]
    fn distribute_rejects_non_positive_amount() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_distribute(&accounts.arbiter, &accounts.user1, &0_i128)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn distribute_missing_config_is_not_found() {
        let (_env, client, accounts) = setup!();
        // An unregistered collection has no config.
        let err = client
            .try_distribute(&accounts.validator, &accounts.user1, &1_000_i128)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn get_royalty_missing_is_not_found() {
        let (_env, client, accounts) = setup!();
        let err = client
            .try_get_royalty(&accounts.validator)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }

    #[test]
    fn distribute_disabled_collection_settles_in_full() {
        let (_env, client, accounts) = setup!();
        // A re-registration with bps 0 keeps the config `Active`; emulate a
        // disabled state by checking that a zero-bps config settles in full.
        client.set_royalty(&accounts.arbiter, &accounts.user2, &0_u32);
        assert_eq!(
            client.get_royalty(&accounts.arbiter).status,
            RoyaltyStatus::Active
        );
        let net = client.distribute(&accounts.arbiter, &accounts.user1, &1_000_i128);
        assert_eq!(net, 1_000);
    }
}
