use crate::dpc::base_dpc::{parameters::CircuitParameters, BaseDPCComponents};
use snarkos_errors::{curves::ConstraintFieldError, gadgets::SynthesisError};
use snarkos_models::{
    algorithms::{CommitmentScheme, CRH},
    curves::to_field_vec::ToConstraintField,
};
use snarkos_utilities::{bytes::ToBytes, to_bytes};

#[derive(Derivative)]
#[derivative(Clone(bound = "C: BaseDPCComponents"))]
pub struct OuterCircuitVerifierInput<C: BaseDPCComponents> {
    pub circuit_parameters: CircuitParameters<C>,
    pub predicate_commitment: <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Output,
    pub local_data_commitment: <C::LocalDataCommitment as CommitmentScheme>::Output,
}

impl<C: BaseDPCComponents> ToConstraintField<C::OuterField> for OuterCircuitVerifierInput<C>
where
    <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::OuterField>,
    <C::PredicateVerificationKeyCommitment as CommitmentScheme>::Output: ToConstraintField<C::OuterField>,

    <C::PredicateVerificationKeyHash as CRH>::Parameters: ToConstraintField<C::OuterField>,

    <C::LocalDataCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,
    <C::LocalDataCommitment as CommitmentScheme>::Output: ToConstraintField<C::InnerField>,
    <C::ValueCommitment as CommitmentScheme>::Parameters: ToConstraintField<C::InnerField>,
{
    fn to_field_elements(&self) -> Result<Vec<C::OuterField>, ConstraintFieldError> {
        let mut v = Vec::new();

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
                .predicate_verification_key_hash
                .parameters()
                .to_field_elements()?,
        );

        let local_data_commitment_parameters_fe = ToConstraintField::<C::InnerField>::to_field_elements(
            self.circuit_parameters.local_data_commitment.parameters(),
        )
        .map_err(|_| SynthesisError::AssignmentMissing)?;

        let local_data_commitment_fe =
            ToConstraintField::<C::InnerField>::to_field_elements(&self.local_data_commitment)
                .map_err(|_| SynthesisError::AssignmentMissing)?;

        let value_commitment_parameters_fe = ToConstraintField::<C::InnerField>::to_field_elements(
            self.circuit_parameters.value_commitment.parameters(),
        )
        .map_err(|_| SynthesisError::AssignmentMissing)?;

        // Then we convert these field elements into bytes
        let predicate_input = [
            to_bytes![local_data_commitment_parameters_fe].map_err(|_| SynthesisError::AssignmentMissing)?,
            to_bytes![local_data_commitment_fe].map_err(|_| SynthesisError::AssignmentMissing)?,
            to_bytes![value_commitment_parameters_fe].map_err(|_| SynthesisError::AssignmentMissing)?,
        ];

        // Then we convert them into `C::ProofCheckF::Fr` elements.
        v.extend_from_slice(&ToConstraintField::<C::OuterField>::to_field_elements(
            predicate_input[0].as_slice(),
        )?);
        v.extend_from_slice(&ToConstraintField::<C::OuterField>::to_field_elements(
            predicate_input[1].as_slice(),
        )?);
        v.extend_from_slice(&ToConstraintField::<C::OuterField>::to_field_elements(
            predicate_input[2].as_slice(),
        )?);

        v.extend_from_slice(&self.predicate_commitment.to_field_elements()?);
        Ok(v)
    }
}
