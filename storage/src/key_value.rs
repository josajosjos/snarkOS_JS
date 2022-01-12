use snarkos_utilities::bytes::{FromBytes, ToBytes};

use std::io::{Read, Result as IoResult, Write};

pub const COL_META: u32 = 0; // MISC Values
pub const COL_BLOCK_HEADER: u32 = 1; // Block hash -> block header
pub const COL_BLOCK_TRANSACTIONS: u32 = 2; // Block hash -> block transactions
pub const COL_BLOCK_LOCATOR: u32 = 3; // Block num -> block hash && block hash -> block num
pub const COL_TRANSACTION_LOCATION: u32 = 4; // Transaction Hash -> (block hash and index)
pub const COL_COMMITMENT: u32 = 5; // Commitment -> index
pub const COL_SERIAL_NUMBER: u32 = 6; // SN -> index
pub const COL_MEMO: u32 = 7; // Memo -> index
pub const COL_DIGEST: u32 = 8; // Ledger digest -> index

pub const COL_RECORDS: u32 = 9; // commitment -> record bytes

pub const NUM_COLS: u32 = 10;

pub const KEY_BEST_BLOCK_NUMBER: &str = "BEST_BLOCK_NUMBER";
pub const KEY_MEMORY_POOL: &str = "MEMORY_POOL";
pub const KEY_PEER_BOOK: &str = "PEER_BOOK";

pub const KEY_GENESIS_CM: &str = "GENESIS_CM";
pub const KEY_GENESIS_SN: &str = "GENESIS_SN";
pub const KEY_GENESIS_MEMO: &str = "GENESIS_MEMO";
pub const KEY_GENESIS_PRED_VK: &str = "GENESIS_PREDICATE_VK";
pub const KEY_GENESIS_ADDRESS_PAIR: &str = "GENESIS_ADDRESS_PAIR";

pub const KEY_CURR_CM_INDEX: &str = "CURRENT_CM_INDEX";
pub const KEY_CURR_SN_INDEX: &str = "CURRENT_SN_INDEX";
pub const KEY_CURR_MEMO_INDEX: &str = "CURRENT_MEMO_INDEX";
pub const KEY_CURR_DIGEST: &str = "CURRENT_DIGEST";

/// Represents address of certain transaction within block
#[derive(Debug, PartialEq, Clone)]
pub struct TransactionLocation {
    /// Transaction index within the block
    pub index: u32,
    /// Block hash
    pub block_hash: [u8; 32],
}

impl ToBytes for TransactionLocation {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.index.write(&mut writer)?;
        self.block_hash.write(&mut writer)
    }
}

impl FromBytes for TransactionLocation {
    #[inline]
    fn read<R: Read>(mut reader: R) -> IoResult<Self> {
        let index: u32 = FromBytes::read(&mut reader)?;
        let block_hash: [u8; 32] = FromBytes::read(&mut reader)?;

        Ok(Self { index, block_hash })
    }
}

/// Database operation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Insert { col: u32, key: Vec<u8>, value: Vec<u8> },
    Delete { col: u32, key: Vec<u8> },
}

impl Op {
    pub fn key(&self) -> &[u8] {
        match self {
            Op::Insert { key, .. } => &key,
            Op::Delete { key, .. } => &key,
        }
    }

    pub fn col(&self) -> u32 {
        match self {
            Op::Insert { col, .. } => *col,
            Op::Delete { col, .. } => *col,
        }
    }
}

/// Batched transaction of database operations.
#[derive(Default, Clone, PartialEq)]
pub struct DatabaseTransaction(pub Vec<Op>);

impl DatabaseTransaction {
    /// Create new transaction.
    pub fn new() -> Self {
        Self(vec![])
    }

    /// Add a key value pair under a specific col.
    pub fn add(&mut self, col: u32, key: &[u8], value: &[u8]) {
        self.0.push(Op::Insert {
            col,
            key: key.to_vec(),
            value: value.to_vec(),
        })
    }

    /// Delete a value given a col and key.
    pub fn delete(&mut self, col: u32, key: &[u8]) {
        self.0.push(Op::Delete { col, key: key.to_vec() })
    }

    /// Add an operation.
    pub fn push(&mut self, op: Op) {
        self.0.push(op)
    }

    /// Add a vector of operations.
    pub fn push_vec(&mut self, ops: Vec<Op>) {
        self.0.extend(ops)
    }

    /// Add another database transaction.
    pub fn extend(&mut self, database_transaction: DatabaseTransaction) {
        self.0.extend(database_transaction.0)
    }
}

pub fn bytes_to_u32(bytes: Vec<u8>) -> u32 {
    let mut num_bytes = [0u8; 4];
    num_bytes.copy_from_slice(&bytes);

    u32::from_le_bytes(num_bytes)
}
