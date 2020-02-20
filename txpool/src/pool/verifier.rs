//! Transaction Verifier
//!
//! Responsible for verifying a transaction before importing to the pool.
//! Should make sure that the transaction is structuraly valid.
//!
//! May have some overlap with `Readiness` since we don't want to keep around
//! stalled transactions.
use std::{
    cmp,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
};

use common_crypto::hash::*;
use tx_pool;
use types::transaction;

use super::{client::Client, Gas, GasPrice, VerifiedTransaction};
use crate::pool::scoring;
use std::ops::Deref;

/// Verification options.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Minimal allowed gas price.
    pub minimal_gas_price: GasPrice,
    /// Current block gas limit.
    pub block_gas_limit: Gas,
    /// Maximal gas limit for a single transaction.
    pub tx_gas_limit: Gas,
    /// Skip checks for early rejection, to make sure that local transactions are always imported.
    pub no_early_reject: bool,
}

#[cfg(test)]
impl Default for Options {
    fn default() -> Self {
        Options {
            minimal_gas_price: 0,
            block_gas_limit: Gas::max_value(),
            tx_gas_limit: Gas::max_value(),
            no_early_reject: false,
        }
    }
}

/// Transaction to verify.
#[cfg_attr(test, derive(Clone))]
pub enum Transaction {
    /// Fresh, never verified transaction.
    ///
    /// We need to do full verification of such transactions
    Unverified(transaction::SignedUserTransaction),

    /// Transaction from retracted block.
    ///
    /// We could skip some parts of verification of such transactions
    Retracted(transaction::SignedUserTransaction),

    /// Locally signed or retracted transaction.
    ///
    /// We can skip consistency verifications and just verify readiness.
    Local(transaction::PendingTransaction),
}

impl Transaction {
    /// Return transaction hash
    pub fn hash(&self) -> HashValue {
        match *self {
            Transaction::Unverified(ref tx) => CryptoHash::crypto_hash(&tx),
            Transaction::Retracted(ref tx) => CryptoHash::crypto_hash(&tx),
            Transaction::Local(ref tx) => CryptoHash::crypto_hash(tx.deref()),
        }
    }

    /// Return transaction gas price
    pub fn gas_price(&self) -> GasPrice {
        match self {
            Transaction::Unverified(ref tx) => tx.gas_unit_price(),
            Transaction::Retracted(ref tx) => tx.gas_unit_price(),
            Transaction::Local(ref tx) => tx.gas_unit_price(),
        }
    }

    fn gas(&self) -> Gas {
        match self {
            Transaction::Unverified(ref tx) => tx.max_gas_amount(),
            Transaction::Retracted(ref tx) => tx.max_gas_amount(),
            Transaction::Local(ref tx) => tx.max_gas_amount(),
        }
    }

    fn transaction(&self) -> &transaction::RawUserTransaction {
        match self {
            Transaction::Unverified(ref tx) => tx.raw_txn(),
            Transaction::Retracted(ref tx) => tx.raw_txn(),
            Transaction::Local(ref tx) => tx.raw_txn(),
        }
    }

    fn is_local(&self) -> bool {
        match self {
            Transaction::Local(..) => true,
            _ => false,
        }
    }

    fn is_retracted(&self) -> bool {
        match self {
            Transaction::Retracted(..) => true,
            _ => false,
        }
    }
}

/// Transaction verifier.
///
/// Verification can be run in parallel for all incoming transactions.
#[derive(Debug)]
pub struct Verifier<C, S, V> {
    client: C,
    options: Options,
    id: Arc<AtomicUsize>,
    transaction_to_replace: Option<(S, Arc<V>)>,
}

impl<C, S, V> Verifier<C, S, V> {
    /// Creates new transaction verfier with specified options.
    pub fn new(
        client: C,
        options: Options,
        id: Arc<AtomicUsize>,
        transaction_to_replace: Option<(S, Arc<V>)>,
    ) -> Self {
        Verifier {
            client,
            options,
            id,
            transaction_to_replace,
        }
    }
}

