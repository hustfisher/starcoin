// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::TxPool;
use anyhow::Result;
use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use types::transaction::SignedUserTransaction;

#[derive(Clone)]
pub struct MockTxPool {
    pool: Arc<Mutex<Vec<SignedUserTransaction>>>,
}

impl MockTxPool {
    pub fn new() -> Self {
        Self::new_with_txns(vec![])
    }

    pub fn new_with_txns(txns: Vec<SignedUserTransaction>) -> Self {
        MockTxPool {
            pool: Arc::new(Mutex::new(txns)),
        }
    }
}

#[async_trait::async_trait]
impl TxPool for MockTxPool {
    async fn add(self, txn: SignedUserTransaction) -> Result<bool> {
        self.pool.lock().unwrap().push(txn);
        //TODO check txn is exist.
        Ok(true)
    }

    async fn get_pending_txns(self) -> Result<Vec<SignedUserTransaction>> {
        Ok(self.pool.lock().unwrap().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_rt::test]
    async fn test_txpool() {
        let pool = MockTxPool::new();

        pool.clone()
            .add(SignedUserTransaction::mock())
            .await
            .unwrap();
        let txns = pool.get_pending_txns().await.unwrap();
        assert_eq!(1, txns.len())
    }
}