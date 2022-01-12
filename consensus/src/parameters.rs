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

use crate::{difficulty::bitcoin_retarget, error::ConsensusError, DeserializedLedger};
use snarkos_profiler::{end_timer, start_timer};
use snarkos_storage::SerialBlockHeader;
use snarkvm_algorithms::SNARK;
use snarkvm_curves::bls12_377::Bls12_377;
use snarkvm_dpc::{
    testnet1::instantiated::{Components, Testnet1DPC},
    DPCComponents,
    DPCScheme,
    MerkleRootHash,
    Network,
    PedersenMerkleRootHash,
    ProgramScheme,
};
use snarkvm_posw::{Marlin, PoswMarlin};
use snarkvm_utilities::FromBytes;

use chrono::Utc;
use rand::{CryptoRng, Rng};

pub const TWO_HOURS_UNIX: i64 = 7200;

/// A data structure containing the sync parameters for a specified network on this node.
#[derive(Clone, Debug)]
pub struct ConsensusParameters {
    /// The network ID that these parameters correspond to.
    pub network_id: Network,
    /// The maximum permitted block size (in bytes).
    pub max_block_size: usize,
    /// The maximum permitted nonce value.
    pub max_nonce: u32,
    /// The anticipated number of seconds for finding a new block.
    pub target_block_time: i64,
    /// The PoSW sync verifier (read-only mode, no proving key loaded).
    pub verifier: PoswMarlin,
    /// The authorized inner SNARK IDs.
    pub authorized_inner_snark_ids: Vec<Vec<u8>>,
}

impl ConsensusParameters {
    /// Calculate the difficulty for the next block based off how long it took to mine the last one.
    pub fn get_block_difficulty(&self, prev_header: &SerialBlockHeader, block_timestamp: i64) -> u64 {
        bitcoin_retarget(
            block_timestamp,
            prev_header.time,
            self.target_block_time,
            prev_header.difficulty_target,
        )
    }

    /// Verify all fields in a block header.
    /// 1. The parent hash points to the tip of the chain.
    /// 2. Transactions hash to merkle root.
    /// 3. The timestamp is less than 2 hours into the future.
    /// 4. The timestamp is greater than parent timestamp.
    /// 5. The header is greater than or equal to target difficulty.
    /// 6. The nonce is within the limit.
    pub fn verify_header(
        &self,
        header: &SerialBlockHeader,
        parent_header: &SerialBlockHeader,
        merkle_root_hash: &MerkleRootHash,
        pedersen_merkle_root_hash: &PedersenMerkleRootHash,
    ) -> Result<(), ConsensusError> {
        let hash_result = header.to_difficulty_hash();

        let now = Utc::now().timestamp();
        let future_timelimit: i64 = now + TWO_HOURS_UNIX;
        let expected_difficulty = self.get_block_difficulty(parent_header, header.time);
        let parent_hash = parent_header.hash();

        if parent_hash != header.previous_block_hash {
            return Err(ConsensusError::NoParent(
                parent_hash.to_string(),
                header.previous_block_hash.to_string(),
            ));
        } else if header.merkle_root_hash != *merkle_root_hash {
            return Err(ConsensusError::MerkleRoot(header.merkle_root_hash.to_string()));
        } else if header.pedersen_merkle_root_hash != *pedersen_merkle_root_hash {
            return Err(ConsensusError::PedersenMerkleRoot(
                header.pedersen_merkle_root_hash.to_string(),
            ));
        } else if header.time > future_timelimit {
            return Err(ConsensusError::FuturisticTimestamp(future_timelimit, header.time));
        } else if header.time < parent_header.time {
            return Err(ConsensusError::TimestampInvalid(header.time, parent_header.time));
        } else if hash_result > header.difficulty_target {
            return Err(ConsensusError::PowInvalid(header.difficulty_target, hash_result));
        } else if header.nonce >= self.max_nonce {
            return Err(ConsensusError::NonceInvalid(header.nonce, self.max_nonce));
        } else if header.difficulty_target != expected_difficulty {
            return Err(ConsensusError::DifficultyMismatch(
                expected_difficulty,
                header.difficulty_target,
            ));
        }

        // Verify the proof
        let proof = <Marlin<Bls12_377> as SNARK>::Proof::read_le(&header.proof.0[..])?;
        let verification_timer = start_timer!(|| "POSW verify");
        self.verifier
            .verify(header.nonce, &proof, &header.pedersen_merkle_root_hash)?;
        end_timer!(verification_timer);

        Ok(())
    }

