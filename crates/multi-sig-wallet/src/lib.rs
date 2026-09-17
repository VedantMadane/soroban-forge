#![no_std]

//! # Soroban Forge — Multi-signature Wallet contract
//!
//! A wallet that requires `threshold` approvals from a set of owners before a
//! transaction executes. The wallet is configured once via
//! [`SorobanForgeMultiSigWallet::initialize`]; transactions submitted by an
//! owner then collect confirmations from other owners until the threshold is
//! met, at which point [`SorobanForgeMultiSigWallet::execute`] completes them.
//!
//! Lifecycle:
//!
//! ```text
//! submit --(confirm × n)--> threshold met --execute--> Executed
//! ```
//!
//! Authorization model:
//! - `initialize` sets the owner set and threshold once (guarded against
//!   re-initialisation).
//! - `submit` requires an owner.
//! - `confirm` requires an owner that has not confirmed already.
//! - `execute` may be called by anyone; it only succeeds once the threshold is
//!   met.
//!
//! The `Rejected` status is reserved for a revocation/rejection method that
//! lands in a follow-up; it is not reachable through the current public
//! interface. Execution of the payload (external contract calls) is
//! intentionally out of scope for this iteration: the contract tracks state,
//! ownership, and authorisation, not payload execution.

#[cfg(test)]
extern crate std;

use soroban_forge_shared_utils::ForgeError;
use soroban_sdk::{contract, contractclient, contractimpl, contracttype, Address, Bytes, Env, Vec};

/// Public interface for the Soroban Forge multi-signature wallet contract.
#[contractclient(name = "SorobanForgeMultiSigWalletClient")]
pub trait SorobanForgeMultiSigWallet {
    /// Configure the wallet with the given set of `owners` and an approval
    /// `threshold`. May only be called once.
    fn initialize(
        env: Env,
        owners: Vec<Address>,
        threshold: u32,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Add a pending transaction submitted by `submitter` and open it for
    /// owner approvals. Returns the stable transaction id.
    fn submit(
        env: Env,
        submitter: Address,
        tx: Bytes,
    ) -> Result<u64, soroban_forge_shared_utils::ForgeError>;

    /// Record `signer`'s approval of `tx_id`.
    fn confirm(
        env: Env,
        tx_id: u64,
        signer: Address,
    ) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Execute `tx_id` once approvals meet the configured threshold.
    fn execute(env: Env, tx_id: u64) -> Result<(), soroban_forge_shared_utils::ForgeError>;

    /// Read the current approval threshold (read-only view).
    fn get_threshold(env: Env) -> Result<u32, soroban_forge_shared_utils::ForgeError>;

    /// Read a stored transaction by id (read-only view).
    fn get_tx(env: Env, tx_id: u64) -> Result<WalletTx, soroban_forge_shared_utils::ForgeError>;
}

/// Lifecycle state of a submitted transaction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxStatus {
    /// Open for confirmations; threshold not yet met.
    Pending,
    /// Threshold met and executed successfully.
    Executed,
    /// Rejected by owners (reached a rejection threshold or manually revoked).
    Rejected,
}

/// A transaction awaiting multi-signature approval.
#[contracttype]
#[derive(Clone, Debug)]
pub struct WalletTx {
    /// Stable identifier assigned at submission time.
    pub tx_id: u64,
    /// Address that submitted the transaction.
    pub submitter: Address,
    /// The encoded transaction payload to execute.
    pub payload: Bytes,
    /// Owners that have confirmed so far.
    pub confirmations: soroban_sdk::Vec<Address>,
    /// Current state.
    pub status: TxStatus,
}

/// Instance-storage keys.
#[contracttype]
enum DataKey {
    /// The transaction record for `u64` id.
    Tx(u64),
    /// The wallet's owner set.
    Owners,
    /// The approval threshold required to execute.
    Threshold,
    /// Monotonic transaction id counter.
    Count,
}

/// The deployable multi-signature wallet contract.
#[contract]
pub struct MultiSigWallet;

#[contractimpl]
impl MultiSigWallet {
    /// Configure the wallet for the first and only time.
    ///
    /// Requires a non-empty owner set with no duplicates and
    /// `0 < threshold <= owners.len()`. The caller deploys the wallet and
    /// initialises it in the same transaction.
    pub fn initialize(env: Env, owners: Vec<Address>, threshold: u32) -> Result<(), ForgeError> {
        if env.storage().instance().has(&DataKey::Threshold) {
            return Err(ForgeError::AlreadyInitialized);
        }
        if owners.is_empty() {
            return Err(ForgeError::InvalidInput);
        }
        if threshold == 0 || threshold > owners.len() {
            return Err(ForgeError::InvalidInput);
        }
        // Reject duplicate owners so a single owner can never inflate their
        // personal approval count.
        for i in 0..owners.len() {
            let owner = owners.get_unchecked(i);
            for j in 0..i {
                if owners.get_unchecked(j) == owner {
                    return Err(ForgeError::InvalidInput);
                }
            }
        }

        env.storage().instance().set(&DataKey::Owners, &owners);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
        Ok(())
    }

