use crate::{variable_length_integer, BlockHeader, Transactions};
use snarkos_errors::objects::BlockError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    /// First 84 bytes of the block as defined by the encoding used by
    /// "block" messages.
    pub header: BlockHeader,
    /// The block transactions.
    pub transactions: Transactions,
}

impl Block {
    pub fn serialize(&self) -> Result<Vec<u8>, BlockError> {
        let mut serialization = vec![];
        serialization.extend(&self.header.serialize().to_vec());
        serialization.extend(&variable_length_integer(self.transactions.len() as u64));

        for transaction in self.transactions.iter() {
            serialization.extend(transaction.serialize()?)
        }

        Ok(serialization)
    }

    pub fn deserialize(bytes: &Vec<u8>) -> Result<Block, BlockError> {
        let (header_bytes, transactions_bytes) = bytes.split_at(84);

        let mut header_array: [u8; 84] = [0u8; 84];
        header_array.copy_from_slice(&header_bytes[0..84]);
        let header = BlockHeader::deserialize(&header_array);

        let transactions = Transactions::deserialize(transactions_bytes)?;

        Ok(Block { header, transactions })
    }
}
