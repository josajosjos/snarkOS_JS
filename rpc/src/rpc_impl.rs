use crate::{rpc_types::*, RpcFunctions};
use snarkos_consensus::{block_reward, miner::MemoryPool, ConsensusParameters};
use snarkos_errors::rpc::RpcError;
use snarkos_network::{context::Context, server::process_transaction_internal};
use snarkos_objects::{transaction::*, BlockHeaderHash};
use snarkos_storage::BlockStorage;

use chrono::Utc;
use std::{str::FromStr, sync::Arc};
use tokio::{runtime::Runtime, sync::Mutex};
use wagyu_bitcoin::{BitcoinAddress, BitcoinPrivateKey, Mainnet};

/// Implements JSON-RPC HTTP endpoint functions for a node.
/// The constructor is given Arc::clone() copies of all needed node components.
pub struct RpcImpl {
    /// Blockchain database storage.
    storage: Arc<BlockStorage>,

    /// Network context held by the server.
    server_context: Arc<Context>,

    /// Consensus parameters generated from node config.
    consensus: ConsensusParameters,

    /// Handle to access the memory pool of transactions.
    memory_pool_lock: Arc<Mutex<MemoryPool>>,
}

impl RpcImpl {
    pub fn new(
        storage: Arc<BlockStorage>,
        server_context: Arc<Context>,
        consensus: ConsensusParameters,
        memory_pool_lock: Arc<Mutex<MemoryPool>>,
    ) -> Self {
        Self {
            storage,
            server_context,
            consensus,
            memory_pool_lock,
        }
    }
}

impl RpcFunctions for RpcImpl {
    /// Dummy function for smoke testing.
    fn add(&self, a: u64, b: u64) -> Result<u64, RpcError> {
        // TODO: REMOVE IN PRODUCTION
        Ok(a + b)
    }

    /// Returns the total amount of currency in the spendable outpoints held by the address.
    fn get_balance(&self, address_string: String) -> Result<u64, RpcError> {
        let address = BitcoinAddress::<Mainnet>::from_str(&address_string)?;

        Ok(self.storage.get_balance(&address))
    }

    /// Returns all stored information on a block hash.
    fn get_block(&self, block_hash_string: String) -> Result<BlockInfo, RpcError> {
        let block_hash = hex::decode(&block_hash_string)?;
        assert_eq!(block_hash.len(), 32);

        let block_header_hash = BlockHeaderHash::new(block_hash);
        let height = self.storage.get_block_num(&block_header_hash)?;

        if let Ok(block) = self.storage.get_block(&block_header_hash) {
            let mut transactions = vec![];

            for transaction in block.transactions.iter() {
                transactions.push(hex::encode(&transaction.to_transaction_id()?));
            }

            let next_block_hash = match self.storage.get_block_hash(height + 1) {
                Ok(next_block_hash) => hex::encode(next_block_hash.0),
                Err(_) => "This is the latest block".to_string(),
            };

            Ok(BlockInfo {
                hash: block_hash_string,
                height,
                confirmations: self.storage.get_latest_block_height() - height,
                size: block.serialize()?.len(),
                nonce: block.header.nonce,
                merkle_root: hex::encode(block.header.merkle_root_hash.0),
                transactions,
                previous_block_hash: hex::encode(block.header.previous_block_hash.0),
                next_block_hash,
            })
        } else {
            Err(RpcError::InvalidBlockHash(block_hash_string))
        }
    }

    /// Returns latest block height + 1 to account for genesis block 0.
    fn get_block_count(&self) -> Result<u32, RpcError> {
        Ok(self.storage.get_block_count())
    }

    /// Returns the block hash of the head of the canonical chain.
    fn get_best_block_hash(&self) -> Result<String, RpcError> {
        let best_block_hash = self.storage.get_block_hash(self.storage.get_latest_block_height())?;

        Ok(hex::encode(&best_block_hash.0))
    }

