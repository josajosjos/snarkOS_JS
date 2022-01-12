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

use crate::{error::ConsensusError, ConsensusParameters, MemoryPool, MerkleTreeLedger};
use snarkos_metrics::{
    self as metrics,
    misc::{BLOCK_HEIGHT, *},
};
use snarkos_storage::BlockPath;
use snarkvm::{
    algorithms::CRH,
    dpc::{
        prelude::*,
        testnet1::{Testnet1DPC, Testnet1Parameters, Testnet1Transaction},
    },
    ledger::{posw::txids_to_roots, Block, LedgerScheme, Storage, StorageError, Transactions},
    utilities::{to_bytes_le, ToBytes},
};

use rand::{CryptoRng, Rng};
use std::sync::Arc;

pub struct Consensus<S: Storage> {
    pub parameters: ConsensusParameters,
    pub dpc: Arc<Testnet1DPC>,
    pub ledger: Arc<MerkleTreeLedger<S>>,
    pub memory_pool: MemoryPool<Testnet1Transaction>,
}

impl<S: Storage> Consensus<S> {
    /// Check if the transaction is valid.
    pub fn verify_transaction(&self, transaction: &Testnet1Transaction) -> Result<bool, ConsensusError> {
        if !self
            .parameters
            .authorized_inner_circuit_ids
            .contains(&to_bytes_le![transaction.inner_circuit_id]?)
        {
            return Ok(false);
        }

        Ok(self.dpc.verify(transaction, &*self.ledger))
    }

    /// Check if the transactions are valid.
    pub fn verify_transactions(&self, transactions: &[Testnet1Transaction]) -> Result<bool, ConsensusError> {
        for tx in transactions {
            if !self
                .parameters
                .authorized_inner_circuit_ids
                .contains(&to_bytes_le![tx.inner_circuit_id]?)
            {
                return Ok(false);
            }
        }

        Ok(self.dpc.verify_transactions(transactions, &*self.ledger))
    }

    /// Check if the block is valid.
    /// Verify transactions and transaction fees.
    pub fn verify_block(&self, block: &Block<Testnet1Transaction>) -> Result<bool, ConsensusError> {
        let transaction_ids: Vec<_> = block.transactions.to_transaction_ids()?;
        let (merkle_root, pedersen_merkle_root, _) = txids_to_roots(&transaction_ids);

        // TODO (howardwu): Change to always `verify_header` (remove the skip genesis conditional).
        //  Instead, inside `verify_header`, check all fields as before, except
        //  now check that the previous block hash is the empty block hash e.g. [0u8; 32].
        // Verify the block header.
        let current_block_height = self.ledger.block_height();
        if current_block_height > 0 && !block.header.is_genesis() {
            let parent_block = self.ledger.latest_block()?;
            if let Err(err) =
                self.parameters
                    .verify_header(&block.header, &parent_block.header, &merkle_root, &pedersen_merkle_root)
            {
                error!("block header failed to verify: {:?}", err);
                return Ok(false);
            }
        }

        // Verify block amounts and check that there is a single coinbase transaction.
        let mut coinbase_transaction_count = 0;
        let mut total_value_balance = AleoAmount::ZERO;

        for transaction in block.transactions.iter() {
            let value_balance = transaction.value_balance;
            if value_balance.is_negative() {
                coinbase_transaction_count += 1;
            }

            total_value_balance = total_value_balance.add(value_balance);
        }

        // Check that there is only 1 coinbase transaction.
        if coinbase_transaction_count > 1 {
            error!("multiple coinbase transactions");
            return Ok(false);
        }

        // Check that the block value balances are correct.
        if current_block_height > 0 && !block.header.is_genesis() {
            let expected_block_reward = crate::get_block_reward(current_block_height + 1).0;
            if total_value_balance.0 + expected_block_reward != 0 {
                trace!("total_value_balance: {:?}", total_value_balance);
                trace!("expected_block_reward: {:?}", expected_block_reward);

                return Ok(false);
            }
        }

        // Check that all the transaction proofs verify
        self.verify_transactions(&block.transactions.0)
    }

