use snarkos_utilities::bytes::{FromBytes, ToBytes};

use std::hash::Hash;

pub trait Record: Default + FromBytes + ToBytes {
    type Owner;
    type Commitment: FromBytes + ToBytes;
    type CommitmentRandomness;
    type Payload;
    type SerialNumberNonce;
    type SerialNumber: Clone + Eq + Hash + FromBytes + ToBytes;
    type Value: FromBytes + ToBytes;

    /// Returns the record owner.
    fn owner(&self) -> &Self::Owner;

    /// Returns whether or not the record is dummy.
    fn is_dummy(&self) -> bool;

    /// Returns the record payload.
    fn payload(&self) -> &Self::Payload;

    /// Returns the birth program id of this record.
    fn birth_program_id(&self) -> &[u8];

    /// Returns the death program id of this record.
    fn death_program_id(&self) -> &[u8];

    /// Returns the randomness used for the serial number.
    fn serial_number_nonce(&self) -> &Self::SerialNumberNonce;

    /// Returns the commitment of this record.
    fn commitment(&self) -> Self::Commitment;

    /// Returns the randomness used for the commitment.
    fn commitment_randomness(&self) -> Self::CommitmentRandomness;

    /// Returns the record value.
    fn value(&self) -> Self::Value;
}
