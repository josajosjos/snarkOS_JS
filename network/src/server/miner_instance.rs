use crate::{context::Context, server::propagate_block};
use snarkos_consensus::{
    miner::{MemoryPool, Miner},
    posw::ProvingKey,
    ConsensusParameters,
};
use snarkos_dpc::{
    address::AddressPublicKey,
    base_dpc::{instantiated::*, parameters::PublicParameters},
};
use snarkos_objects::block::Block;

use std::sync::Arc;
use tokio::{sync::Mutex, task};

/// Parameters for spawning a miner that runs proof of work to find a block.
pub struct MinerInstance {
    coinbase_address: AddressPublicKey<Components>,
    consensus: ConsensusParameters,
    parameters: PublicParameters<Components>,
    storage: Arc<MerkleTreeLedger>,
    memory_pool_lock: Arc<Mutex<MemoryPool<Tx>>>,
    server_context: Arc<Context>,
    proving_key: ProvingKey,
}

impl MinerInstance {
    /// Creates a new MinerInstance for spawning miners.
    pub fn new(
        coinbase_address: AddressPublicKey<Components>,
        consensus: ConsensusParameters,
        parameters: PublicParameters<Components>,
        storage: Arc<MerkleTreeLedger>,
        memory_pool_lock: Arc<Mutex<MemoryPool<Tx>>>,
        server_context: Arc<Context>,
        proving_key: ProvingKey,
    ) -> Self {
        Self {
            coinbase_address,
            consensus,
            parameters,
            storage,
            memory_pool_lock,
            server_context,
            proving_key,
        }
    }

    /// Spawns a new miner on a new thread using MinerInstance parameters.
    /// Once a block is found, A block message is sent to all peers.
    /// Calling this function multiple times will spawn additional listeners on separate threads.
    /// Miner threads are asynchronous so the only way to stop them is to kill the runtime they were started in. This may be changed in the future.
    pub fn spawn(self) {
        task::spawn(async move {
            let context = self.server_context.clone();
            let local_address = self.server_context.local_address;
            let miner = Miner::new(
                self.coinbase_address.clone(),
                self.consensus.clone(),
                self.proving_key.clone(),
            );

            loop {
                info!("Mining new block");

                let (block_serialized, _coinbase_records) = miner
                    .mine_block(&self.parameters, &self.storage, &self.memory_pool_lock)
                    .await
                    .unwrap();

                info!(
                    "Block found!           {:?}",
                    Block::<Tx>::deserialize(&block_serialized).unwrap()
                );

                propagate_block(context.clone(), block_serialized, local_address)
                    .await
                    .unwrap();
            }
        });
    }
}
