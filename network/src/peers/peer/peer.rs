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

use anyhow::*;
use chrono::Utc;
use futures::{select_biased, FutureExt};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use super::PeerQuality;
use crate::{message::Payload, BlockCache, NetworkError, Node};

use super::{network::*, outbound_handler::*};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum PeerStatus {
    Connected,
    Connecting,
    Disconnected,
}

impl Default for PeerStatus {
    fn default() -> Self {
        PeerStatus::Disconnected
    }
}

/// A data structure containing information about a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub address: SocketAddr,
    #[serde(skip)]
    pub status: PeerStatus,
    pub quality: PeerQuality,
    pub is_bootnode: bool,
    #[serde(skip)]
    pub queued_inbound_message_count: Arc<AtomicUsize>,
    #[serde(skip)]
    pub queued_outbound_message_count: Arc<AtomicUsize>,
    /// Whether this peer is routable or not.
    ///
    /// `None` indicates the node has never attempted a connection with this peer.
    pub is_routable: Option<bool>,

    #[serde(skip)]
    pub block_received_cache: BlockCache<{ crate::PEER_BLOCK_CACHE_SIZE }>,
}

const FAILURE_EXPIRY_TIME: Duration = Duration::from_secs(15 * 60);
const FAILURE_THRESHOLD: usize = 5;

impl Peer {
    pub fn new(address: SocketAddr, is_bootnode: bool) -> Self {
        Self {
            address,
            status: PeerStatus::Disconnected,
            quality: Default::default(),
            is_bootnode,
            queued_inbound_message_count: Default::default(),
            queued_outbound_message_count: Default::default(),

            // Set to `None` since peer creation only ever happens before a connection to the peer,
            // therefore we don't know if its listener is routable or not.
            is_routable: None,
            block_received_cache: BlockCache::default(),
        }
    }

    pub fn judge_bad(&mut self) -> bool {
        let f = self.failures();
        // self.quality.rtt_ms > 1500 ||
        f >= FAILURE_THRESHOLD || self.quality.is_inactive(chrono::Utc::now())
    }

    pub fn judge_bad_offline(&mut self) -> bool {
        self.failures() >= FAILURE_THRESHOLD
    }

    pub fn fail(&mut self) {
        self.quality.failures.push(Utc::now());
    }

    pub fn failures(&mut self) -> usize {
        let now = Utc::now();
        if self.quality.failures.len() >= FAILURE_THRESHOLD {
            self.quality.failures = self
                .quality
                .failures
                .iter()
                .filter(|x| now.signed_duration_since(**x) < chrono::Duration::from_std(FAILURE_EXPIRY_TIME).unwrap())
                .copied()
                .collect();
        }
        self.quality.failures.len()
    }

    pub fn handshake_timeout(&self) -> Duration {
        if self.is_bootnode {
            Duration::from_secs(crate::HANDSHAKE_BOOTNODE_TIMEOUT_SECS as u64)
        } else {
            Self::peer_handshake_timeout()
        }
    }

    pub fn peer_handshake_timeout() -> Duration {
        Duration::from_secs(crate::HANDSHAKE_PEER_TIMEOUT_SECS as u64)
    }

    pub(super) async fn run(
        &mut self,
        node: Node,
        mut network: PeerIOHandle,
        mut receiver: mpsc::Receiver<PeerAction>,
    ) -> Result<(), NetworkError> {
        let mut reader = network.take_reader();

        let (sender, mut read_receiver) = mpsc::channel::<Result<Vec<u8>, NetworkError>>(8);
        let queued_inbound_message_count = self.queued_inbound_message_count.clone();

        tokio::spawn(async move {
            loop {
                if sender
                    .send(reader.read_raw_payload().await.map(|x| x.to_vec()))
                    .await
                    .is_err()
                {
                    break;
                } else {
                    queued_inbound_message_count.fetch_add(1, Ordering::SeqCst);
                    metrics::increment_gauge!(snarkos_metrics::queues::INBOUND, 1.0);
                }
            }
        });

        loop {
            select_biased! {
                message = receiver.recv().fuse() => {
                    if message.is_none() {
                        break;
                    }
                    let message = message.unwrap();
                    match self.process_message(&mut network, message).await? {
                        PeerResponse::Disconnect => break,
                        PeerResponse::None => (),
                    }
                },
                data = read_receiver.recv().fuse() => {
                    if data.is_none() {
                        break;
                    }

                    self.queued_inbound_message_count.fetch_sub(1, Ordering::SeqCst);
                    metrics::decrement_gauge!(snarkos_metrics::queues::INBOUND, 1.0);

                    let data = match data.unwrap() {
                        // decrypt
                        Ok(data) => network.read_payload(&data[..]),
                        Err(e) => Err(e)
                    };

                    let deserialized = self.deserialize_payload(data);

                    let time_received = match deserialized {
                        Ok(Payload::GetPeers)
                        | Ok(Payload::GetSync(_))
                        | Ok(Payload::GetBlocks(_))
                        | Ok(Payload::GetMemoryPool) => Some(Instant::now()),
                        _ => None,
                    };

                    self.dispatch_payload(&node, &mut network, time_received, deserialized).await?;
                },
            }
        }

        let queued_inbound_message_count = self.queued_inbound_message_count.swap(0, Ordering::SeqCst);
        metrics::decrement_gauge!(snarkos_metrics::queues::INBOUND, queued_inbound_message_count as f64);
        let queued_outbound_message_count = self.queued_outbound_message_count.swap(0, Ordering::SeqCst);
        metrics::decrement_gauge!(snarkos_metrics::queues::OUTBOUND, queued_outbound_message_count as f64);

        Ok(())
    }

    pub(super) fn set_connected(&mut self) {
        self.quality.connected();
        self.status = PeerStatus::Connected;
    }

    pub(super) fn set_connecting(&mut self) {
        self.quality.see();
        self.status = PeerStatus::Connecting;
    }

    pub(super) fn set_disconnected(&mut self) {
        self.quality.disconnected();
        self.status = PeerStatus::Disconnected;
    }

    pub(super) fn set_routable(&mut self, is_routable: bool) {
        self.is_routable = Some(is_routable)
    }
}