    /// Receive a block from an external source and process it based on ledger state.
    pub async fn receive_block(
        &self,
        block: &Block<Testnet1Transaction>,
        batch_import: bool,
    ) -> Result<(), ConsensusError> {
        // Block is a genesis block.
        if block.header.is_genesis() {
            debug!("Received a genesis block");

            let canon_genesis_block_hash = self.ledger.get_block_hash(0)?;
            let canon_genesis_block = self.ledger.get_block(&canon_genesis_block_hash)?;

            match block.header == canon_genesis_block.header {
                true => debug!("Received the canon genesis block. No action needed"),
                false => warn!("Received a mismatching genesis block. Peer is on a different chain"),
            }

            return Ok(());
        }

        // Block is an unknown orphan block.
        if !self.ledger.contains_block_hash(&block.header.previous_block_hash) // Is block's previous block hash known to us?
            // Is block's previous block hash in our canon chain?
            && !self.ledger.is_canon(&block.header.previous_block_hash)
        {
            // TODO (howardwu): Deprecate the genesis case in here.
            // There are two possible cases for an unknown orphan.
            // 1) The block is a genesis block, or
            // 2) The block is unknown and does not correspond with the canon chain.
            if self.ledger.is_empty() {
                debug!("Processing an unknown genesis block");

                self.process_block(block).await?;
            } else {
                debug!("Processing an unknown orphan block");

                metrics::increment_counter!(ORPHAN_BLOCKS);
                self.ledger.insert_only(block)?;
            }

            return Ok(());
        }

        // If the block is not an unknown orphan block, find the origin of the block.
        match self.ledger.get_block_path(&block.header)? {
            BlockPath::ExistingBlock => {
                debug!("Received a pre-existing block");
                return Err(ConsensusError::PreExistingBlock);
            }
            BlockPath::CanonChain(block_height) => {
                debug!("Processing a block on the canon chain. Height {}", block_height);

                self.process_block(block).await?;

                if !batch_import {
                    // Attempt to fast forward the block state if the node already stores
                    // the children of the new canon block.
                    let child_path = self.ledger.longest_child_path(block.header.get_hash())?;

                    if child_path.len() > 1 {
                        debug!(
                            "Attempting to canonize the descendants of block at height {}.",
                            block_height
                        );
                    }

                    for child_block_hash in child_path.into_iter().skip(1) {
                        let new_block = self.ledger.get_block(&child_block_hash)?;

                        debug!(
                            "Processing the next known descendant. Height {}",
                            self.ledger.block_height() + 1
                        );
                        self.process_block(&new_block).await?;
                    }
                }
            }
            BlockPath::SideChain(side_chain_path) => {
                debug!(
                    "Processing a block on a side chain. Height {}",
                    side_chain_path.new_block_number
                );

                // If the side chain is now heavier than the canon chain,
                // perform a fork to the side chain.
                let canon_difficulty = self.get_canon_difficulty_from_height(side_chain_path.shared_block_number)?;

                if side_chain_path.aggregate_difficulty > canon_difficulty {
                    debug!(
                        "Determined side chain is heavier than canon chain by {}%",
                        get_delta_percentage(side_chain_path.aggregate_difficulty, canon_difficulty)
                    );
                    warn!("A valid fork has been detected. Performing a fork to the side chain.");

                    // Fork to superior side chain
                    self.ledger.revert_for_fork(&side_chain_path)?;

                    // Update the current block height metric.
                    metrics::gauge!(BLOCK_HEIGHT, self.ledger.block_height() as f64);

                    if !side_chain_path.path.is_empty() {
                        for block_hash in side_chain_path.path {
                            if block_hash == block.header.get_hash() {
                                self.process_block(block).await?
                            } else {
                                let new_block = self.ledger.get_block(&block_hash)?;
                                self.process_block(&new_block).await?;
                            }
                        }
                    }
                } else {
                    metrics::increment_counter!(ORPHAN_BLOCKS);

                    // If the sidechain is not longer than the main canon chain, simply store the block
                    self.ledger.insert_only(block)?;
                }
            }
        };

        Ok(())
    }

    /// Return whether or not the given block is valid and insert it.
    /// 1. Verify that the block header is valid.
    /// 2. Verify that the transactions are valid.
    /// 3. Insert/canonize block.
    pub async fn process_block(&self, block: &Block<Testnet1Transaction>) -> Result<(), ConsensusError> {
        if self.ledger.is_canon(&block.header.get_hash()) {
            return Ok(());
        }

        // 1. Verify that the block valid
        if !self.verify_block(block)? {
            return Err(ConsensusError::InvalidBlock(block.header.get_hash().0.to_vec()));
        }

        // 2. Insert/canonize block
        self.ledger.insert_and_commit(block)?;

        // Increment the current block height metric
        metrics::increment_gauge!(BLOCK_HEIGHT, 1.0);

        // 3. Remove transactions from the mempool
        for transaction_id in block.transactions.to_transaction_ids()? {
            self.memory_pool.remove_by_hash(&transaction_id).await?;
        }

        Ok(())
    }

