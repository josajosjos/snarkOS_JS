use crate::dpc::base_dpc::{parameters::CircuitParameters, BaseDPCComponents};
use snarkos_algorithms::merkle_tree::MerkleTreeDigest;
use snarkos_errors::curves::ConstraintFieldError;
use snarkos_models::{
    algorithms::{CommitmentScheme, EncryptionScheme, MerkleParameters, SignatureScheme, CRH},
    curves::to_field_vec::ToConstraintField,
};

#[derive(Derivative)]
#[derivative(Clone(bound = "C: BaseDPCComponents"))]
pub struct InnerCircuitVerifierInput<C: BaseDPCComponents> {
    // Commitment, CRH, and signature parameters
    pub circuit_parameters: CircuitParameters<C>,

    // Ledger parameters and digest
    pub ledger_parameters: C::MerkleParameters,
    pub ledger_digest: MerkleTreeDigest<C::MerkleParameters>,

    // Input record serial numbers and death predicate commitments
    pub old_serial_numbers: Vec<<C::AccountSignature as SignatureScheme>::PublicKey>,

    // Output record commitments and birth predicate commitments
    pub new_commitments: Vec<<C::RecordCommitment as CommitmentScheme>::Output>,

    // New record ciphertext hashes
    pub new_records_ciphertext_hashes: Vec<<C::RecordCiphertextCRH as CRH>::Output>,

    // Predicate input commitment and memo
    pub predicate_commitment: <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Output,
    pub local_data_commitment: <C::LocalDataCRH as CRH>::Output,
    pub memo: [u8; 32],

    pub value_balance: i64,

    pub network_id: u8,
}

impl<C: BaseDPCComponents> ToConstraintField<C::InnerField> for InnerCircuitVerifierInput<C>
where
    <C::AccountCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,
    <C::AccountCommitment as CommitmentScheme>::Output: ToConstraintField<C::InnerField>,

    <C::AccountEncryption as EncryptionScheme>::Parameters: ToConstraintField<C::InnerField>,

    <C::AccountSignature as SignatureScheme>::Parameters: ToConstraintField<C::InnerField>,
    <C::AccountSignature as SignatureScheme>::PublicKey: ToConstraintField<C::InnerField>,

    <C::RecordCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,
    <C::RecordCommitment as CommitmentScheme>::Output: ToConstraintField<C::InnerField>,

    <C::RecordCiphertextCRH as CRH>::Parameters: ToConstraintField<C::InnerField>,
    <C::RecordCiphertextCRH as CRH>::Output: ToConstraintField<C::InnerField>,

    <C::SerialNumberNonceCRH as CRH>::Parameters: ToConstraintField<C::InnerField>,

    <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,
    <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Output: ToConstraintField<C::InnerField>,

    <C::LocalDataCRH as CRH>::Parameters: ToConstraintField<C::InnerField>,
    <C::LocalDataCRH as CRH>::Output: ToConstraintField<C::InnerField>,

    <C::ValueCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,

    <<C::MerkleParameters as MerkleParameters>::H as CRH>::Parameters: ToConstraintField<C::InnerField>,
    MerkleTreeDigest<C::MerkleParameters>: ToConstraintField<C::InnerField>,
{
    fn to_field_elements(&self) -> Result<Vec<C::InnerField>, ConstraintFieldError> {
        let mut v = Vec::new();

        v.extend_from_slice(
            &self
                .circuit_parameters
                .account_commitment
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .account_encryption
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .account_signature
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .record_commitment
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .record_ciphertext_crh
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .predicate_verification_key_commitment
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .local_data_crh
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .serial_number_nonce
                .parameters()
                .to_field_elements()?,
        );
        v.extend_from_slice(
            &self
                .circuit_parameters
                .value_commitment
                .parameters()
                .to_field_elements()?,
        );

        v.extend_from_slice(&self.ledger_parameters.parameters().to_field_elements()?);
        v.extend_from_slice(&self.ledger_digest.to_field_elements()?);

        for sn in &self.old_serial_numbers {
            v.extend_from_slice(&sn.to_field_elements()?);
        }

        for (cm, ciphertext_hash) in self.new_commitments.iter().zip(&self.new_records_ciphertext_hashes) {
            v.extend_from_slice(&cm.to_field_elements()?);
            v.extend_from_slice(&ciphertext_hash.to_field_elements()?);
        }

        v.extend_from_slice(&self.predicate_commitment.to_field_elements()?);
        v.extend_from_slice(&ToConstraintField::<C::InnerField>::to_field_elements(&self.memo)?);
        v.extend_from_slice(&ToConstraintField::<C::InnerField>::to_field_elements(
            &[self.network_id][..],
        )?);
        v.extend_from_slice(&self.local_data_commitment.to_field_elements()?);

        let value_balance_as_u64 = self.value_balance.abs() as u64;

        let is_negative: bool = self.value_balance.is_negative();

        v.extend_from_slice(&ToConstraintField::<C::InnerField>::to_field_elements(
            &value_balance_as_u64.to_le_bytes()[..],
        )?);

        v.extend_from_slice(&ToConstraintField::<C::InnerField>::to_field_elements(
            &[is_negative as u8][..],
        )?);

        Ok(v)
    }
}
