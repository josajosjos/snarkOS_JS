// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkOS library.

// The snarkOS library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkOS library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkOS library. If not, see <https://www.gnu.org/licenses/>.

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use crate::{Node, Payload, Peer};
use anyhow::*;
use futures::{pin_mut, select, FutureExt};
use hash_hasher::{HashBuildHasher, HashedMap, HashedSet};
use rand::{prelude::SliceRandom, rngs::SmallRng, SeedableRng};
use snarkos_storage::Digest;
use snarkvm_algorithms::crh::double_sha256;
use snarkvm_dpc::{BlockHeader, BlockHeaderHash};
use tokio::{sync::mpsc, time::Instant};

pub enum SyncInbound {
    BlockHashes(SocketAddr, Vec<BlockHeaderHash>),
    Block(SocketAddr, Vec<u8>, Option<u32>),
}

pub struct SyncMaster {
    node: Node,
    incoming: mpsc::Receiver<SyncInbound>,
}

struct SyncBlock {
    address: SocketAddr,
    block: Vec<u8>,
    height: Option<u32>,
}

impl SyncMaster {
    pub fn new(node: Node) -> (Self, mpsc::Sender<SyncInbound>) {
        let (sender, receiver) = mpsc::channel(256);
        let new = Self {
            node,
            incoming: receiver,
        };
        (new, sender)
    }

    async fn find_sync_nodes(&mut self) -> Result<Vec<Peer>> {
        let our_block_height = self.node.storage.canon().await?.block_height;
        let mut interesting_peers = vec![];
        for mut node in self.node.peer_book.connected_peers_snapshot().await {
            let judge_bad = node.judge_bad();
            if !judge_bad && node.quality.block_height as usize > our_block_height + 1 {
                interesting_peers.push(node);
            }
        }
        interesting_peers.sort_by(|x, y| y.quality.block_height.cmp(&x.quality.block_height));

        // trim nodes close to us if any are > 10 blocks ahead
        if let Some(i) = interesting_peers
            .iter()
            .position(|x| x.quality.block_height as usize <= our_block_height + 10)
        {
            interesting_peers.truncate(i + 1);
        }

        if !interesting_peers.is_empty() {
            info!("found {} interesting peers for sync", interesting_peers.len());
            trace!("sync interesting peers = {:?}", interesting_peers);
        }

        Ok(interesting_peers)
    }

    async fn block_locator_hashes(&mut self) -> Result<Vec<Digest>> {
        let forks_of_interest = self.node.expect_sync().consensus.scan_forks().await?;
        trace!("sync found {} forks", forks_of_interest.len());
        let blocks_of_interest: Vec<Digest> = forks_of_interest.into_iter().map(|(_canon, fork)| fork).collect();
        let mut tips_of_blocks_of_interest: Vec<Digest> = Vec::with_capacity(blocks_of_interest.len());
        for block in blocks_of_interest {
            if tips_of_blocks_of_interest.len() > crate::MAX_BLOCK_SYNC_COUNT as usize {
                debug!("reached limit of blocks of interest in sync block locator hashes");
                break;
            }
            let mut fork_path = self.node.storage.longest_child_path(&block).await?;
            if fork_path.len() < 2 {
                // a minor fork, we probably don't care
                continue;
            }
            tips_of_blocks_of_interest.push(fork_path.pop().unwrap());
        }
        match self
            .node
            .storage
            .get_block_locator_hashes(tips_of_blocks_of_interest, snarkos_consensus::OLDEST_FORK_THRESHOLD)
            .await
        {
            Ok(block_locator_hashes) => Ok(block_locator_hashes),
            Err(e) => {
                error!("Unable to get block locator hashes from storage: {:?}", e);
                Err(e)
            }
        }
    }

    async fn send_sync_messages(&mut self, sync_nodes: Vec<Peer>) -> Result<usize> {
        info!("requested block information from {} peers", sync_nodes.len());
        let block_locator_hashes = self
            .block_locator_hashes()
            .await?
            .into_iter()
            .map(|x| BlockHeaderHash(x.bytes::<32>().unwrap()))
            .collect::<Vec<_>>();
        let mut future_set = vec![];

        for peer in sync_nodes.iter() {
            if let Some(handle) = self.node.peer_book.get_peer_handle(peer.address) {
                let locator_hashes = block_locator_hashes.clone();
                future_set.push(async move {
                    handle.send_payload(Payload::GetSync(locator_hashes), None).await;
                });
            }
        }
        let sent = future_set.len();
        futures::future::join_all(future_set).await;
        Ok(sent)
    }

