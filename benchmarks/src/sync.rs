use crate::random_txn;
use actix::clock::{delay_for, Duration};
use actix::{Addr, Arbiter, System};
use anyhow::Result;
use criterion::{BatchSize, Bencher};
use crypto::HashValue;
use libp2p::multiaddr::Multiaddr;
use logger::prelude::*;
use starcoin_bus::BusActor;
use starcoin_chain::{BlockChain, ChainActor, ChainActorRef};
use starcoin_config::{get_available_port, NodeConfig};
use starcoin_consensus::dummy::DummyConsensus;
use starcoin_genesis::Genesis;
use starcoin_network::{NetworkActor, NetworkAsyncService, RawRpcRequestMessage};
use starcoin_network_api::NetworkService;
use starcoin_sync::Downloader;
use starcoin_sync::{
    helper::{get_body_by_hash, get_headers, get_headers_msg_for_common, get_info_by_hash},
    ProcessActor,
};
use starcoin_txpool::{TxPool, TxPoolService};
use starcoin_wallet_api::WalletAccount;
use std::sync::Arc;
use storage::cache_storage::CacheStorage;
use storage::storage::StorageInstance;
use storage::Storage;
use tokio::runtime::Handle;
use traits::{ChainAsyncService, Consensus};
use types::peer_info::{PeerId, PeerInfo};

/// Benchmarking support for sync.
pub struct SyncBencher;

impl SyncBencher {
    pub fn sync_block(&self, num: u64) {
        let mut system = System::new("sync-bench");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        system.block_on(async move {
            let (_bus_1, addr_1, network_1, chain_1, tx_1, storage_1, rpc_rx) =
                create_node(Some(num), None, handle.clone()).await.unwrap();
            let chain_1_clone = chain_1.clone();
            let _processor = Arbiter::new()
                .exec(move || -> Addr<ProcessActor<DummyConsensus>> {
                    ProcessActor::launch(chain_1_clone, tx_1, storage_1, rpc_rx).unwrap()
                })
                .await
                .unwrap();

            let (_, _, network_2, chain_2, _, _, _) =
                create_node(None, Some((addr_1, network_1)), handle.clone())
                    .await
                    .unwrap();
            let chain_2_clone = chain_2.clone();
            let downloader = Arc::new(Downloader::new(chain_2_clone));
            for i in 0..3 {
                SyncBencher::sync_block_inner(downloader.clone(), network_2.clone())
                    .await
                    .unwrap();
                let first = chain_1.clone().master_head().await.unwrap();
                let second = chain_2.clone().master_head().await.unwrap();
                if first.get_head() != second.get_head() {
                    info!("full sync failed: {}", i);
                    delay_for(Duration::from_millis(1000)).await;
                } else {
                    break;
                }
            }
            let first = chain_1.clone().master_head().await.unwrap();
            let second = chain_2.clone().master_head().await.unwrap();
            assert_eq!(first.get_head(), second.get_head());
            info!("full sync test ok.");
        });
    }

    pub fn bench_full_sync(&self, b: &mut Bencher, num: u64) {
        b.iter_batched(
            || (self, num),
            |(bench, n)| bench.sync_block(n),
            BatchSize::LargeInput,
        )
    }

    async fn sync_block_inner(
        downloader: Arc<Downloader<DummyConsensus>>,
        network: NetworkAsyncService,
    ) -> Result<()> {
        if let Some(best_peer) = network.best_peer().await? {
            if let Some(header) = downloader.get_chain_reader().master_head_header().await? {
                let begin_number = header.number();
                let end_number = best_peer.get_block_number();

                if let Some(ancestor_header) = downloader
                    .find_ancestor_header(
                        best_peer.get_peer_id(),
                        network.clone(),
                        begin_number,
                        true,
                    )
                    .await?
                {
                    let mut latest_block_id = ancestor_header.id();
                    let mut latest_number = ancestor_header.number();
                    loop {
                        if end_number <= latest_number {
                            break;
                        }
                        let get_headers_req = get_headers_msg_for_common(latest_block_id);
                        let headers = get_headers(&network, get_headers_req).await?;
                        let latest_header = headers.last().expect("headers is empty.");
                        latest_block_id = latest_header.id();
                        latest_number = latest_header.number();
                        let hashs: Vec<HashValue> =
                            headers.iter().map(|header| header.id()).collect();
                        let bodies = get_body_by_hash(&network, hashs.clone()).await?;
                        let infos = get_info_by_hash(&network, hashs).await?;
                        info!(
                            "sync block number : {:?} from peer {:?}",
                            latest_number,
                            best_peer.get_peer_id()
                        );
                        downloader.do_blocks(headers, bodies, infos).await;
                    }
                }
            }
        }

        Ok(())
    }
}

