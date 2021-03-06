// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2

use crate::chain_watcher::{ChainWatcher, WatchBlock, WatchTxn};
use crate::pubsub_client::PubSubClient;
use actix::{Addr, System};
use failure::Fail;
use futures::channel::oneshot;
use futures::{future::FutureExt, select, stream::StreamExt, TryStream, TryStreamExt};
use futures01::future::Future as Future01;
use jsonrpc_core::{MetaIoHandler, Metadata};
use jsonrpc_core_client::{transports::ipc, transports::local, transports::ws, RpcChannel};
use starcoin_crypto::HashValue;
use starcoin_logger::{prelude::*, LogPattern};
use starcoin_rpc_api::node::NodeInfo;
use starcoin_rpc_api::types::event::Event;
use starcoin_rpc_api::types::pubsub::EventFilter;
use starcoin_rpc_api::types::pubsub::ThinBlock;
use starcoin_rpc_api::{
    chain::ChainClient, debug::DebugClient, node::NodeClient, state::StateClient,
    txpool::TxPoolClient, wallet::WalletClient,
};
use starcoin_state_api::StateWithProof;
use starcoin_types::access_path::AccessPath;
use starcoin_types::account_address::AccountAddress;
use starcoin_types::account_state::AccountState;
use starcoin_types::block::{Block, BlockNumber};
use starcoin_types::peer_info::PeerInfo;
use starcoin_types::startup_info::ChainInfo;
use starcoin_types::transaction::{
    RawUserTransaction, SignedUserTransaction, Transaction, TransactionInfo,
};
use starcoin_wallet_api::WalletAccount;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio01::reactor::Reactor;
use tokio_compat::prelude::*;
use tokio_compat::runtime::Runtime;

pub mod chain_watcher;
mod pubsub_client;
mod remote_state_reader;
pub use crate::remote_state_reader::RemoteStateReader;

#[derive(Debug, Clone)]
enum ConnSource {
    Ipc(PathBuf, Arc<Reactor>),
    WebSocket(String),
    Local,
}

pub struct RpcClient {
    inner: RefCell<Option<RpcClientInner>>,
    rt: RefCell<Runtime>,
    conn_source: ConnSource,
    chain_watcher: Addr<ChainWatcher>,
}

struct ConnectionProvider {
    conn_source: ConnSource,
}

#[derive(Error, Debug)]
pub enum ConnError {
    #[error("io error, {0}")]
    Io(#[from] std::io::Error),
    #[error("rpc error, {0}")]
    RpcError(jsonrpc_client_transports::RpcError),
}

impl ConnectionProvider {
    async fn get_rpc_channel(&self) -> anyhow::Result<RpcChannel, ConnError> {
        match &self.conn_source {
            ConnSource::Ipc(sock_path, reactor) => {
                let conn_fut = ipc::connect(sock_path, &reactor.handle())?;
                conn_fut.compat().await.map_err(ConnError::RpcError)
            }
            // only have ipc impl for now
            _ => unreachable!(),
        }
    }
}

impl RpcClient {
    pub(crate) fn new(conn_source: ConnSource, inner: RpcClientInner, mut rt: Runtime) -> Self {
        let (tx, rx) = oneshot::channel();
        let pubsub_client = inner.pubsub_client.clone();
        std::thread::spawn(move || {
            let sys = System::new("client-actix-system");
            let watcher = ChainWatcher::launch(pubsub_client);

            tx.send(watcher).unwrap();
            let _ = sys.run();
        });
        let watcher = rt.block_on_std(rx).unwrap();

        Self {
            inner: RefCell::new(Some(inner)),
            rt: RefCell::new(rt),
            conn_source,
            chain_watcher: watcher,
        }
    }
    pub fn connect_websocket(url: &str) -> anyhow::Result<Self> {
        let mut rt = Runtime::new().unwrap();

        let conn = ws::try_connect(url).map_err(|e| anyhow::Error::new(e.compat()))?;
        let client = rt.block_on(conn.map_err(map_err))?;
        Ok(Self::new(
            ConnSource::WebSocket(url.to_string()),
            client,
            rt,
        ))
    }

