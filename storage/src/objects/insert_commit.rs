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

use crate::*;
use snarkvm::{
    dpc::{Parameters, RecordCommitmentTree, Transaction, TransactionScheme},
    ledger::{
        Block,
        BlockError,
        BlockHeaderHash,
        BlockScheme,
        DatabaseTransaction,
        LedgerScheme,
        Op,
        Storage,
        StorageError,
    },
    utilities::{has_duplicates, to_bytes_le, ToBytes},
};

use std::sync::atomic::Ordering;

impl<C: Parameters, S: Storage> Ledger<C, S> {
    /// Commit a transaction to the canon chain
    #[allow(clippy::type_complexity)]
    pub(crate) fn commit_transaction(
        &self,
        sn_index: &mut usize,
        cm_index: &mut usize,
        transaction: &Transaction<C>,
    ) -> Result<(Vec<Op>, Vec<(C::RecordCommitment, usize)>), StorageError> {
        let old_serial_numbers = transaction.serial_numbers();
        let new_commitments = transaction.commitments();

        let mut ops = Vec::with_capacity(old_serial_numbers.len() + new_commitments.len());
        let mut cms = Vec::with_capacity(new_commitments.len());

        for sn in old_serial_numbers {
            let sn_bytes = to_bytes_le![sn]?;
            if self.get_sn_index(&sn_bytes)?.is_some() {
                return Err(StorageError::ExistingSn(sn_bytes.to_vec()));
            }

            ops.push(Op::Insert {
                col: COL_SERIAL_NUMBER,
                key: sn_bytes,
                value: (*sn_index as u32).to_le_bytes().to_vec(),
            });
            *sn_index += 1;
        }

        for cm in new_commitments {
            let cm_bytes = to_bytes_le![cm]?;
            if self.get_cm_index(&cm_bytes)?.is_some() {
                return Err(StorageError::ExistingCm(cm_bytes.to_vec()));
            }

            ops.push(Op::Insert {
                col: COL_COMMITMENT,
                key: cm_bytes,
                value: (*cm_index as u32).to_le_bytes().to_vec(),
            });
            cms.push((cm.clone(), *cm_index));

            *cm_index += 1;
        }

        Ok((ops, cms))
    }

    /// Insert a block into storage without canonizing/committing it.
    pub fn insert_only(&self, block: &Block<Transaction<C>>) -> Result<(), StorageError> {
        // If the ledger is initialized, ensure the block header is not a genesis header.
        if self.block_height() != 0 && block.header().is_genesis() {
            return Err(StorageError::InvalidBlockHeader);
        }

        let block_hash = block.header.get_hash();

        // Check that the block does not already exist.
        if self.contains_block_hash(&block_hash) {
            return Err(StorageError::BlockError(BlockError::BlockExists(
                block_hash.to_string(),
            )));
        }

        let mut database_transaction = DatabaseTransaction::new();

        let mut transaction_serial_numbers = Vec::with_capacity(block.transactions.0.len());
        let mut transaction_commitments = Vec::with_capacity(block.transactions.0.len());

        for transaction in &block.transactions.0 {
            transaction_serial_numbers.push(transaction.serial_numbers());
            transaction_commitments.push(transaction.commitments());
        }

        // Sanitize the block inputs

        // Check if the transactions in the block have duplicate serial numbers
        if has_duplicates(transaction_serial_numbers) {
            return Err(StorageError::DuplicateSn);
        }

        // Check if the transactions in the block have duplicate commitments
        if has_duplicates(transaction_commitments) {
            return Err(StorageError::DuplicateCm);
        }

        for (index, transaction) in block.transactions.0.iter().enumerate() {
            let transaction_location = TransactionLocation {
                index: index as u32,
                block_hash: block_hash.0,
            };
            database_transaction.push(Op::Insert {
                col: COL_TRANSACTION_LOCATION,
                key: transaction.transaction_id()?.to_vec(),
                value: to_bytes_le![transaction_location]?,
            });
        }

        database_transaction.push(Op::Insert {
            col: COL_BLOCK_HEADER,
            key: block_hash.0.to_vec(),
            value: to_bytes_le![block.header]?,
        });
        database_transaction.push(Op::Insert {
            col: COL_BLOCK_TRANSACTIONS,
            key: block_hash.0.to_vec(),
            value: to_bytes_le![block.transactions]?,
        });

        let mut child_hashes = self.get_child_block_hashes(&block.header.previous_block_hash)?;

        if !child_hashes.contains(&block_hash) {
            child_hashes.push(block_hash);

            database_transaction.push(Op::Insert {
                col: COL_CHILD_HASHES,
                key: block.header.previous_block_hash.0.to_vec(),
                value: bincode::serialize(&child_hashes)?,
            });
        }

        self.storage.batch(database_transaction)?;

        Ok(())
    }