    /// Generate a transaction by spending old records and specifying new record attributes
    #[allow(clippy::too_many_arguments)]
    pub fn create_transaction<R: Rng + CryptoRng>(
        &self,
        old_records: Vec<Record<Testnet1Parameters>>,
        private_keys: Vec<PrivateKey<Testnet1Parameters>>,
        new_records: Vec<Record<Testnet1Parameters>>,
        memo: Option<[u8; 64]>,
        rng: &mut R,
    ) -> Result<Testnet1Transaction, ConsensusError> {
        // TODO (raychu86): Genericize this model to allow for generic programs.
        let noop = Arc::new(self.dpc.noop_program.clone());

        let mut builder = StateTransition::builder();

        for (private_key, old_record) in private_keys.iter().zip(old_records.iter()) {
            builder = builder.add_input(Input::new(
                private_key.compute_key(),
                old_record.clone(),
                None,
                noop.clone(),
            )?);
        }

        for new_record in new_records.iter() {
            builder = builder.add_output(Output::new(
                new_record.owner(),
                AleoAmount::from_bytes(new_record.value() as i64),
                new_record.payload().clone(),
                None,
                noop.clone(),
            )?);
        }

        match memo {
            Some(memo) => builder = builder.append_memo(&memo.to_vec()),
            None => (),
        };

        let state = builder.build(noop.clone(), rng)?;

        // Offline execution to generate a transaction authorization.
        let authorization = self.dpc.authorize::<R>(&private_keys, &state, rng)?;

        // Online execution to generate a transaction.
        let compute_keys = private_keys
            .iter()
            .take(Testnet1Parameters::NUM_INPUT_RECORDS)
            .map(|private_key| private_key.compute_key().clone())
            .collect();

        let transaction = self
            .dpc
            .execute(&compute_keys, authorization, state.executables(), &*self.ledger, rng)?;

        Ok(transaction)
    }

    /// Generate a coinbase transaction given candidate block transactions
    pub fn create_coinbase_transaction<R: Rng + CryptoRng>(
        &self,
        block_num: u32,
        transactions: &Transactions<Testnet1Transaction>,
        recipient: Address<Testnet1Parameters>,
        rng: &mut R,
    ) -> Result<(Vec<Record<Testnet1Parameters>>, Testnet1Transaction), ConsensusError> {
        // Calculate the total value balance of the block.
        let mut total_value_balance = crate::get_block_reward(block_num);
        for transaction in transactions.iter() {
            if transaction.value_balance.is_negative() {
                return Err(ConsensusError::CoinbaseTransactionAlreadyExists());
            }

            total_value_balance = total_value_balance.add(transaction.value_balance);
        }

        let noop = Arc::new(self.dpc.noop_program.clone());
        let state = StateTransition::builder()
            .add_output(Output::new(
                recipient,
                total_value_balance,
                Payload::default(),
                None,
                noop.clone(),
            )?)
            .add_output(Output::new(
                recipient,
                AleoAmount::from_bytes(0),
                Payload::default(),
                None,
                noop.clone(),
            )?)
            .build(noop, rng)?;
        let authorization = self.dpc.authorize::<R>(&vec![], &state, rng)?;
        let transaction = self
            .dpc
            .execute(&vec![], authorization, state.executables(), &*self.ledger, rng)?;
        let new_records = state.output_records().clone();

        Ok((new_records, transaction))
    }

    fn get_canon_difficulty_from_height(&self, height: u32) -> Result<u128, StorageError> {
        let current_block_height = self.ledger.block_height();
        let path_size = current_block_height - height;
        let mut aggregate_difficulty = 0u128;

        for i in 0..path_size {
            let block_header = self
                .ledger
                .get_block_header(&self.ledger.get_block_hash(current_block_height - i)?)?;

            aggregate_difficulty += block_header.difficulty_target as u128;
        }

        Ok(aggregate_difficulty)
    }
}

fn get_delta_percentage(side_chain_diff: u128, canon_diff: u128) -> f64 {
    let delta = side_chain_diff - canon_diff;
    (delta as f64 / canon_diff as f64) * 100.0
}