    pub fn connect_local<THandler, TMetadata>(handler: THandler) -> Self
    where
        THandler: Deref<Target = MetaIoHandler<TMetadata>> + std::marker::Send + 'static,
        TMetadata: Metadata + Default,
    {
        let rt = Runtime::new().unwrap();
        let (client, future) = local::connect(handler);
        // process server event interval.
        // TODO use more graceful method.
        rt.spawn_std(async {
            let mut future = future
                .map_err(|e| error!("rpc error: {:?}", e))
                .compat()
                .fuse();
            let mut timer = tokio::time::interval(Duration::from_millis(10)).fuse();
            loop {
                select! {
                res = future => {
                },
                t = timer.select_next_some() =>{
                }
                complete => break,
                };
            }
        });
        Self::new(ConnSource::Local, client, rt)
    }

    pub fn connect_ipc<P: AsRef<Path>>(sock_path: P) -> anyhow::Result<Self> {
        let mut rt = Runtime::new().unwrap();
        let reactor = Reactor::new().unwrap();
        let path = sock_path.as_ref().to_path_buf();
        let fut = ipc::connect(sock_path, &reactor.handle())?;
        let client_inner = rt.block_on(fut.map_err(map_err))?;

        Ok(Self::new(
            ConnSource::Ipc(path, Arc::new(reactor)),
            client_inner,
            rt,
        ))
    }