    /// Returns the block hash of the index specified if it exists in the canonical chain.
    fn get_block_hash(&self, block_height: u32) -> Result<String, RpcError> {
        let block_hash = self.storage.get_block_hash(block_height)?;

        Ok(hex::encode(&block_hash.0))
    }

    /// Returns unspent transaction outpoints owned by and address.
    fn list_unspent(&self, address_string: String) -> Result<Vec<(String, u32)>, RpcError> {
        let address = BitcoinAddress::<Mainnet>::from_str(&address_string)?;

        let mut result = vec![];
        for (outpoint, _balance) in self.storage.get_spendable_outpoints(&address) {
            let utxo = (hex::encode(outpoint.transaction_id), outpoint.index);

            result.push(utxo);
        }

        Ok(result)
    }

    /// Returns hex encoded bytes of a transaction from its transaction id.
    fn get_raw_transaction(&self, transaction_id: String) -> Result<String, RpcError> {
        Ok(hex::encode(
            &self
                .storage
                .get_transaction_bytes(&hex::decode(transaction_id)?)?
                .serialize()?,
        ))
    }

    /// Returns hex encoded bytes of a transaction from transaction inputs and outputs.
    fn create_raw_transaction(
        &self,
        inputs: Vec<RPCTransactionOutpoint>,
        outputs: RPCTransactionOutputs,
    ) -> Result<String, RpcError> {
        let mut transaction_inputs = vec![];
        for input in inputs {
            let transaction_input = TransactionInput::new(hex::decode(&input.txid)?, input.vout, None)?;
            transaction_inputs.push(transaction_input);
        }

        let mut transaction_outputs = vec![];
        for (address, amount) in outputs.0 {
            let address = BitcoinAddress::<Mainnet>::from_str(&address)?;
            transaction_outputs.push(TransactionOutput::new(&address, amount)?);
        }

        let transaction_parameters = TransactionParameters {
            version: 1,
            inputs: transaction_inputs,
            outputs: transaction_outputs,
        };

        let transaction = Transaction::new(&transaction_parameters)?;
        let raw_transaction_bytes = hex::encode(&transaction.serialize()?);

        Ok(raw_transaction_bytes)
    }

    /// Returns information about a transaction from serialized bytes.
    fn decode_raw_transaction(&self, transaction_bytes: String) -> Result<TransactionInfo, RpcError> {
        let transaction = Transaction::from_str(&transaction_bytes)?;

        let mut inputs = vec![];

        for input in &transaction.parameters.inputs {
            inputs.push(RPCTransactionInput {
                txid: hex::encode(&input.outpoint.transaction_id),
                vout: input.outpoint.index,
                script_sig: hex::encode(&input.script_sig),
            })
        }

        let mut outputs = vec![];
        for output in &transaction.parameters.outputs {
            outputs.push(RPCTransactionOutput {
                amount: output.amount,
                script_pub_key: hex::encode(&output.script_pub_key),
            })
        }

        Ok(TransactionInfo {
            txid: hex::encode(&transaction.to_transaction_id()?),
            size: hex::decode(transaction_bytes)?.len(),
            version: transaction.parameters.version,
            inputs,
            outputs,
        })
    }

    /// Returns hex encoded bytes of a transaction that is signed given private keys.
    fn sign_raw_transaction(
        &self,
        unsigned_transaction_bytes: String,
        private_keys: Vec<String>,
    ) -> Result<String, RpcError> {
        let mut transaction = Transaction::from_str(&unsigned_transaction_bytes)?;

        for input in transaction.parameters.inputs.clone() {
            if !input.outpoint.is_coinbase() && input.outpoint.script_pub_key.is_none() {
                transaction = transaction.update_outpoint(
                    self.storage
                        .get_outpoint(&input.outpoint.transaction_id, input.outpoint.index as usize)?,
                );
            }
        }

        for private_key_string in private_keys {
            let private_key = BitcoinPrivateKey::<Mainnet>::from_str(&private_key_string)?;
            transaction = transaction.sign(&private_key)?;
        }

        Ok(hex::encode(transaction.serialize()?))
    }

