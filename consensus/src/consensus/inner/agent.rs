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

use crate::consensus::ConsensusMessageWrapped;

use super::*;

impl ConsensusInner {
    /// initialize genesis block if neccesary and catches up the chain
    async fn init(&mut self) -> Result<()> {
        let canon = self.storage.canon().await?;
        // no blocks present/genesis situation
        if canon.is_empty() {
            // no blocks
            let hash = self.public.genesis_block.header.hash();
            let block = self.public.genesis_block.clone();
            self.storage.insert_block(&block).await?;

            self.commit_block(&hash, &block).await?;
        }

        // scan for forks
        // let forks = self.scan_forks().await?;
        // for (canon, fork_child) in forks {
        //     let canon_height = match self.storage.get_block_state(&canon).await? {
        //         BlockStatus::Committed(n) => n,
        //         _ => continue,
        //     };
        //     let fork_blocks = self.storage.longest_child_path(&fork_child).await?;
        //     debug!(
        //         "fork detected @ {}/{} -- starts at {}, goes for {} blocks, ending at {}",
        //         canon_height,
        //         canon,
        //         fork_child,
        //         fork_blocks.len(),
        //         fork_blocks.last().unwrap()
        //     );
        // }

        if let Err(e) = self.try_to_fast_forward().await {
            match e {
                ConsensusError::InvalidBlock(e) => debug!("invalid block in initial fast-forward: {}", e),
                e => warn!("failed to perform initial fast-forward: {:?}", e),
            }
        };
        info!("fastforwarding complete");
        Ok(())
    }

    pub(in crate::consensus) async fn agent(mut self, mut receiver: mpsc::Receiver<ConsensusMessageWrapped>) {
        self.init()
            .await
            .expect("failed to initialize ledger & storage with genesis block");

        while let Some((message, response)) = receiver.recv().await {
            metrics::decrement_gauge!(snarkos_metrics::queues::CONSENSUS, 1.0);

            match message {
                ConsensusMessage::ReceiveTransaction(transaction) => {
                    response.send(Box::new(self.receive_transaction(transaction))).ok();
                }
                ConsensusMessage::VerifyTransactions(transactions) => {
                    let out = match self.verify_transactions(transactions.iter()) {
                        Ok(out) => out,
                        Err(e) => {
                            error!(
                                "failed to validate transactions -- note this does not mean the transactions were valid or invalid: {:?}",
                                e
                            );
                            false
                        }
                    };
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::ReceiveBlock(block) => match self.receive_block(&block).await {
                    Ok(()) => {
                        response.send(Box::new(true)).ok();
                    }
                    Err(e) => {
                        match e {
                            ConsensusError::InvalidBlock(e) => {
                                debug!("failed receiving block: {:?}", e);
                            }
                            e => {
                                warn!("failed receiving block: {:?}", e);
                            }
                        }
                        response.send(Box::new(false)).ok();
                    }
                },
                ConsensusMessage::FetchMemoryPool(size) => {
                    let out: Vec<SerialTransaction> =
                        self.memory_pool.get_candidates(size).into_iter().cloned().collect();
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::CreateTransaction(request) => {
                    let out = self.create_transaction(*request);
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::CreatePartialTransaction(request) => {
                    let out = self.create_partial_transaction(request);
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::ForceDecommit(hash) => {
                    let out = self.decommit_ledger_block(&hash[..].into()).await;
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::FastForward() => {
                    let out = self.try_to_fast_forward().await;
                    response.send(Box::new(out)).ok();
                }
                ConsensusMessage::ScanForks() => {
                    response.send(Box::new(self.scan_forks().await)).ok();
                }
            }
        }
    }
}