    pub fn watch_txn(
        &self,
        txn_hash: HashValue,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ThinBlock> {
        let f = async move {
            let r = self.chain_watcher.send(WatchTxn { txn_hash }).await?;
            match timeout {
                Some(t) => tokio::time::timeout(t, r).await??,
                None => r.await?,
            }
        };
        self.rt.borrow_mut().block_on_std(f)
    }
    pub fn watch_block(&self, block_number: BlockNumber) -> anyhow::Result<ThinBlock> {
        let f = async move {
            let r = self.chain_watcher.send(WatchBlock(block_number)).await?;
            r.await?
        };
        self.rt.borrow_mut().block_on_std(f)
    }

    pub fn node_status(&self) -> anyhow::Result<bool> {
        self.call_rpc_blocking(|inner| async move { inner.node_client.status().compat().await })
            .map_err(map_err)
    }

    pub fn node_info(&self) -> anyhow::Result<NodeInfo> {
        self.call_rpc_blocking(|inner| async move { inner.node_client.info().compat().await })
            .map_err(map_err)
    }
    pub fn node_metrics(&self) -> anyhow::Result<HashMap<String, String>> {
        self.call_rpc_blocking(|inner| async move { inner.node_client.metrics().compat().await })
            .map_err(map_err)
    }

    pub fn node_peers(&self) -> anyhow::Result<Vec<PeerInfo>> {
        self.call_rpc_blocking(|inner| async move { inner.node_client.peers().compat().await })
            .map_err(map_err)
    }

    pub fn next_sequence_number_in_txpool(
        &self,
        address: AccountAddress,
    ) -> anyhow::Result<Option<u64>> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .txpool_client
                .next_sequence_number(address)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn submit_transaction(
        &self,
        txn: SignedUserTransaction,
    ) -> anyhow::Result<Result<(), anyhow::Error>> {
        self.call_rpc_blocking(|inner| async move {
            inner.txpool_client.submit_transaction(txn).compat().await
        })
        .map(|r| r.map_err(|e| anyhow::format_err!("{}", e)))
        .map_err(map_err)
    }
    //TODO should split client for different api ?
    // such as  RpcClient().account().default()
    pub fn wallet_default(&self) -> anyhow::Result<Option<WalletAccount>> {
        self.call_rpc_blocking(|inner| async move { inner.wallet_client.default().compat().await })
            .map_err(map_err)
    }

    pub fn wallet_create(&self, password: String) -> anyhow::Result<WalletAccount> {
        self.call_rpc_blocking(|inner| async move {
            inner.wallet_client.create(password).compat().await
        })
        .map_err(map_err)
    }

    pub fn wallet_list(&self) -> anyhow::Result<Vec<WalletAccount>> {
        self.call_rpc_blocking(|inner| async move { inner.wallet_client.list().compat().await })
            .map_err(map_err)
    }

    pub fn wallet_get(&self, address: AccountAddress) -> anyhow::Result<Option<WalletAccount>> {
        self.call_rpc_blocking(
            |inner| async move { inner.wallet_client.get(address).compat().await },
        )
        .map_err(map_err)
    }

    /// partial sign a multisig account's txn
    pub fn wallet_sign_multisig_txn(
        &self,
        raw_txn: RawUserTransaction,
        signer_address: AccountAddress,
    ) -> anyhow::Result<SignedUserTransaction> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .wallet_client
                .sign_txn(raw_txn, signer_address)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn wallet_sign_txn(
        &self,
        raw_txn: RawUserTransaction,
    ) -> anyhow::Result<SignedUserTransaction> {
        let signer = raw_txn.sender();
        self.call_rpc_blocking(|inner| async move {
            inner.wallet_client.sign_txn(raw_txn, signer).compat().await
        })
        .map_err(map_err)
    }

    pub fn wallet_unlock(
        &self,
        address: AccountAddress,
        password: String,
        duration: std::time::Duration,
    ) -> anyhow::Result<()> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .wallet_client
                .unlock(address, password, duration)
                .compat()
                .await
        })
        .map_err(map_err)
    }
    pub fn wallet_export(
        &self,
        address: AccountAddress,
        password: String,
    ) -> anyhow::Result<Vec<u8>> {
        self.call_rpc_blocking(|inner| async move {
            inner.wallet_client.export(address, password).compat().await
        })
        .map_err(map_err)
    }
    pub fn wallet_import(
        &self,
        address: AccountAddress,
        private_key: Vec<u8>,
        password: String,
    ) -> anyhow::Result<WalletAccount> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .wallet_client
                .import(address, private_key, password)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn state_get(&self, access_path: AccessPath) -> anyhow::Result<Option<Vec<u8>>> {
        self.call_rpc_blocking(
            |inner| async move { inner.state_client.get(access_path).compat().await },
        )
        .map_err(map_err)
    }

    pub fn state_get_with_proof(&self, access_path: AccessPath) -> anyhow::Result<StateWithProof> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .state_client
                .get_with_proof(access_path)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn state_get_state_root(&self) -> anyhow::Result<HashValue> {
        self.call_rpc_blocking(
            |inner| async move { inner.state_client.get_state_root().compat().await },
        )
        .map_err(map_err)
    }

    pub fn state_get_account_state(
        &self,
        address: AccountAddress,
    ) -> anyhow::Result<Option<AccountState>> {
        self.call_rpc_blocking(|inner| async move {
            inner.state_client.get_account_state(address).compat().await
        })
        .map_err(map_err)
    }

    pub fn debug_set_log_level(
        &self,
        logger_name: Option<String>,
        level: Level,
    ) -> anyhow::Result<()> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .debug_client
                .set_log_level(logger_name, level.to_string())
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn debug_set_log_pattern(&self, pattern: LogPattern) -> anyhow::Result<()> {
        self.call_rpc_blocking(|inner| async move {
            inner.debug_client.set_log_pattern(pattern).compat().await
        })
        .map_err(map_err)
    }

    pub fn debug_panic(&self) -> anyhow::Result<()> {
        self.call_rpc_blocking(|inner| async move { inner.debug_client.panic().compat().await })
            .map_err(map_err)
    }

    pub fn chain_head(&self) -> anyhow::Result<ChainInfo> {
        self.call_rpc_blocking(|inner| async move { inner.chain_client.head().compat().await })
            .map_err(map_err)
    }

    pub fn chain_get_block_by_hash(&self, hash: HashValue) -> anyhow::Result<Block> {
        self.call_rpc_blocking(|inner| async move {
            inner.chain_client.get_block_by_hash(hash).compat().await
        })
        .map_err(map_err)
    }

    pub fn chain_get_block_by_number(&self, number: BlockNumber) -> anyhow::Result<Block> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .chain_client
                .get_block_by_number(number)
                .compat()
                .await
        })
        .map_err(map_err)
    }
    pub fn chain_get_blocks_by_number(
        &self,
        number: Option<BlockNumber>,
        count: u64,
    ) -> anyhow::Result<Vec<Block>> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .chain_client
                .get_blocks_by_number(number, count)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn chain_get_transaction(&self, txn_id: HashValue) -> anyhow::Result<Transaction> {
        self.call_rpc_blocking(|inner| async move {
            inner.chain_client.get_transaction(txn_id).compat().await
        })
        .map_err(map_err)
    }

    pub fn chain_get_txn_by_block(
        &self,
        block_id: HashValue,
    ) -> anyhow::Result<Vec<TransactionInfo>> {
        self.call_rpc_blocking(|inner| async move {
            inner.chain_client.get_txn_by_block(block_id).compat().await
        })
        .map_err(map_err)
    }

    pub fn chain_get_txn_info_by_block_and_index(
        &self,
        block_id: HashValue,
        idx: u64,
    ) -> anyhow::Result<Option<TransactionInfo>> {
        self.call_rpc_blocking(|inner| async move {
            inner
                .chain_client
                .get_txn_info_by_block_and_index(block_id, idx)
                .compat()
                .await
        })
        .map_err(map_err)
    }

    pub fn chain_branches(&self) -> anyhow::Result<Vec<ChainInfo>> {
        self.call_rpc_blocking(|inner| async move { inner.chain_client.branches().compat().await })
            .map_err(map_err)
    }

    pub fn subscribe_events(
        &self,
        filter: EventFilter,
    ) -> anyhow::Result<impl TryStream<Ok = Event, Error = anyhow::Error>> {
        self.call_rpc_blocking(|inner| async move {
            let res = inner.pubsub_client.subscribe_events(filter).await;
            res.map(|s| s.compat().map_err(map_err))
        })
        .map_err(map_err)
    }
    pub fn subscribe_new_blocks(
        &self,
    ) -> anyhow::Result<impl TryStream<Ok = ThinBlock, Error = anyhow::Error>> {
        self.call_rpc_blocking(|inner| async move {
            let res = inner.pubsub_client.subscribe_new_block().await;
            res.map(|s| s.compat().map_err(map_err))
        })
        .map_err(map_err)
    }
    pub fn subscribe_new_transactions(
        &self,
    ) -> anyhow::Result<impl TryStream<Ok = Vec<HashValue>, Error = anyhow::Error>> {
        self.call_rpc_blocking(|inner| async move {
            let res = inner.pubsub_client.subscribe_new_transactions().await;
            res.map(|s| s.compat().map_err(map_err))
        })
        .map_err(map_err)
    }

    fn call_rpc_blocking<F, T>(
        &self,
        f: impl FnOnce(RpcClientInner) -> F,
    ) -> Result<T, jsonrpc_client_transports::RpcError>
    where
        F: std::future::Future<Output = Result<T, jsonrpc_client_transports::RpcError>>,
    {
        let inner_opt = self.inner.borrow().as_ref().cloned();
        let inner = match inner_opt {
            Some(inner) => inner,
            None => {
                let new_inner: RpcClientInner = self.rt.borrow_mut().block_on_std(async {
                    Self::get_rpc_channel(self.conn_source.clone())
                        .await
                        .map(|c| c.into())
                })?;
                *self.inner.borrow_mut() = Some(new_inner.clone());
                new_inner
            }
        };

        let result = self.rt.borrow_mut().block_on_std(async { f(inner).await });

        if let Err(rpc_error) = &result {
            if let jsonrpc_client_transports::RpcError::Other(e) = rpc_error {
                error!("rpc error due to {:?}", e);
                *self.inner.borrow_mut() = None;
            }
        }

        result
    }

    async fn get_rpc_channel(
        conn_source: ConnSource,
    ) -> anyhow::Result<RpcChannel, jsonrpc_client_transports::RpcError> {
        let conn_provider = ConnectionProvider { conn_source };
        match conn_provider.get_rpc_channel().await {
            Ok(channel) => Ok(channel),
            Err(ConnError::RpcError(e)) => Err(e),
            Err(ConnError::Io(e)) => Err(jsonrpc_client_transports::RpcError::Other(
                failure::Error::from(e),
            )),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RpcClientInner {
    node_client: NodeClient,
    txpool_client: TxPoolClient,
    wallet_client: WalletClient,
    state_client: StateClient,
    debug_client: DebugClient,
    chain_client: ChainClient,
    pubsub_client: PubSubClient,
}

impl RpcClientInner {
    pub fn new(channel: RpcChannel) -> Self {
        Self {
            node_client: channel.clone().into(),
            txpool_client: channel.clone().into(),
            wallet_client: channel.clone().into(),
            state_client: channel.clone().into(),
            debug_client: channel.clone().into(),
            chain_client: channel.clone().into(),
            pubsub_client: channel.into(),
        }
    }
}

fn map_err(rpc_err: jsonrpc_client_transports::RpcError) -> anyhow::Error {
    rpc_err.compat().into()
}

impl From<RpcChannel> for RpcClientInner {
    fn from(channel: RpcChannel) -> Self {
        Self::new(channel)
    }
}