async fn create_node(
    num: Option<u64>,
    seed: Option<(Multiaddr, NetworkAsyncService)>,
    handle: Handle,
) -> Result<(
    Addr<BusActor>,
    Multiaddr,
    NetworkAsyncService,
    ChainActorRef<DummyConsensus>,
    TxPoolService,
    Arc<Storage>,
    futures::channel::mpsc::UnboundedReceiver<RawRpcRequestMessage>,
)> {
    let bus = BusActor::launch();
    // storage
    let storage =
        Arc::new(Storage::new(StorageInstance::new_cache_instance(CacheStorage::new())).unwrap());
    // node config
    let mut config = NodeConfig::random_for_test();
    config.sync.full_sync_mode();
    let my_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", get_available_port())
        .parse()
        .unwrap();
    config.network.listen = my_addr.clone();
    if let Some((seed_listen, seed_net)) = seed {
        let seed_id = seed_net.identify().to_base58();
        let seed_addr: Multiaddr = format!("{}/p2p/{}", &seed_listen, seed_id).parse().unwrap();
        config.network.seeds = vec![seed_addr];
    }
    let node_config = Arc::new(config);

    // genesis
    let genesis = Genesis::load(node_config.net()).unwrap();
    let genesis_hash = genesis.block().header().id();

    let genesis_startup_info = genesis.execute(storage.clone()).unwrap();
    let txpool = {
        let best_block_id = *genesis_startup_info.get_master();
        TxPool::start(
            node_config.tx_pool.clone(),
            storage.clone(),
            best_block_id,
            bus.clone(),
        )
    };

    let txpool_service = txpool.get_service();

    // network
    let key_pair = node_config.clone().network.network_keypair();
    let addr = PeerId::from_ed25519_public_key(key_pair.public_key.clone());
    let mut rpc_proto_info = Vec::new();
    let sync_rpc_proto_info = starcoin_sync::helper::sync_rpc_info();
    rpc_proto_info.push((sync_rpc_proto_info.0.into(), sync_rpc_proto_info.1));
    let node_config_clone = node_config.clone();
    let bus_clone = bus.clone();
    let handle_clone = handle.clone();
    let addr_clone = addr.clone();
    let (network, rpc_rx) = NetworkActor::launch(
        node_config_clone,
        bus_clone,
        handle_clone,
        genesis_hash,
        PeerInfo::new_for_test(addr_clone, rpc_proto_info),
    );

    // chain
    let txpool_service_clone = txpool_service.clone();
    let node_config_clone = node_config.clone();
    let genesis_startup_info_clone = genesis_startup_info.clone();
    let storage_clone = storage.clone();
    let bus_clone = bus.clone();
    let chain = Arbiter::new()
        .exec(move || -> ChainActorRef<DummyConsensus> {
            ChainActor::launch(
                node_config_clone,
                genesis_startup_info_clone,
                storage_clone,
                bus_clone,
                txpool_service_clone,
            )
            .unwrap()
        })
        .await?;

    if let Some(n) = num {
        let miner_account = WalletAccount::random();
        for i in 0..n {
            info!(
                "create block: {:?} : {:?}",
                &node_config.network.self_peer_id, i
            );
            let startup_info = chain.clone().master_startup_info().await?;

            let block_chain = BlockChain::<DummyConsensus, Storage>::new(
                node_config.clone(),
                startup_info.master,
                storage.clone(),
            )
            .unwrap();

            let mut txn_vec = Vec::new();
            txn_vec.push(random_txn(i + 1));
            let block_template = chain
                .clone()
                .create_block_template(
                    *miner_account.address(),
                    Some(miner_account.get_auth_key().prefix().to_vec()),
                    None,
                    txn_vec,
                )
                .await
                .unwrap()
                .unwrap();
            let block =
                DummyConsensus::create_block(node_config.clone(), &block_chain, block_template)
                    .unwrap();
            chain.clone().try_connect(block).await.unwrap().unwrap();
        }
    }
    Ok((
        bus,
        my_addr,
        network,
        chain,
        txpool_service,
        storage,
        rpc_rx,
    ))
}
