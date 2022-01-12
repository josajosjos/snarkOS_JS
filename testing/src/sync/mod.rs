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

use snarkos_consensus::{ConsensusParameters, MerkleTreeLedger};
use snarkos_storage::LedgerStorage;
use snarkvm::{
    dpc::{testnet1::Testnet1Parameters, Network, Parameters, TransactionError, TransactionScheme},
    ledger::posw::PoswMarlin,
    utilities::{FromBytes, ToBytes},
};

use anyhow::Result;
use once_cell::sync::Lazy;
use std::{
    io::{Read, Result as IoResult, Write},
    sync::Arc,
};

mod data;
pub use data::*;

mod fixture;
pub use fixture::*;

pub static TEST_CONSENSUS_PARAMS: Lazy<ConsensusParameters> = Lazy::new(|| {
    ConsensusParameters {
        max_block_size: 1_000_000usize,
        max_nonce: u32::max_value(),
        target_block_time: 2i64, //unix seconds
        network_id: Network::Testnet1,
        verifier: PoswMarlin::verify_only().unwrap(),
        authorized_inner_circuit_ids: vec![
            <Testnet1Parameters as Parameters>::inner_circuit_id()
                .to_bytes_le()
                .unwrap(),
        ],
    }
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestTestnet1Transaction;

impl TransactionScheme for TestTestnet1Transaction {
    type Commitment = [u8; 32];
    type Digest = [u8; 32];
    type EncryptedRecord = [u8; 32];
    type InnerCircuitID = [u8; 32];
    type Memo = [u8; 64];
    type SerialNumber = [u8; 32];
    type Signature = [u8; 32];
    type ValueBalance = i64;

    fn transaction_id(&self) -> Result<[u8; 32]> {
        Ok([0u8; 32])
    }

    fn network_id(&self) -> u8 {
        0
    }

    fn ledger_digest(&self) -> &Self::Digest {
        &[0u8; 32]
    }

    fn inner_circuit_id(&self) -> &Self::InnerCircuitID {
        &[0u8; 32]
    }

    fn serial_numbers(&self) -> &[Self::SerialNumber] {
        &[[0u8; 32]; 2]
    }

    fn commitments(&self) -> &[Self::Commitment] {
        &[[0u8; 32]; 2]
    }

    fn value_balance(&self) -> i64 {
        0
    }

    fn memo(&self) -> &Self::Memo {
        &[0u8; 64]
    }

    fn signatures(&self) -> &[Self::Signature] {
        &[[0u8; 32]; 2]
    }

    fn encrypted_records(&self) -> &[Self::EncryptedRecord] {
        &[[0u8; 32]; 2]
    }
}

impl ToBytes for TestTestnet1Transaction {
    #[inline]
    fn write_le<W: Write>(&self, mut _writer: W) -> IoResult<()> {
        Ok(())
    }
}

impl FromBytes for TestTestnet1Transaction {
    #[inline]
    fn read_le<R: Read>(mut _reader: R) -> IoResult<Self> {
        Ok(Self)
    }
}

pub fn create_test_consensus() -> snarkos_consensus::Consensus<LedgerStorage> {
    create_test_consensus_from_ledger(Arc::new(FIXTURE_VK.ledger()))
}

pub fn create_test_consensus_from_ledger(
    ledger: Arc<MerkleTreeLedger<LedgerStorage>>,
) -> snarkos_consensus::Consensus<LedgerStorage> {
    snarkos_consensus::Consensus {
        ledger,
        memory_pool: Default::default(),
        parameters: TEST_CONSENSUS_PARAMS.clone(),
        dpc: FIXTURE.dpc.clone(),
    }
}