    /// Send raw transaction bytes to this node to be added into the mempool.
    /// If valid, the transaction will be stored and propagated to all peers.
    /// Returns the transaction id if valid.
    fn send_raw_transaction(&self, transaction_bytes: String) -> Result<String, RpcError> {
        let transaction = Transaction::from_str(&transaction_bytes)?;

        match self.storage.check_for_double_spend(&transaction) {
            Ok(_) => {
                Runtime::new()?.block_on(process_transaction_internal(
                    self.server_context.clone(),
                    self.storage.clone(),
                    self.memory_pool_lock.clone(),
                    transaction.serialize()?,
                    self.server_context.local_address,
                ))?;

                Ok(hex::encode(transaction.to_transaction_id()?))
            }
            Err(_) => Ok("Transaction contains spent outputs".into()),
        }
    }

    /// Fetch the number of connected peers this node has.
    fn get_connection_count(&self) -> Result<usize, RpcError> {
        // Create a temporary tokio runtime to make an asynchronous function call
        let peer_book = Runtime::new()?.block_on(self.server_context.peer_book.read());

        Ok(peer_book.connected_total() as usize)
    }

    /// Returns this nodes connected peers.
    fn get_peer_info(&self) -> Result<PeerInfo, RpcError> {
        // Create a temporary tokio runtime to make an asynchronous function call
        let peer_book = Runtime::new()?.block_on(self.server_context.peer_book.read());

        let mut peers = vec![];

        for (peer, _last_seen) in &peer_book.get_connected() {
            peers.push(peer.clone());
        }

        Ok(PeerInfo { peers })
    }

    /// Returns the current mempool and consensus information known by this node.
    fn get_block_template(&self) -> Result<BlockTemplate, RpcError> {
        let block_height = self.storage.get_latest_block_height();
        let block = self.storage.get_block_from_block_num(block_height)?;

        let time = Utc::now().timestamp();

        let memory_pool = Runtime::new()?.block_on(self.memory_pool_lock.lock());
        let full_transactions = memory_pool.get_candidates(&self.storage, self.consensus.max_block_size)?;

        self.storage.check_for_double_spends(&full_transactions)?;

        let mut transaction_strings = vec![];

        let coinbase_value =
            block_reward(block_height + 1) + self.storage.calculate_transaction_fees(&full_transactions)?;

        for transaction in full_transactions.iter() {
            transaction_strings.push(hex::encode(transaction.serialize()?));
        }

        Ok(BlockTemplate {
            previous_block_hash: hex::encode(&block.header.previous_block_hash.0),
            block_height,
            time,
            difficulty_target: self.consensus.get_block_difficulty(&block.header, time),
            transactions: transaction_strings,
            coinbase_value,
        })
    }

    //    fn stop_node(&self) -> Result<bool, RpcError> {
    //        unimplemented!()
    //    }
}

impl RpcImpl {}

#[cfg(test)]
mod tests {
    use super::*;
    use snarkos_consensus::test_data::*;
    use snarkos_network::test_data::*;
    use snarkos_storage::test_data::*;

    use jsonrpc_test as json_test;
    use jsonrpc_test::Rpc;
    use serde_json::Value;
    use std::{collections::HashMap, net::SocketAddr};

    pub const GENESIS_BLOCK_JSON: &'static str = "{\n  \"confirmations\": 0,\n  \"hash\": \"3a8a5db71a2e00007b47cac0c43e5b96ca6f0107dd98ab568ac51b829856a46a\",\n  \"height\": 0,\n  \"merkle_root\": \"b3d9ad9de8e21b2b3a9ffb40bae6fefa852026e7fb2e279322cd7589a20ee355\",\n  \"next_block_hash\": \"This is the latest block\",\n  \"nonce\": 121136,\n  \"previous_block_hash\": \"0000000000000000000000000000000000000000000000000000000000000000\",\n  \"size\": 166,\n  \"transactions\": [\n    \"b3d9ad9de8e21b2b3a9ffb40bae6fefa852026e7fb2e279322cd7589a20ee355\"\n  ]\n}";
    pub const GENESIS_UNSPENT: &'static str =
        "[\n  [\n    \"b3d9ad9de8e21b2b3a9ffb40bae6fefa852026e7fb2e279322cd7589a20ee355\",\n    0\n  ]\n]";

