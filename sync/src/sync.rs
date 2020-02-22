use crate::download::{DownloadActor, Downloader};
use crate::message::{BlockBody, HashWithNumber, SyncMessage};
use crate::pool::TTLPool;
use crate::process::{ProcessActor, Processor};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler};
use anyhow::Result;
use atomic_refcell::AtomicRefCell;
use chain::{mem_chain::MemChain, ChainActor};
use config::NodeConfig;
use network::NetworkActor;
use std::sync::Arc;
use types::{block::BlockHeader, peer_info::PeerInfo};

pub struct SyncActor {
    process_address: Addr<ProcessActor>,
    download_address: Addr<DownloadActor>,
}

impl SyncActor {
    pub fn launch(
        // _node_config: &NodeConfig,
        // _network: Addr<NetworkActor>,
        //        chain: Addr<ChainActor>,
        process_address: Addr<ProcessActor>,
        download_address: Addr<DownloadActor>,
    ) -> Result<Addr<SyncActor>> {
        let actor = SyncActor {
            download_address,
            process_address,
        };
        Ok(actor.start())
    }
}

impl Actor for SyncActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("Sync actor started");
    }
}

impl Handler<SyncMessage> for SyncActor {
    type Result = ();

    fn handle(&mut self, msg: SyncMessage, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            SyncMessage::DownloadMessage(download_msg) => {
                self.download_address
                    .send(download_msg)
                    .into_actor(self)
                    .then(|_result, act, _ctx| async {}.into_actor(act))
                    .wait(ctx);
            }
            SyncMessage::ProcessMessage(process_msg) => {
                self.process_address
                    .send(process_msg)
                    .into_actor(self)
                    .then(|_result, act, _ctx| async {}.into_actor(act))
                    .wait(ctx);
            }
        }
    }
}

pub struct SyncFlow {
    pub downloader: Downloader,
    pub processor: Processor,
    pub peer_info: PeerInfo,
}

impl SyncFlow {
    pub fn new(peer_info: PeerInfo, chain_reader: Arc<AtomicRefCell<MemChain>>) -> Self {
        let downloader = Downloader::new(chain_reader.clone());
        let processor = Processor::new(chain_reader);
        SyncFlow {
            downloader,
            processor,
            peer_info,
        }
    }
}