    /// Submit a new transaction for owner approval.
    ///
    /// Requires the submitter to be an owner. Returns the stable `tx_id` that
    /// confirmations reference.
    pub fn submit(env: Env, submitter: Address, tx: Bytes) -> Result<u64, ForgeError> {
        if !Self::is_initialized(&env) {
            return Err(ForgeError::NotInitialized);
        }
        if !Self::is_owner(&env, &submitter) {
            return Err(ForgeError::Unauthorized);
        }
        submitter.require_auth();

        let tx_id = Self::next_id(&env)?;
        let wallet_tx = WalletTx {
            tx_id,
            submitter,
            payload: tx,
            confirmations: Vec::new(&env),
            status: TxStatus::Pending,
        };
        env.storage()
            .instance()
            .set(&DataKey::Tx(tx_id), &wallet_tx);
        Ok(tx_id)
    }

    /// Record an owner's approval of a pending transaction.
    ///
    /// An owner may confirm only once, and only while the transaction is
    /// `Pending`.
    pub fn confirm(env: Env, tx_id: u64, signer: Address) -> Result<(), ForgeError> {
        let mut wallet_tx = Self::get_tx_impl(&env, tx_id)?;
        if wallet_tx.status != TxStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }
        if !Self::is_owner(&env, &signer) {
            return Err(ForgeError::Unauthorized);
        }
        signer.require_auth();