    pub const TEST_TRANSACTION_UNSIGNED: &str = "0100000001758103bb958ba3222e96641e1b39d21e640d325146c2c7aa869a926f8369c5c400000000000110270000000000001976a914ba4fecdfa1d8a56dbf248f1337cefdf06cfc1f6a88ac";
    pub const TEST_TRANSACTION_SIGNED: &str = "0100000001758103bb958ba3222e96641e1b39d21e640d325146c2c7aa869a926f8369c5c4000000006a473045022100d26dc37d53907d3e28a941e7c192f9d7fdc07644bab79676106d150b9e059301022036bbd1044a566f86b189e8a5f6c428832f67503d9199bc21843f1672cae5daab2103ca64499d857698431e999035fd22d97896b1dff672739ad9acb8643cdd2be9510110270000000000001976a914ba4fecdfa1d8a56dbf248f1337cefdf06cfc1f6a88ac";
    pub const TEST_TRANSACTION_JSON: &str = "{\n  \"inputs\": [\n    {\n      \"script_sig\": \"473045022100d26dc37d53907d3e28a941e7c192f9d7fdc07644bab79676106d150b9e059301022036bbd1044a566f86b189e8a5f6c428832f67503d9199bc21843f1672cae5daab2103ca64499d857698431e999035fd22d97896b1dff672739ad9acb8643cdd2be951\",\n      \"txid\": \"758103bb958ba3222e96641e1b39d21e640d325146c2c7aa869a926f8369c5c4\",\n      \"vout\": 0\n    }\n  ],\n  \"outputs\": [\n    {\n      \"amount\": 10000,\n      \"script_pub_key\": \"76a914ba4fecdfa1d8a56dbf248f1337cefdf06cfc1f6a88ac\"\n    }\n  ],\n  \"size\": 183,\n  \"txid\": \"c8cdbf72a885b8382f4789a9005546468bc91263c4dc8d92f3724e11f64487a6\",\n  \"version\": 1\n}";
    pub const TEST_TRANSACTION_TXID: &str = "758103bb958ba3222e96641e1b39d21e640d325146c2c7aa869a926f8369c5c4";
    pub const TEST_TRANSACTION_PRIVATE_KEY: &str = "1Hz8RzEXYPF6z8o7z5SHVnjzmhqS5At5kU";
    pub const TEST_TRANSACTION_SPENDABLE: u64 = 10000u64;

    fn initialize_test_rpc(storage: Arc<BlockStorage>) -> Rpc {
        let bootnode_address = random_socket_address();
        let server_address = random_socket_address();

        let server = initialize_test_server(
            server_address,
            bootnode_address,
            storage.clone(),
            CONNECTION_FREQUENCY_LONG,
        );

        let consensus = TEST_CONSENSUS;

        json_test::Rpc::new(
            RpcImpl::new(storage, server.context.clone(), consensus, server.memory_pool_lock).to_delegate(),
        )
    }

    fn make_request_no_params(rpc: Rpc, method: String) -> Value {
        let request = format!("{{ \"jsonrpc\":\"2.0\", \"id\": 1, \"method\": \"{}\" }}", method,);

        let response = rpc.io.handle_request_sync(&request).unwrap();

        let extracted: Value = serde_json::from_str(&response).unwrap();

        extracted["result"].clone()
    }

    #[test]
    fn test_add() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(rpc.request("add", &[1, 2]), r#"3"#);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_get_balance() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let genesis_miner = TEST_WALLETS[0].address;

        assert_eq!(rpc.request("getbalance", &[genesis_miner]), format![
            r#"{}"#,
            GENESIS_BLOCK_GENESIS_MINER_BALANCE
        ]);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_get_block_call() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(
            rpc.request("getblock", &[GENESIS_BLOCK_HEADER_HASH]),
            GENESIS_BLOCK_JSON
        );

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_get_block_count() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let method = "getblockcount".to_string();

        let result = make_request_no_params(rpc, method);

        assert_eq!(result.as_u64().unwrap(), 1u64);

        kill_storage_async(path);
    }