    /// Commit/canonize a particular block.
    pub fn commit(&self, block: &Block<Transaction<C>>) -> Result<(), StorageError> {
        // If the ledger is initialized, ensure the block header is not a genesis header.
        let block_height = self.block_height();
        let is_genesis = block.header().is_genesis();
        if block_height != 0 && is_genesis {
            return Err(StorageError::InvalidBlockHeader);
        }

        // Check if the block is already in the canon chain
        let block_header_hash = block.header.get_hash();
        if self.is_canon(&block_header_hash) {
            return Err(StorageError::ExistingCanonBlock(block_header_hash.to_string()));
        }

        let mut database_transaction = DatabaseTransaction::new();

        let mut transaction_serial_numbers = Vec::with_capacity(block.transactions.0.len());
        let mut transaction_commitments = Vec::with_capacity(block.transactions.0.len());

        for transaction in &block.transactions.0 {
            transaction_serial_numbers.push(transaction.serial_numbers());
            transaction_commitments.push(transaction.commitments());
        }

        // Sanitize the block inputs

        // Check if the transactions in the block have duplicate serial numbers
        if has_duplicates(transaction_serial_numbers) {
            return Err(StorageError::DuplicateSn);
        }

        // Check if the transactions in the block have duplicate commitments
        if has_duplicates(transaction_commitments) {
            return Err(StorageError::DuplicateCm);
        }

        let mut sn_index = self.current_sn_index()?;
        let mut cm_index = self.current_cm_index()?;

        // Process the individual transactions

        let mut transaction_cms = vec![];

        for (index, transaction) in block.transactions.0.iter().enumerate() {
            let (tx_ops, cms) = self.commit_transaction(&mut sn_index, &mut cm_index, transaction)?;
            database_transaction.push_vec(tx_ops);
            transaction_cms.extend(cms);

            let transaction_location = TransactionLocation {
                index: index as u32,
                block_hash: block_header_hash.0,
            };
            database_transaction.push(Op::Insert {
                col: COL_TRANSACTION_LOCATION,
                key: transaction.transaction_id()?.to_vec(),
                value: to_bytes_le![transaction_location]?,
            });
        }

        // Update the database state for current indexes

        database_transaction.push(Op::Insert {
            col: COL_META,
            key: KEY_CURR_SN_INDEX.as_bytes().to_vec(),
            value: (sn_index as u32).to_le_bytes().to_vec(),
        });
        database_transaction.push(Op::Insert {
            col: COL_META,
            key: KEY_CURR_CM_INDEX.as_bytes().to_vec(),
            value: (cm_index as u32).to_le_bytes().to_vec(),
        });

        // Update the best block number

        let new_block_height = match is_genesis {
            true => block_height,
            false => block_height + 1,
        };

        database_transaction.push(Op::Insert {
            col: COL_META,
            key: KEY_BEST_BLOCK_NUMBER.as_bytes().to_vec(),
            value: new_block_height.to_le_bytes().to_vec(),
        });

        // Update the block location

        database_transaction.push(Op::Insert {
            col: COL_BLOCK_LOCATOR,
            key: block_header_hash.0.to_vec(),
            value: new_block_height.to_le_bytes().to_vec(),
        });
        database_transaction.push(Op::Insert {
            col: COL_BLOCK_LOCATOR,
            key: new_block_height.to_le_bytes().to_vec(),
            value: block_header_hash.0.to_vec(),
        });

        // If it's the genesis block, store its initial applicable digest.
        if is_genesis {
            database_transaction.push(Op::Insert {
                col: COL_DIGEST,
                key: to_bytes_le![self.latest_digest()?]?,
                value: 0u32.to_le_bytes().to_vec(),
            });
        }

        // Rebuild the new commitment merkle tree
        self.add_new_commitments(transaction_cms)?;
        let tree = self.cm_merkle_tree.load();
        let new_digest = tree.root();

        database_transaction.push(Op::Insert {
            col: COL_DIGEST,
            key: to_bytes_le![new_digest]?,
            value: new_block_height.to_le_bytes().to_vec(),
        });
        database_transaction.push(Op::Insert {
            col: COL_META,
            key: KEY_CURR_DIGEST.as_bytes().to_vec(),
            value: to_bytes_le![new_digest]?,
        });

        self.storage.batch(database_transaction)?;

        if !is_genesis {
            self.current_block_height.fetch_add(1, Ordering::SeqCst);
        }

        Ok(())
    }

    /// Insert a block into the storage and commit as part of the longest chain.
    pub fn insert_and_commit(&self, block: &Block<Transaction<C>>) -> Result<(), StorageError> {
        let block_hash = block.header.get_hash();

        // If the block does not exist in the storage
        if !self.contains_block_hash(&block_hash) {
            // Insert it first
            self.insert_only(block)?;
        }
        // Commit it
        self.commit(block)
    }

    /// Returns true if the block exists in the canon chain.
    pub fn is_canon(&self, block_hash: &BlockHeaderHash) -> bool {
        self.contains_block_hash(block_hash) && self.get_block_number(block_hash).is_ok()
    }

    /// Revert the chain to the state before the fork.
    pub fn revert_for_fork(&self, side_chain_path: &SideChainPath) -> Result<(), StorageError> {
        let current_block_height = self.block_height();

        if side_chain_path.new_block_number > current_block_height {
            // Decommit all blocks on canon chain up to the shared block number with the side chain.
            for _ in (side_chain_path.shared_block_number)..current_block_height {
                self.decommit_latest_block()?;
            }
        }

        Ok(())
    }
}