        if wallet_tx.confirmations.contains(&signer) {
            return Err(ForgeError::InvalidInput);
        }
        wallet_tx.confirmations.push_back(signer);
        env.storage()
            .instance()
            .set(&DataKey::Tx(tx_id), &wallet_tx);
        Ok(())
    }

    /// Execute a transaction once the owner approvals meet the threshold.
    ///
    /// Callable by anyone once the threshold is met; otherwise the state
    /// transition is rejected.
    pub fn execute(env: Env, tx_id: u64) -> Result<(), ForgeError> {
        let mut wallet_tx = Self::get_tx_impl(&env, tx_id)?;
        if wallet_tx.status != TxStatus::Pending {
            return Err(ForgeError::InvalidInput);
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(ForgeError::NotInitialized)?;
        if wallet_tx.confirmations.len() < threshold {
            return Err(ForgeError::InvalidInput);
        }

        wallet_tx.status = TxStatus::Executed;
        env.storage()
            .instance()
            .set(&DataKey::Tx(tx_id), &wallet_tx);
        Ok(())
    }

    /// Read the configured approval threshold (read-only view).
    pub fn get_threshold(env: Env) -> Result<u32, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(ForgeError::NotInitialized)
    }

    /// Read a stored transaction by id (read-only view).
    pub fn get_tx(env: Env, tx_id: u64) -> Result<WalletTx, ForgeError> {
        Self::get_tx_impl(&env, tx_id)
    }

    /// Allocate the next monotonic transaction id.
    fn next_id(env: &Env) -> Result<u64, ForgeError> {
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        let id = count.checked_add(1).ok_or(ForgeError::ArithmeticOverflow)?;
        env.storage().instance().set(&DataKey::Count, &id);
        Ok(id)
    }

    fn is_initialized(env: &Env) -> bool {
        env.storage().instance().has(&DataKey::Threshold)
    }

    fn is_owner(env: &Env, address: &Address) -> bool {
        let owners: Vec<Address> = match env.storage().instance().get(&DataKey::Owners) {
            Some(owners) => owners,
            None => return false,
        };
        owners.contains(address)
    }

    fn get_tx_impl(env: &Env, tx_id: u64) -> Result<WalletTx, ForgeError> {
        env.storage()
            .instance()
            .get(&DataKey::Tx(tx_id))
            .ok_or(ForgeError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_forge_test_utils::TestAccounts;
    use soroban_sdk::{Bytes, Env};

    /// Build a fresh env with mocked auths, a registered contract, a configured
    /// wallet (threshold 2), and named accounts. The generated client exposes
    /// its env via the public `env` field, so it cannot be returned from a
    /// helper.
    macro_rules! setup {
        () => {{
            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register(MultiSigWallet, ());
            let client = SorobanForgeMultiSigWalletClient::new(&env, &contract_id);
            let accounts = TestAccounts::generate(&env);
            let owners = owner_vec(&env, &accounts);
            client.initialize(&owners, &2_u32);
            (env, client, accounts)
        }};
    }

    fn owner_vec(env: &Env, accounts: &TestAccounts) -> soroban_sdk::Vec<Address> {
        soroban_sdk::vec![
            env,
            accounts.user1.clone(),
            accounts.user2.clone(),
            accounts.user3.clone()
        ]
    }

    fn payload(env: &Env) -> Bytes {
        Bytes::from_array(env, &[0x01, 0x02, 0x03])
    }

    #[test]
    fn initialize_sets_threshold() {
        let (_env, client, accounts) = setup!();
        assert_eq!(client.get_threshold(), 2_u32);
        let _ = owner_vec(&client.env, &accounts);
    }

    /// A fresh, uninitialized wallet for `initialize` validation tests. The
    /// standard `setup!` is intentionally avoided so `try_initialize` never
    /// returns `AlreadyInitialized`.
    macro_rules! fresh {
        () => {{
            let env = Env::default();
            env.mock_all_auths();
            let contract_id = env.register(MultiSigWallet, ());
            let client = SorobanForgeMultiSigWalletClient::new(&env, &contract_id);
            let accounts = TestAccounts::generate(&env);
            (env, client, accounts)
        }};
    }

    #[test]
    fn initialize_rejects_zero_threshold() {
        let (env, client, accounts) = fresh!();
        let err = client
            .try_initialize(&owner_vec(&env, &accounts), &0_u32)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn initialize_rejects_threshold_above_owner_count() {
        let (env, client, accounts) = fresh!();
        let err = client
            .try_initialize(&owner_vec(&env, &accounts), &99_u32)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn initialize_rejects_empty_owners() {
        let (env, client, _accounts) = fresh!();
        let err = client
            .try_initialize(&soroban_sdk::Vec::new(&env), &1_u32)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn initialize_rejects_duplicate_owners() {
        let (env, client, accounts) = fresh!();
        let dup = soroban_sdk::vec![
            &env,
            accounts.user1.clone(),
            accounts.user1.clone(),
            accounts.user2.clone()
        ];
        let err = client.try_initialize(&dup, &2_u32).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn initialize_rejects_reinitialization() {
        let (env, client, accounts) = setup!();
        let err = client
            .try_initialize(&owner_vec(&env, &accounts), &3_u32)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::AlreadyInitialized);
    }

    #[test]
    fn submit_creates_pending_tx() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        let tx = client.get_tx(&tx_id);
        assert_eq!(tx.submitter, accounts.user1);
        assert_eq!(tx.status, TxStatus::Pending);
        assert_eq!(tx.confirmations.len(), 0);
    }

    #[test]
    fn submit_assigns_distinct_ids() {
        let (env, client, accounts) = setup!();
        let id1 = client.submit(&accounts.user1, &payload(&env));
        let id2 = client.submit(&accounts.user1, &payload(&env));
        assert_ne!(id1, id2);
    }

    #[test]
    fn submit_rejects_non_owner() {
        let (env, client, accounts) = setup!();
        let err = client
            .try_submit(&accounts.arbiter, &payload(&env))
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::Unauthorized);
    }

    #[test]
    fn submit_before_initialize_is_not_initialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MultiSigWallet, ());
        let client = SorobanForgeMultiSigWalletClient::new(&env, &contract_id);
        let accounts = TestAccounts::generate(&env);
        let err = client
            .try_submit(&accounts.user1, &payload(&env))
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::NotInitialized);
    }

    #[test]
    fn confirm_records_approval() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        client.confirm(&tx_id, &accounts.user2);
        let tx = client.get_tx(&tx_id);
        assert_eq!(tx.confirmations.len(), 1);
        assert_eq!(tx.confirmations.get_unchecked(0), accounts.user2);
    }

    #[test]
    fn confirm_twice_is_invalid() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        client.confirm(&tx_id, &accounts.user2);
        let err = client
            .try_confirm(&tx_id, &accounts.user2)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn confirm_non_owner_is_unauthorized() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        let err = client
            .try_confirm(&tx_id, &accounts.arbiter)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ForgeError::Unauthorized);
    }

    #[test]
    fn confirm_missing_tx_is_not_found() {
        let (_env, client, accounts) = setup!();
        client
            .try_confirm(&999, &accounts.user1)
            .unwrap_err()
            .unwrap();
    }

    #[test]
    fn execute_requires_threshold() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        // Below the threshold of 2: execution is still blocked.
        client.confirm(&tx_id, &accounts.user2);
        let err = client.try_execute(&tx_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn execute_after_threshold_succeeds() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        client.confirm(&tx_id, &accounts.user2);
        client.confirm(&tx_id, &accounts.user3);
        client.execute(&tx_id);
        assert_eq!(client.get_tx(&tx_id).status, TxStatus::Executed);
    }

    #[test]
    fn execute_twice_is_invalid() {
        let (env, client, accounts) = setup!();
        let tx_id = client.submit(&accounts.user1, &payload(&env));
        client.confirm(&tx_id, &accounts.user2);
        client.confirm(&tx_id, &accounts.user3);
        client.execute(&tx_id);
        let err = client.try_execute(&tx_id).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::InvalidInput);
    }

    #[test]
    fn missing_tx_is_not_found() {
        let (_env, client, _accounts) = setup!();
        let err = client.try_get_tx(&999).unwrap_err().unwrap();
        assert_eq!(err, ForgeError::NotFound);
    }
}