    #[test]
    fn test_get_best_block_hash() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let method = "getbestblockhash".to_string();

        let result = make_request_no_params(rpc, method);

        assert_eq!(result.as_str().unwrap(), GENESIS_BLOCK_HEADER_HASH.to_string());

        kill_storage_async(path);
    }

    #[test]
    fn test_get_block_hash() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(rpc.request("getblockhash", &[0u32]), format![
            r#""{}""#,
            GENESIS_BLOCK_HEADER_HASH
        ]);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_get_raw_transaction() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(rpc.request("getrawtransaction", &[GENESIS_TRANSACTION_ID]), format![
            r#""{}""#,
            GENESIS_TRANSACTION
        ]);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_list_unspent() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let genesis_miner = TEST_WALLETS[0].address;

        assert_eq!(rpc.request("listunspent", &[genesis_miner]), GENESIS_UNSPENT);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_create_raw_transaction() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let inputs = RPCTransactionOutpoint {
            txid: TEST_TRANSACTION_TXID.into(),
            vout: 0,
        };

        let mut map = HashMap::new();
        map.insert(TEST_TRANSACTION_PRIVATE_KEY.to_string(), TEST_TRANSACTION_SPENDABLE);

        let outputs = RPCTransactionOutputs(map);

        assert_eq!(rpc.request("createrawtransaction", &(vec![inputs], outputs)), format![
            r#""{}""#,
            TEST_TRANSACTION_UNSIGNED
        ]);

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_decode_raw_transaction() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(
            rpc.request("decoderawtransaction", &[TEST_TRANSACTION_SIGNED]),
            TEST_TRANSACTION_JSON
        );

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_sign_raw_transaction() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let genesis_miner_private_key = TEST_WALLETS[0].private_key;

        assert_eq!(
            rpc.request(
                "signrawtransaction",
                &(BLOCK_1_TRANSACTION, vec![genesis_miner_private_key])
            ),
            format![r#""{}""#, BLOCK_1_TRANSACTION]
        );

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_send_raw_transaction() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        assert_eq!(
            rpc.request("sendrawtransaction", &[BLOCK_1_TRANSACTION]),
            r#""Transaction contains spent outputs""#
        );

        drop(rpc);
        kill_storage_async(path);
    }

    #[test]
    fn test_get_connection_count() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let method = "getconnectioncount".to_string();

        let result = make_request_no_params(rpc, method);

        assert_eq!(result.as_u64().unwrap(), 0u64);

        kill_storage_async(path);
    }

    #[test]
    fn test_get_peer_info() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let method = "getpeerinfo".to_string();

        let result = make_request_no_params(rpc, method);

        let peer_info: PeerInfo = serde_json::from_value(result).unwrap();

        let expected_peers: Vec<SocketAddr> = vec![];

        assert_eq!(peer_info.peers, expected_peers);

        kill_storage_async(path);
    }

    #[test]
    fn test_get_block_template() {
        let (storage, path) = initialize_test_blockchain();
        let rpc = initialize_test_rpc(storage);

        let method = "getblocktemplate".to_string();

        let result = make_request_no_params(rpc, method);

        let template: BlockTemplate = serde_json::from_value(result).unwrap();

        let expected_transactions: Vec<String> = vec![];

        assert_eq!(
            template.previous_block_hash,
            "0000000000000000000000000000000000000000000000000000000000000000".to_string()
        );
        assert_eq!(template.block_height, 0);
        assert_eq!(template.difficulty_target, 281474976710654);
        assert_eq!(template.transactions, expected_transactions);
        assert_eq!(template.coinbase_value, 100_000_000);

        kill_storage_async(path);
    }
}