    /// receives an arbitrary amount of inbound sync messages with a given timeout.
    /// if the passed `handler` callback returns `true`, then the loop is terminated early.
    /// if the sync stream closes, the loop is also terminated early.
    async fn receive_messages<F: FnMut(SyncInbound) -> bool>(
        &mut self,
        timeout_sec: u64,
        moving_timeout_sec: u64,
        mut handler: F,
    ) {
        let end = Instant::now() + Duration::from_secs(timeout_sec);
        let mut moving_end = Instant::now() + Duration::from_secs(moving_timeout_sec);
        loop {
            let timeout = tokio::time::sleep_until(end.min(moving_end)).fuse();
            pin_mut!(timeout);
            select! {
                msg = self.incoming.recv().fuse() => {
                    if msg.is_none() {
                        break;
                    }
                    metrics::decrement_gauge!(snarkos_metrics::queues::SYNC_ITEMS, 1.0);
                    if handler(msg.unwrap()) {
                        break;
                    }
                    moving_end += Duration::from_secs(moving_timeout_sec);
                },
                _ = timeout => {
                    break;
                }
            }
        }
    }

    async fn receive_sync_hashes(&mut self, max_message_count: usize) -> HashMap<SocketAddr, Vec<Digest>> {
        const TIMEOUT: u64 = 5;
        let mut received_block_hashes = HashMap::new();

        self.receive_messages(TIMEOUT, TIMEOUT, |msg| {
            match msg {
                SyncInbound::BlockHashes(addr, hashes) => {
                    received_block_hashes.insert(addr, hashes.into_iter().map(|x| -> Digest { x.0.into() }).collect());
                }
                SyncInbound::Block(..) => {
                    warn!("received sync block prematurely");
                }
            }
            //todo: fail if peer sends > 1 block hash packet
            received_block_hashes.len() >= max_message_count
        })
        .await;

        info!(
            "received {} hashes from {} peers in {} seconds",
            received_block_hashes
                .values()
                .map(|x: &Vec<Digest>| x.len())
                .sum::<usize>(),
            received_block_hashes.len(),
            TIMEOUT
        );

        received_block_hashes
    }

    async fn receive_sync_blocks(&mut self, block_count: usize) -> Vec<SyncBlock> {
        const TIMEOUT: u64 = 30;
        let mut blocks = vec![];

        self.receive_messages(TIMEOUT, 4, |msg| {
            match msg {
                SyncInbound::BlockHashes(_, _) => {
                    // late, ignored
                }
                SyncInbound::Block(address, block, height) => {
                    blocks.push(SyncBlock { address, block, height });
                }
            }
            blocks.len() >= block_count
        })
        .await;

        info!("received {} blocks in {} seconds", blocks.len(), TIMEOUT);

        blocks
    }

    fn order_block_hashes(input: &[(SocketAddr, Vec<Digest>)]) -> Vec<Digest> {
        let mut block_order = vec![];
        let mut seen = HashedSet::<&Digest>::with_hasher(HashBuildHasher::default());
        let mut block_index = 0;
        loop {
            let mut found_row = false;
            for (_, hashes) in input {
                if let Some(hash) = hashes.get(block_index) {
                    found_row = true;
                    if seen.contains(&hash) {
                        continue;
                    }
                    seen.insert(hash);
                    block_order.push(hash.clone());
                }
            }
            block_index += 1;
            if !found_row {
                break;
            }
        }
        block_order
    }

    fn block_peer_map(blocks: &[(SocketAddr, Vec<Digest>)]) -> HashedMap<Digest, Vec<SocketAddr>> {
        let mut block_peer_map = HashedMap::with_capacity_and_hasher(blocks.len(), HashBuildHasher::default());
        for (addr, hashes) in blocks {
            for hash in hashes {
                block_peer_map.entry(hash.clone()).or_insert_with(Vec::new).push(*addr);
            }
        }
        block_peer_map
    }

    #[allow(clippy::type_complexity)]
    fn get_peer_blocks(
        &mut self,
        blocks: &[Digest],
        block_peer_map: &HashedMap<Digest, Vec<SocketAddr>>,
    ) -> (
        Vec<SocketAddr>,
        HashedMap<Digest, SocketAddr>,
        HashMap<SocketAddr, Vec<Digest>>,
    ) {
        let mut peer_block_requests: HashMap<SocketAddr, Vec<Digest>> = HashMap::new();
        let mut block_peers = HashedMap::with_hasher(HashBuildHasher::default());
        for block in blocks {
            let peers = block_peer_map.get(block);
            if peers.is_none() {
                continue;
            }
            let random_peer = peers.unwrap().choose(&mut SmallRng::from_entropy());
            if random_peer.is_none() {
                continue;
            }
            block_peers.insert(block.clone(), *random_peer.unwrap());
            peer_block_requests
                .entry(*random_peer.unwrap())
                .or_insert_with(Vec::new)
                .push(block.clone());
        }
        let addresses: Vec<SocketAddr> = peer_block_requests.keys().copied().collect();
        (addresses, block_peers, peer_block_requests)
    }