    // TODO (raychu86): Genericize this model to allow for generic programs.
    /// Generate the birth and death program proofs for a transaction for a given transaction kernel
    #[allow(clippy::type_complexity)]
    pub fn generate_program_proofs<'a, R: Rng + CryptoRng>(
        dpc: &Testnet1DPC,
        transaction_kernel: &<Testnet1DPC as DPCScheme<DeserializedLedger<'a, Components>>>::TransactionKernel,
        rng: &mut R,
    ) -> Result<Vec<<Testnet1DPC as DPCScheme<DeserializedLedger<'a, Components>>>::Execution>, ConsensusError> {
        let local_data = transaction_kernel.into_local_data();

        let mut program_proofs = Vec::with_capacity(Components::NUM_TOTAL_RECORDS);
        for position in 0..Components::NUM_TOTAL_RECORDS {
            program_proofs.push(dpc.noop_program.execute(&local_data, position as u8, rng)?);
        }

        Ok(program_proofs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_block_reward;
    use rand::{thread_rng, Rng};
    use snarkos_testing::sync::DATA;
    use snarkvm_dpc::PedersenMerkleRootHash;

    #[test]
    fn test_block_rewards() {
        let rng = &mut thread_rng();

        let first_halfing: u32 = 4 * 365 * 24 * 100;
        let second_halfing: u32 = first_halfing * 2;

        let mut block_reward: i64 = 150 * 1_000_000;

        // Before block halving
        assert_eq!(get_block_reward(0).0, block_reward);

        for _ in 0..100 {
            let block_num: u32 = rng.gen_range(0..first_halfing);
            assert_eq!(get_block_reward(block_num).0, block_reward);
        }

        // First block halving

        block_reward /= 2;

        assert_eq!(get_block_reward(first_halfing).0, block_reward);

        for _ in 0..100 {
            let block_num: u32 = rng.gen_range((first_halfing + 1)..second_halfing);
            assert_eq!(get_block_reward(block_num).0, block_reward);
        }

        // Second and final block halving

        block_reward /= 2;

        assert_eq!(get_block_reward(second_halfing).0, block_reward);
        assert_eq!(get_block_reward(u32::MAX).0, block_reward);

        for _ in 0..100 {
            let block_num: u32 = rng.gen_range(second_halfing..u32::MAX);
            assert_eq!(get_block_reward(block_num).0, block_reward);
        }
    }

    #[test]
    fn verify_header() {
        // mine a PoSW proof
        let posw = PoswMarlin::load().unwrap();

        let consensus: ConsensusParameters = ConsensusParameters {
            max_block_size: 1_000_000usize,
            max_nonce: std::u32::MAX - 1,
            target_block_time: 2i64, //unix seconds
            network_id: Network::Mainnet,
            verifier: posw,
            authorized_inner_snark_ids: vec![],
        };

        let b1 = DATA.block_1.clone();
        let h1 = b1.header;

        let b2 = DATA.block_2.clone();
        let h2 = b2.header;
        let merkle_root_hash = h2.merkle_root_hash.clone();
        let pedersen_merkle_root = h2.pedersen_merkle_root_hash.clone();

        // OK
        consensus
            .verify_header(&h2, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap();

        // invalid parent hash
        let mut h2_err = h2.clone();
        h2_err.previous_block_hash = [9u8; 32].into();
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // invalid merkle root hash
        let mut h2_err = h2.clone();
        h2_err.merkle_root_hash = MerkleRootHash([3; 32]);
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // past block
        let mut h2_err = h2.clone();
        h2_err.time = 100;
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // far in the future block
        let mut h2_err = h2.clone();
        h2_err.time = Utc::now().timestamp() + 7201;
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // invalid difficulty
        let mut h2_err = h2.clone();
        h2_err.difficulty_target = 100; // set the difficulty very very high
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // invalid nonce
        let mut h2_err = h2.clone();
        h2_err.nonce = std::u32::MAX; // over the max nonce
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // invalid pedersen merkle root hash
        let mut h2_err = h2.clone();
        h2_err.pedersen_merkle_root_hash = PedersenMerkleRootHash([9; 32]);
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();

        // expected difficulty did not match the difficulty target
        let mut h2_err = h2;
        h2_err.difficulty_target = consensus.get_block_difficulty(&h1, Utc::now().timestamp()) + 1;
        consensus
            .verify_header(&h2_err, &h1, &merkle_root_hash, &pedersen_merkle_root)
            .unwrap_err();
    }
}