impl<C: Client> tx_pool::Verifier<Transaction>
    for Verifier<C, scoring::NonceAndGasPrice, VerifiedTransaction>
{
    type Error = transaction::TransactionError;
    type VerifiedTransaction = VerifiedTransaction;

    fn verify_transaction(
        &self,
        tx: Transaction,
    ) -> Result<Self::VerifiedTransaction, Self::Error> {
        todo!()
        //        // The checks here should be ordered by cost/complexity.
        //        // Cheap checks should be done as early as possible to discard unneeded transactions early.
        //
        //        let hash = tx.hash();
        //
        //        if self.client.transaction_already_included(&hash) {
        //            trace!(target: "txqueue", "[{:?}] Rejected tx already in the blockchain", hash);
        //            return Err(transaction::Error::AlreadyImported);
        //        }
        //
        //        let gas_limit = cmp::min(self.options.tx_gas_limit, self.options.block_gas_limit);
        //        if tx.gas() > &gas_limit {
        //            debug!(
        //                target: "txqueue",
        //                "[{:?}] Rejected transaction above gas limit: {} > min({}, {})",
        //                hash,
        //                tx.gas(),
        //                self.options.block_gas_limit,
        //                self.options.tx_gas_limit,
        //            );
        //            return Err(transaction::Error::GasLimitExceeded {
        //                limit: gas_limit,
        //                got: *tx.gas(),
        //            });
        //        }
        //
        //        let minimal_gas = self.client.required_gas(tx.transaction());
        //        if tx.gas() < &minimal_gas {
        //            trace!(target: "txqueue",
        //                   "[{:?}] Rejected transaction with insufficient gas: {} < {}",
        //                   hash,
        //                   tx.gas(),
        //                   minimal_gas,
        //            );
        //
        //            return Err(transaction::Error::InsufficientGas {
        //                minimal: minimal_gas,
        //                got: *tx.gas(),
        //            });
        //        }
        //
        //        let is_own = tx.is_local();
        //        // Quick exit for non-service and non-local transactions
        //        //
        //        // We're checking if the transaction is below configured minimal gas price
        //        // or the effective minimal gas price in case the pool is full.
        //        if !tx.gas_price().is_zero() && !is_own {
        //            if tx.gas_price() < &self.options.minimal_gas_price {
        //                trace!(
        //                    target: "txqueue",
        //                    "[{:?}] Rejected tx below minimal gas price threshold: {} < {}",
        //                    hash,
        //                    tx.gas_price(),
        //                    self.options.minimal_gas_price,
        //                );
        //                return Err(transaction::Error::InsufficientGasPrice {
        //                    minimal: self.options.minimal_gas_price,
        //                    got: *tx.gas_price(),
        //                });
        //            }
        //
        //            if let Some((ref scoring, ref vtx)) = self.transaction_to_replace {
        //                if scoring.should_reject_early(vtx, &tx) {
        //                    trace!(
        //                        target: "txqueue",
        //                        "[{:?}] Rejected tx early, cause it doesn't have any chance to get to the pool: (gas price: {} < {})",
        //                        hash,
        //                        tx.gas_price(),
        //                        vtx.transaction.gas_price,
        //                    );
        //                    return Err(transaction::Error::TooCheapToReplace {
        //                        prev: Some(vtx.transaction.gas_price),
        //                        new: Some(*tx.gas_price()),
        //                    });
        //                }
        //            }
        //        }
        //
        //        // Some more heavy checks below.
        //        // Actually recover sender and verify that transaction
        //        let is_retracted = tx.is_retracted();
        //        let transaction = match tx {
        //            Transaction::Retracted(tx) | Transaction::Unverified(tx) => {
        //                match self.client.verify_transaction(tx) {
        //                    Ok(signed) => signed.into(),
        //                    Err(err) => {
        //                        debug!(target: "txqueue", "[{:?}] Rejected tx {:?}", hash, err);
        //                        return Err(err);
        //                    }
        //                }
        //            }
        //            Transaction::Local(tx) => match self.client.verify_transaction_basic(&**tx) {
        //                Ok(()) => tx,
        //                Err(err) => {
        //                    warn!(target: "txqueue", "[{:?}] Rejected local tx {:?}", hash, err);
        //                    return Err(err);
        //                }
        //            },
        //        };
        //
        //        // Verify RLP payload
        //        if let Err(err) = self.client.decode_transaction(&transaction.rlp_bytes()) {
        //            debug!(target: "txqueue", "[{:?}] Rejected transaction's rlp payload", err);
        //            return Err(err);
        //        }
        //
        //        let sender = transaction.sender();
        //        let account_details = self.client.account_details(&sender);
        //
        //        if transaction.gas_price < self.options.minimal_gas_price {
        //            let transaction_type = self.client.transaction_type(&transaction);
        //            if let TransactionType::Service = transaction_type {
        //                debug!(target: "txqueue", "Service tx {:?} below minimal gas price accepted", hash);
        //            } else if is_own || account_details.is_local {
        //                info!(target: "own_tx", "Local tx {:?} below minimal gas price accepted", hash);
        //            } else {
        //                trace!(
        //                    target: "txqueue",
        //                    "[{:?}] Rejected tx below minimal gas price threshold: {} < {}",
        //                    hash,
        //                    transaction.gas_price,
        //                    self.options.minimal_gas_price,
        //                );
        //                return Err(transaction::Error::InsufficientGasPrice {
        //                    minimal: self.options.minimal_gas_price,
        //                    got: transaction.gas_price,
        //                });
        //            }
        //        }
        //
        //        let (full_gas_price, overflow_1) = transaction.gas_price.overflowing_mul(transaction.gas);
        //        let (cost, overflow_2) = transaction.value.overflowing_add(full_gas_price);
        //        if overflow_1 || overflow_2 {
        //            trace!(
        //                target: "txqueue",
        //                "[{:?}] Rejected tx, price overflow",
        //                hash
        //            );
        //            return Err(transaction::Error::InsufficientBalance {
        //                cost: U256::max_value(),
        //                balance: account_details.balance,
        //            });
        //        }
        //        if account_details.balance < cost {
        //            debug!(
        //                target: "txqueue",
        //                "[{:?}] Rejected tx with not enough balance: {} < {}",
        //                hash,
        //                account_details.balance,
        //                cost,
        //            );
        //            return Err(transaction::Error::InsufficientBalance {
        //                cost,
        //                balance: account_details.balance,
        //            });
        //        }
        //
        //        if transaction.nonce < account_details.nonce {
        //            debug!(
        //                target: "txqueue",
        //                "[{:?}] Rejected tx with old nonce ({} < {})",
        //                hash,
        //                transaction.nonce,
        //                account_details.nonce,
        //            );
        //            return Err(transaction::Error::Old);
        //        }
        //
        //        let priority = match (is_own || account_details.is_local, is_retracted) {
        //            (true, _) => super::Priority::Local,
        //            (false, false) => super::Priority::Regular,
        //            (false, true) => super::Priority::Retracted,
        //        };
        //        Ok(VerifiedTransaction {
        //            transaction,
        //            priority,
        //            hash,
        //            sender,
        //            insertion_id: self.id.fetch_add(1, atomic::Ordering::AcqRel),
        //        })
    }
}