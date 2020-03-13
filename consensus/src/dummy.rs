// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{Consensus, ConsensusHeader};
use anyhow::{Error, Result};
use config::NodeConfig;
use futures::channel::oneshot::Receiver;
use std::convert::TryFrom;
use std::sync::Arc;
use traits::ChainReader;
use types::block::{Block, BlockHeader, BlockTemplate};
use rand::{thread_rng, Rng};
use std::thread;
use std::time::Duration;

pub struct DummyHeader {}

impl ConsensusHeader for DummyHeader {}

impl TryFrom<Vec<u8>> for DummyHeader {
    type Error = Error;

    fn try_from(_value: Vec<u8>) -> Result<Self> {
        Ok(DummyHeader {})
    }
}

impl Into<Vec<u8>> for DummyHeader {
    fn into(self) -> Vec<u8> {
        vec![]
    }
}

pub struct DummyConsensus {}

impl Consensus for DummyConsensus {
    fn init_genesis_header(_config: Arc<NodeConfig>) -> Vec<u8> {
        vec![]
    }

    fn verify_header(
        _config: Arc<NodeConfig>,
        _reader: &dyn ChainReader,
        _header: &BlockHeader,
    ) -> Result<()> {
        Ok(())
    }

    fn create_block(
        _config: Arc<NodeConfig>,
        _reader: &dyn ChainReader,
        block_template: BlockTemplate,
        _cancel: Receiver<()>,
    ) -> Result<Block> {
        let mut rng = thread_rng();
        let time: u64 = rng.gen_range(1, 7);
        thread::sleep(Duration::from_secs(time));
        Ok(block_template.into_block(DummyHeader {}))
    }
}