    async fn request_blocks(&mut self, peer_block_requests: HashMap<SocketAddr, Vec<Digest>>) -> usize {
        let mut sent = 0usize;

        let mut future_set = vec![];
        for (addr, request) in peer_block_requests {
            if let Some(peer) = self.node.peer_book.get_peer_handle(addr) {
                sent += request.len();
                let request: Vec<BlockHeaderHash> = request
                    .into_iter()
                    .map(|x| BlockHeaderHash(x.bytes::<32>().unwrap()))
                    .collect();
                future_set.push(async move {
                    peer.expecting_sync_blocks(request.len() as u32).await;
                    peer.send_payload(Payload::GetBlocks(request), None).await;
                });
            }
        }
        futures::future::join_all(future_set).await;
        sent
    }

    async fn cancel_outstanding_syncs(&mut self, addresses: &[SocketAddr]) {
        let mut future_set = vec![];
        for addr in addresses {
            if let Some(peer) = self.node.peer_book.get_peer_handle(*addr) {
                future_set.push(async move {
                    peer.cancel_sync().await;
                });
            }
        }
        futures::future::join_all(future_set).await;
    }

    pub async fn run(mut self) -> Result<()> {
        let sync_nodes = self.find_sync_nodes().await?;

        if sync_nodes.is_empty() {
            return Ok(());
        }

        self.node.register_block_sync_attempt();

        let hash_requests_sent = self.send_sync_messages(sync_nodes).await?;

        if hash_requests_sent == 0 {
            return Ok(());
        }

        let received_block_hashes = self.receive_sync_hashes(hash_requests_sent).await;

        if received_block_hashes.is_empty() {
            return Ok(());
        }

        let blocks = received_block_hashes.into_iter().collect::<Vec<_>>();

        let early_blocks = Self::order_block_hashes(&blocks[..]);
        let early_blocks_count = early_blocks.len();

        let early_block_states = self.node.storage.get_block_states(&early_blocks[..]).await?;
        let block_order: Vec<_> = early_blocks
            .into_iter()
            .zip(early_block_states.iter())
            .filter(|(_, status)| matches!(status, snarkos_storage::BlockStatus::Unknown))
            .map(|(hash, _)| hash)
            .collect();

        info!(
            "requesting {} blocks for sync, received headers for {} known blocks",
            block_order.len(),
            early_blocks_count - block_order.len()
        );
        if block_order.is_empty() {
            return Ok(());
        }

        let block_peer_map = Self::block_peer_map(&blocks[..]);

        let (peer_addresses, block_peers, peer_block_requests) =
            self.get_peer_blocks(&block_order[..], &block_peer_map);

        let sent_block_requests = self.request_blocks(peer_block_requests).await;

        let received_blocks = self.receive_sync_blocks(sent_block_requests).await;

        info!(
            "received {}/{} blocks for sync",
            received_blocks.len(),
            sent_block_requests
        );

        self.cancel_outstanding_syncs(&peer_addresses[..]).await;

        let mut blocks_by_hash: HashedMap<Digest, _> =
            HashedMap::with_capacity_and_hasher(received_blocks.len(), HashBuildHasher::default());

        for block in received_blocks {
            let block_header = &block.block[..BlockHeader::size()];
            let hash = double_sha256(block_header).into();
            blocks_by_hash.insert(hash, block);
        }

        for (i, hash) in block_order.iter().enumerate() {
            if let Some(block) = blocks_by_hash.remove(hash) {
                self.node
                    .process_received_block(block.address, block.block, block.height, false)
                    .await?;
            } else {
                warn!(
                    "did not receive block {}/{} ({}...) from {} by sync deadline",
                    i,
                    block_order.len(),
                    &hash.to_string()[..8],
                    block_peers.get(hash).map(|x| x.to_string()).unwrap_or_default(),
                );
            }
        }

        Ok(())
    }
}

impl Drop for SyncMaster {
    fn drop(&mut self) {
        metrics::gauge!(snarkos_metrics::queues::SYNC_ITEMS, 0.0);
    }
}
