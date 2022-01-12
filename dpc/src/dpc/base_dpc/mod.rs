use crate::dpc::base_dpc::{
    binding_signature::*,
    record_payload::RecordPayload,
    records::record_serializer::{decode_from_group, RecordSerializer},
};
use snarkos_algorithms::{
    encoding::Elligator2,
    merkle_tree::{MerklePath, MerkleTreeDigest},
};
use snarkos_errors::dpc::DPCError;
use snarkos_models::{
    algorithms::{
        CommitmentScheme,
        EncryptionScheme,
        LoadableMerkleParameters,
        MerkleParameters,
        SignatureScheme,
        CRH,
        PRF,
        SNARK,
    },
    curves::{AffineCurve, Group, ModelParameters, MontgomeryModelParameters, ProjectiveCurve, TEModelParameters},
    dpc::{DPCComponents, DPCScheme, Predicate, Record, RecordSerializerScheme},
    gadgets::algorithms::{BindingSignatureGadget, CRHGadget, CommitmentGadget, SNARKVerifierGadget},
    objects::{AccountScheme, LedgerScheme, Transaction},
};
use snarkos_objects::{Account, AccountAddress, AccountPrivateKey};
use snarkos_utilities::{
    bytes::{bits_to_bytes, bytes_to_bits, FromBytes, ToBytes},
    has_duplicates,
    rand::UniformRand,
    to_bytes,
};

use itertools::Itertools;
use rand::Rng;
use std::marker::PhantomData;

pub mod binding_signature;

pub mod predicate;
use self::predicate::*;

pub mod record;
use self::record::*;

pub mod transaction;
use self::transaction::*;

pub mod inner_circuit;
use self::inner_circuit::*;

pub mod inner_circuit_gadget;
pub use self::inner_circuit_gadget::*;

pub mod inner_circuit_verifier_input;
use self::inner_circuit_verifier_input::*;

pub mod predicate_circuit;
use self::predicate_circuit::*;

pub mod outer_circuit;
use self::outer_circuit::*;

pub mod outer_circuit_gadget;
pub use self::outer_circuit_gadget::*;

pub mod outer_circuit_verifier_input;
use self::outer_circuit_verifier_input::*;

pub mod parameters;
use self::parameters::*;

pub mod records;

pub mod record_payload;

pub mod instantiated;

#[cfg(test)]
mod test;

///////////////////////////////////////////////////////////////////////////////

/// Trait that stores all information about the components of a Plain DPC
/// scheme. Simplifies the interface of Plain DPC by wrapping all these into
/// one.
pub trait BaseDPCComponents: DPCComponents {
    /// Ledger digest type.
    type MerkleParameters: LoadableMerkleParameters;
    type MerkleHashGadget: CRHGadget<<Self::MerkleParameters as MerkleParameters>::H, Self::InnerField>;

    /// Commitment scheme for committing to a record value
    type ValueCommitment: CommitmentScheme;
    type ValueCommitmentGadget: CommitmentGadget<Self::ValueCommitment, Self::InnerField>;

    /// Gadget for verifying the binding signature
    type BindingSignatureGroup: Group + ProjectiveCurve;
    type BindingSignatureGadget: BindingSignatureGadget<
        Self::ValueCommitment,
        Self::InnerField,
        Self::BindingSignatureGroup,
    >;

    /// Group and Model Parameters for record encryption
    type EncryptionGroup: Group + ProjectiveCurve;
    type EncryptionModelParameters: MontgomeryModelParameters + TEModelParameters;

    /// SNARK for non-proof-verification checks
    type InnerSNARK: SNARK<
        Circuit = InnerCircuit<Self>,
        AssignedCircuit = InnerCircuit<Self>,
        VerifierInput = InnerCircuitVerifierInput<Self>,
    >;

    /// SNARK Verifier gadget for the inner snark
    type InnerSNARKGadget: SNARKVerifierGadget<Self::InnerSNARK, Self::OuterField>;

    /// SNARK for proof-verification checks
    type OuterSNARK: SNARK<
        Circuit = OuterCircuit<Self>,
        AssignedCircuit = OuterCircuit<Self>,
        VerifierInput = OuterCircuitVerifierInput<Self>,
    >;

    /// SNARK for a "dummy predicate" that does nothing with its input.
    type PredicateSNARK: SNARK<
        Circuit = PredicateCircuit<Self>,
        AssignedCircuit = PredicateCircuit<Self>,
        VerifierInput = PredicateLocalData<Self>,
    >;

    /// SNARK Verifier gadget for the "dummy predicate" that does nothing with its input.
    type PredicateSNARKGadget: SNARKVerifierGadget<Self::PredicateSNARK, Self::OuterField>;
}

///////////////////////////////////////////////////////////////////////////////

pub struct DPC<Components: BaseDPCComponents> {
    _components: PhantomData<Components>,
}

/// Returned by `PlainDPC::execute_helper`. Stores data required to produce the
/// final transaction after `execute_helper` has created old serial numbers and
/// ledger witnesses, and new records and commitments. For convenience, it also
/// stores references to existing information like old records and secret keys.
pub(crate) struct ExecuteContext<'a, L, Components: BaseDPCComponents>
where
    L: LedgerScheme<
        Commitment = <Components::RecordCommitment as CommitmentScheme>::Output,
        MerkleParameters = Components::MerkleParameters,
        MerklePath = MerklePath<Components::MerkleParameters>,
        MerkleTreeDigest = MerkleTreeDigest<Components::MerkleParameters>,
        SerialNumber = <Components::AccountSignature as SignatureScheme>::PublicKey,
    >,
{
    circuit_parameters: &'a CircuitParameters<Components>,
    ledger_digest: L::MerkleTreeDigest,

    // Old record stuff
    old_account_private_keys: &'a [AccountPrivateKey<Components>],
    old_records: &'a [DPCRecord<Components>],
    old_witnesses: Vec<MerklePath<Components::MerkleParameters>>,
    old_serial_numbers: Vec<<Components::AccountSignature as SignatureScheme>::PublicKey>,
    old_randomizers: Vec<Vec<u8>>,

    // New record stuff
    new_records: Vec<DPCRecord<Components>>,
    new_sn_nonce_randomness: Vec<[u8; 32]>,
    new_commitments: Vec<<Components::RecordCommitment as CommitmentScheme>::Output>,

    // Predicate and local data commitment and randomness
    predicate_commitment: <Components::PredicateVerificationKeyCommitment as CommitmentScheme>::Output,
    predicate_randomness: <Components::PredicateVerificationKeyCommitment as CommitmentScheme>::Randomness,

    local_data_commitment: <Components::LocalDataCRH as CRH>::Output,
    local_data_commitment_randomizers: Vec<<Components::LocalDataCommitment as CommitmentScheme>::Randomness>,

    // Value Balance
    value_balance: i64,
}

impl<L, Components: BaseDPCComponents> ExecuteContext<'_, L, Components>
where
    L: LedgerScheme<
        Commitment = <Components::RecordCommitment as CommitmentScheme>::Output,
        MerkleParameters = Components::MerkleParameters,
        MerklePath = MerklePath<Components::MerkleParameters>,
        MerkleTreeDigest = MerkleTreeDigest<Components::MerkleParameters>,
        SerialNumber = <Components::AccountSignature as SignatureScheme>::PublicKey,
    >,
{
    fn into_local_data(&self) -> LocalData<Components> {
        LocalData {
            circuit_parameters: self.circuit_parameters.clone(),

            old_records: self.old_records.to_vec(),
            old_serial_numbers: self.old_serial_numbers.to_vec(),

            new_records: self.new_records.to_vec(),

            local_data_commitment: self.local_data_commitment.clone(),
            local_data_commitment_randomizers: self.local_data_commitment_randomizers.clone(),
        }
    }
}

/// Stores local data required to produce predicate proofs.
pub struct LocalData<Components: BaseDPCComponents> {
    pub circuit_parameters: CircuitParameters<Components>,

    // Old records and serial numbers
    pub old_records: Vec<DPCRecord<Components>>,
    pub old_serial_numbers: Vec<<Components::AccountSignature as SignatureScheme>::PublicKey>,

    // New records
    pub new_records: Vec<DPCRecord<Components>>,

    // Commitment to the above information.
    pub local_data_commitment: <Components::LocalDataCRH as CRH>::Output,
    pub local_data_commitment_randomizers: Vec<<Components::LocalDataCommitment as CommitmentScheme>::Randomness>,
}

///////////////////////////////////////////////////////////////////////////////

impl<Components: BaseDPCComponents> DPC<Components> {
    pub fn generate_circuit_parameters<R: Rng>(rng: &mut R) -> Result<CircuitParameters<Components>, DPCError> {
        let time = start_timer!(|| "Account commitment scheme setup");
        let account_commitment = Components::AccountCommitment::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Account encryption scheme setup");
        let account_encryption = Components::AccountEncryption::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Account signature setup");
        let account_signature = Components::AccountSignature::setup(rng)?;
        end_timer!(time);

        let time = start_timer!(|| "Record commitment scheme setup");
        let record_commitment = Components::RecordCommitment::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Record ciphertext CRH setup");
        let record_ciphertext_crh = Components::RecordCiphertextCRH::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Verification key commitment setup");
        let predicate_verification_key_commitment = Components::PredicateVerificationKeyCommitment::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Local data CRH setup");
        let local_data_crh = Components::LocalDataCRH::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Local data commitment setup");
        let local_data_commitment = Components::LocalDataCommitment::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Value commitment setup");
        let value_commitment = Components::ValueCommitment::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Serial nonce CRH setup");
        let serial_number_nonce = Components::SerialNumberNonceCRH::setup(rng);
        end_timer!(time);

        let time = start_timer!(|| "Verification key CRH setup");
        let predicate_verification_key_hash = Components::PredicateVerificationKeyHash::setup(rng);
        end_timer!(time);

        let comm_crh_sig_pp = CircuitParameters {
            account_commitment,
            account_encryption,
            account_signature,
            record_commitment,
            record_ciphertext_crh,
            predicate_verification_key_commitment,
            predicate_verification_key_hash,
            local_data_crh,
            local_data_commitment,
            value_commitment,
            serial_number_nonce,
        };

        Ok(comm_crh_sig_pp)
    }

    pub fn generate_predicate_snark_parameters<R: Rng>(
        circuit_parameters: &CircuitParameters<Components>,
        rng: &mut R,
    ) -> Result<PredicateSNARKParameters<Components>, DPCError> {
        let (pk, pvk) = Components::PredicateSNARK::setup(PredicateCircuit::blank(circuit_parameters), rng)?;

        Ok(PredicateSNARKParameters {
            proving_key: pk,
            verification_key: pvk.into(),
        })
    }

    pub fn generate_sn(
        params: &CircuitParameters<Components>,
        record: &DPCRecord<Components>,
        account_private_key: &AccountPrivateKey<Components>,
    ) -> Result<(<Components::AccountSignature as SignatureScheme>::PublicKey, Vec<u8>), DPCError> {
        let sn_time = start_timer!(|| "Generate serial number");
        let sk_prf = &account_private_key.sk_prf;
        let sn_nonce = to_bytes!(record.serial_number_nonce())?;
        // Compute the serial number.
        let prf_input = FromBytes::read(sn_nonce.as_slice())?;
        let prf_seed = FromBytes::read(to_bytes!(sk_prf)?.as_slice())?;
        let sig_and_pk_randomizer = to_bytes![Components::PRF::evaluate(&prf_seed, &prf_input)?]?;

        let sn = Components::AccountSignature::randomize_public_key(
            &params.account_signature,
            &account_private_key.pk_sig(&params.account_signature)?,
            &sig_and_pk_randomizer,
        )?;
        end_timer!(sn_time);
        Ok((sn, sig_and_pk_randomizer))
    }

    pub fn generate_record<R: Rng>(
        parameters: &CircuitParameters<Components>,
        sn_nonce: &<Components::SerialNumberNonceCRH as CRH>::Output,
        account_address: &AccountAddress<Components>,
        is_dummy: bool,
        value: u64,
        payload: &RecordPayload,
        birth_predicate: &DPCPredicate<Components>,
        death_predicate: &DPCPredicate<Components>,
        rng: &mut R,
    ) -> Result<DPCRecord<Components>, DPCError> {
        let record_time = start_timer!(|| "Generate record");
        // Sample new commitment randomness.
        let commitment_randomness = <Components::RecordCommitment as CommitmentScheme>::Randomness::rand(rng);

        // Construct a record commitment.
        let birth_predicate_hash = birth_predicate.into_compact_repr();
        let death_predicate_hash = death_predicate.into_compact_repr();
        // Total = 32 + 1 + 8 + 32 + 32 + 32 + 32 = 169 bytes
        let commitment_input = to_bytes![
            account_address,      // 256 bits = 32 bytes
            is_dummy,             // 1 bit = 1 byte
            value,                // 64 bits = 8 bytes
            payload,              // 256 bits = 32 bytes
            birth_predicate_hash, // 256 bits = 32 bytes
            death_predicate_hash, // 256 bits = 32 bytes
            sn_nonce              // 256 bits = 32 bytes
        ]?;

        let commitment = Components::RecordCommitment::commit(
            &parameters.record_commitment,
            &commitment_input,
            &commitment_randomness,
        )?;

        let record = DPCRecord {
            account_address: account_address.clone(),
            is_dummy,
            value,
            payload: payload.clone(),
            birth_predicate_hash,
            death_predicate_hash,
            serial_number_nonce: sn_nonce.clone(),
            commitment,
            commitment_randomness,
            _components: PhantomData,
        };
        end_timer!(record_time);
        Ok(record)
    }

    pub(crate) fn execute_helper<'a, L, R: Rng>(
        parameters: &'a CircuitParameters<Components>,

        old_records: &'a [<Self as DPCScheme<L>>::Record],
        old_account_private_keys: &'a [AccountPrivateKey<Components>],

        new_account_address: &[AccountAddress<Components>],
        new_is_dummy_flags: &[bool],
        new_values: &[u64],
        new_payloads: &[<Self as DPCScheme<L>>::Payload],
        new_birth_predicates: &[<Self as DPCScheme<L>>::Predicate],
        new_death_predicates: &[<Self as DPCScheme<L>>::Predicate],

        memo: &[u8; 32],
        network_id: u8,

        ledger: &L,
        rng: &mut R,
    ) -> Result<ExecuteContext<'a, L, Components>, DPCError>
    where
        L: LedgerScheme<
            Commitment = <Components::RecordCommitment as CommitmentScheme>::Output,
            MerkleParameters = Components::MerkleParameters,
            MerklePath = MerklePath<Components::MerkleParameters>,
            MerkleTreeDigest = MerkleTreeDigest<Components::MerkleParameters>,
            SerialNumber = <Components::AccountSignature as SignatureScheme>::PublicKey,
            Transaction = DPCTransaction<Components>,
        >,
    {
        assert_eq!(Components::NUM_INPUT_RECORDS, old_records.len());
        assert_eq!(Components::NUM_INPUT_RECORDS, old_account_private_keys.len());

        assert_eq!(Components::NUM_OUTPUT_RECORDS, new_account_address.len());
        assert_eq!(Components::NUM_OUTPUT_RECORDS, new_is_dummy_flags.len());
        assert_eq!(Components::NUM_OUTPUT_RECORDS, new_payloads.len());
        assert_eq!(Components::NUM_OUTPUT_RECORDS, new_birth_predicates.len());
        assert_eq!(Components::NUM_OUTPUT_RECORDS, new_death_predicates.len());

        let mut old_witnesses = Vec::with_capacity(Components::NUM_INPUT_RECORDS);
        let mut old_serial_numbers = Vec::with_capacity(Components::NUM_INPUT_RECORDS);
        let mut old_randomizers = Vec::with_capacity(Components::NUM_INPUT_RECORDS);
        let mut joint_serial_numbers = Vec::new();
        let mut old_death_pred_hashes = Vec::new();

        let mut value_balance: i64 = 0;

        // Compute the ledger membership witness and serial number from the old records.
        for (i, record) in old_records.iter().enumerate() {
            let input_record_time = start_timer!(|| format!("Process input record {}", i));

            if record.is_dummy() {
                old_witnesses.push(MerklePath::default());
            } else {
                let witness = ledger.prove_cm(&record.commitment())?;
                old_witnesses.push(witness);

                value_balance += record.value() as i64;
            }

            let (sn, randomizer) = Self::generate_sn(&parameters, record, &old_account_private_keys[i])?;
            joint_serial_numbers.extend_from_slice(&to_bytes![sn]?);
            old_serial_numbers.push(sn);
            old_randomizers.push(randomizer);
            old_death_pred_hashes.push(record.death_predicate_hash().to_vec());

            end_timer!(input_record_time);
        }

        let mut new_records = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_commitments = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_sn_nonce_randomness = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_birth_pred_hashes = Vec::new();

        // Generate new records and commitments for them.
        for j in 0..Components::NUM_OUTPUT_RECORDS {
            let output_record_time = start_timer!(|| format!("Process output record {}", j));
            let sn_nonce_time = start_timer!(|| "Generate serial number nonce");

            // Sample randomness sn_randomness for the CRH input.
            let sn_randomness: [u8; 32] = rng.gen();

            let crh_input = to_bytes![j as u8, sn_randomness, joint_serial_numbers]?;
            let sn_nonce = Components::SerialNumberNonceCRH::hash(&parameters.serial_number_nonce, &crh_input)?;

            end_timer!(sn_nonce_time);

            let record = Self::generate_record(
                parameters,
                &sn_nonce,
                &new_account_address[j],
                new_is_dummy_flags[j],
                new_values[j],
                &new_payloads[j],
                &new_birth_predicates[j],
                &new_death_predicates[j],
                rng,
            )?;

            if !record.is_dummy {
                value_balance -= record.value() as i64;
            }

            new_commitments.push(record.commitment.clone());
            new_sn_nonce_randomness.push(sn_randomness);
            new_birth_pred_hashes.push(record.birth_predicate_hash().to_vec());
            new_records.push(record);

            end_timer!(output_record_time);
        }

        let local_data_comm_timer = start_timer!(|| "Compute local data commitment");

        let mut local_data_commitment_randomizers = vec![];

        let mut old_record_commitments = Vec::new();
        for i in 0..Components::NUM_INPUT_RECORDS {
            let record = &old_records[i];
            let input_bytes = to_bytes![old_serial_numbers[i], record.commitment(), memo, network_id]?;

            let commitment_randomness = <Components::LocalDataCommitment as CommitmentScheme>::Randomness::rand(rng);
            let commitment = Components::LocalDataCommitment::commit(
                &parameters.local_data_commitment,
                &input_bytes,
                &commitment_randomness,
            )?;

            old_record_commitments.extend_from_slice(&to_bytes![commitment]?);
            local_data_commitment_randomizers.push(commitment_randomness);
        }

        let mut new_record_commitments = Vec::new();
        for j in 0..Components::NUM_OUTPUT_RECORDS {
            let record = &new_records[j];
            let input_bytes = to_bytes![record.commitment(), memo, network_id]?;

            let commitment_randomness = <Components::LocalDataCommitment as CommitmentScheme>::Randomness::rand(rng);
            let commitment = Components::LocalDataCommitment::commit(
                &parameters.local_data_commitment,
                &input_bytes,
                &commitment_randomness,
            )?;

            new_record_commitments.extend_from_slice(&to_bytes![commitment]?);
            local_data_commitment_randomizers.push(commitment_randomness);
        }

        let inner1_hash = Components::LocalDataCRH::hash(&parameters.local_data_crh, &old_record_commitments)?;

        let inner2_hash = Components::LocalDataCRH::hash(&parameters.local_data_crh, &new_record_commitments)?;

        let local_data_comm =
            Components::LocalDataCRH::hash(&parameters.local_data_crh, &to_bytes![inner1_hash, inner2_hash]?)?;

        end_timer!(local_data_comm_timer);

        let pred_hash_comm_timer = start_timer!(|| "Compute predicate commitment");
        let (predicate_comm, predicate_rand) = {
            let mut input = Vec::new();
            for hash in old_death_pred_hashes {
                input.extend_from_slice(&hash);
            }

            for hash in new_birth_pred_hashes {
                input.extend_from_slice(&hash);
            }
            let predicate_rand =
                <Components::PredicateVerificationKeyCommitment as CommitmentScheme>::Randomness::rand(rng);
            let predicate_comm = Components::PredicateVerificationKeyCommitment::commit(
                &parameters.predicate_verification_key_commitment,
                &input,
                &predicate_rand,
            )?;
            (predicate_comm, predicate_rand)
        };
        end_timer!(pred_hash_comm_timer);

        let ledger_digest = ledger.digest().expect("could not get digest");

        let context = ExecuteContext {
            circuit_parameters: parameters,
            ledger_digest,

            old_records,
            old_witnesses,
            old_account_private_keys,
            old_serial_numbers,
            old_randomizers,

            new_records,
            new_sn_nonce_randomness,
            new_commitments,

            predicate_commitment: predicate_comm,
            predicate_randomness: predicate_rand,
            local_data_commitment: local_data_comm,
            local_data_commitment_randomizers,

            value_balance,
        };
        Ok(context)
    }
}

impl<Components: BaseDPCComponents, L: LedgerScheme> DPCScheme<L> for DPC<Components>
where
    L: LedgerScheme<
        Commitment = <Components::RecordCommitment as CommitmentScheme>::Output,
        MerkleParameters = Components::MerkleParameters,
        MerklePath = MerklePath<Components::MerkleParameters>,
        MerkleTreeDigest = MerkleTreeDigest<Components::MerkleParameters>,
        SerialNumber = <Components::AccountSignature as SignatureScheme>::PublicKey,
        Transaction = DPCTransaction<Components>,
    >,
{
    type Account = Account<Components>;
    type LocalData = LocalData<Components>;
    type Metadata = [u8; 32];
    type Parameters = PublicParameters<Components>;
    type Payload = <Self::Record as Record>::Payload;
    type Predicate = DPCPredicate<Components>;
    type PrivatePredInput = PrivatePredicateInput<Components>;
    type Record = DPCRecord<Components>;
    type Transaction = DPCTransaction<Components>;

    fn setup<R: Rng>(
        ledger_parameters: &Components::MerkleParameters,
        rng: &mut R,
    ) -> Result<Self::Parameters, DPCError> {
        let setup_time = start_timer!(|| "BaseDPC::setup");
        let circuit_parameters = Self::generate_circuit_parameters(rng)?;

        let predicate_snark_setup_time = start_timer!(|| "Dummy predicate SNARK setup");
        let predicate_snark_parameters = Self::generate_predicate_snark_parameters(&circuit_parameters, rng)?;
        let predicate_snark_proof = Components::PredicateSNARK::prove(
            &predicate_snark_parameters.proving_key,
            PredicateCircuit::blank(&circuit_parameters),
            rng,
        )?;
        end_timer!(predicate_snark_setup_time);

        let private_pred_input = PrivatePredicateInput {
            verification_key: predicate_snark_parameters.verification_key.clone(),
            proof: predicate_snark_proof,
        };

        let snark_setup_time = start_timer!(|| "Execute inner SNARK setup");
        let inner_snark_parameters =
            Components::InnerSNARK::setup(InnerCircuit::blank(&circuit_parameters, ledger_parameters), rng)?;
        end_timer!(snark_setup_time);

        let snark_setup_time = start_timer!(|| "Execute outer SNARK setup");
        let inner_snark_vk: <Components::InnerSNARK as SNARK>::VerificationParameters =
            inner_snark_parameters.1.clone().into();
        let inner_snark_proof = Components::InnerSNARK::prove(
            &inner_snark_parameters.0,
            InnerCircuit::blank(&circuit_parameters, ledger_parameters),
            rng,
        )?;

        let outer_snark_parameters = Components::OuterSNARK::setup(
            OuterCircuit::blank(
                &circuit_parameters,
                ledger_parameters,
                &inner_snark_vk,
                &inner_snark_proof,
                &private_pred_input,
            ),
            rng,
        )?;
        end_timer!(snark_setup_time);
        end_timer!(setup_time);

        let inner_snark_parameters = (Some(inner_snark_parameters.0), inner_snark_parameters.1);
        let outer_snark_parameters = (Some(outer_snark_parameters.0), outer_snark_parameters.1);

        Ok(PublicParameters {
            circuit_parameters,
            predicate_snark_parameters,
            inner_snark_parameters,
            outer_snark_parameters,
        })
    }

    fn create_account<R: Rng>(parameters: &Self::Parameters, rng: &mut R) -> Result<Self::Account, DPCError> {
        let time = start_timer!(|| "BaseDPC::create_account");

        let account_signature_parameters = &parameters.circuit_parameters.account_signature;
        let commitment_parameters = &parameters.circuit_parameters.account_commitment;
        let encryption_parameters = &parameters.circuit_parameters.account_encryption;
        let account = Account::new(
            account_signature_parameters,
            commitment_parameters,
            encryption_parameters,
            rng,
        )?;

        end_timer!(time);

        Ok(account)
    }

    fn execute<R: Rng>(
        parameters: &Self::Parameters,
        old_records: &[Self::Record],
        old_account_private_keys: &[<Self::Account as AccountScheme>::AccountPrivateKey],
        mut old_death_pred_proof_generator: impl FnMut(&Self::LocalData) -> Result<Vec<Self::PrivatePredInput>, DPCError>,

        new_account_address: &[<Self::Account as AccountScheme>::AccountAddress],
        new_is_dummy_flags: &[bool],
        new_values: &[u64],
        new_payloads: &[Self::Payload],
        new_birth_predicates: &[Self::Predicate],
        new_death_predicates: &[Self::Predicate],
        mut new_birth_pred_proof_generator: impl FnMut(&Self::LocalData) -> Result<Vec<Self::PrivatePredInput>, DPCError>,

        memorandum: &<Self::Transaction as Transaction>::Memorandum,
        network_id: u8,
        ledger: &L,
        rng: &mut R,
    ) -> Result<(Vec<Self::Record>, Self::Transaction), DPCError> {
        let exec_time = start_timer!(|| "BaseDPC::execute");
        let context = Self::execute_helper(
            &parameters.circuit_parameters,
            old_records,
            old_account_private_keys,
            new_account_address,
            new_is_dummy_flags,
            new_values,
            new_payloads,
            new_birth_predicates,
            new_death_predicates,
            memorandum,
            network_id,
            ledger,
            rng,
        )?;

        let local_data = context.into_local_data();
        let old_death_pred_attributes = old_death_pred_proof_generator(&local_data)?;
        let new_birth_pred_attributes = new_birth_pred_proof_generator(&local_data)?;

        let ExecuteContext {
            circuit_parameters,
            ledger_digest,

            old_records,
            old_witnesses,
            old_account_private_keys,
            old_serial_numbers,
            old_randomizers,

            new_records,
            new_sn_nonce_randomness,
            new_commitments,
            predicate_commitment,
            predicate_randomness,
            local_data_commitment,
            local_data_commitment_randomizers,
            value_balance,
        } = context;

        // Generate Schnorr signature on transaction data

        let signature_time = start_timer!(|| "Sign and randomize transaction contents");

        let signature_message = to_bytes![
            network_id,
            ledger_digest,
            old_serial_numbers,
            new_commitments,
            predicate_commitment,
            local_data_commitment,
            value_balance,
            memorandum
        ]?;

        let mut signatures = Vec::with_capacity(Components::NUM_INPUT_RECORDS);
        for i in 0..Components::NUM_INPUT_RECORDS {
            let sk_sig = &old_account_private_keys[i].sk_sig;
            let randomizer = &old_randomizers[i];

            // Sign the transaction data
            let account_signature = Components::AccountSignature::sign(
                &circuit_parameters.account_signature,
                sk_sig,
                &signature_message,
                rng,
            )?;

            // Randomize the signature
            let randomized_signature = Components::AccountSignature::randomize_signature(
                &circuit_parameters.account_signature,
                &account_signature,
                randomizer,
            )?;

            signatures.push(randomized_signature);
        }

        end_timer!(signature_time);

        // Generate binding signature

        // Generate value commitments for input records

        let mut old_value_commits = vec![];
        let mut old_value_commit_randomness = vec![];

        for old_record in old_records {
            // If the record is a dummy, then the value should be 0
            let input_value = match old_record.is_dummy() {
                true => 0,
                false => old_record.value(),
            };

            // Generate value commitment randomness
            let value_commitment_randomness =
                <<Components as BaseDPCComponents>::ValueCommitment as CommitmentScheme>::Randomness::rand(rng);

            // Generate the value commitment
            let value_commitment = parameters
                .circuit_parameters
                .value_commitment
                .commit(&input_value.to_le_bytes(), &value_commitment_randomness)
                .unwrap();

            old_value_commits.push(value_commitment);
            old_value_commit_randomness.push(value_commitment_randomness);
        }

        // Generate value commitments for output records

        let mut new_value_commits = vec![];
        let mut new_value_commit_randomness = vec![];

        for new_record in &new_records {
            // If the record is a dummy, then the value should be 0
            let output_value = match new_record.is_dummy() {
                true => 0,
                false => new_record.value(),
            };

            // Generate value commitment randomness
            let value_commitment_randomness =
                <<Components as BaseDPCComponents>::ValueCommitment as CommitmentScheme>::Randomness::rand(rng);

            // Generate the value commitment
            let value_commitment = parameters
                .circuit_parameters
                .value_commitment
                .commit(&output_value.to_le_bytes(), &value_commitment_randomness)
                .unwrap();

            new_value_commits.push(value_commitment);
            new_value_commit_randomness.push(value_commitment_randomness);
        }

        let sighash = to_bytes![local_data_commitment]?;

        let binding_signature =
            create_binding_signature::<Components::ValueCommitment, Components::BindingSignatureGroup, _>(
                &circuit_parameters.value_commitment,
                &old_value_commits,
                &new_value_commits,
                &old_value_commit_randomness,
                &new_value_commit_randomness,
                value_balance,
                &sighash,
                rng,
            )?;

        // Record encoding and encryption

        let mut new_records_field_elements = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_group_encoding = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_encryption_randomness = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_encryption_blinding_exponents = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_encryption_ciphertexts = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_ciphertext_and_fq_high_selectors_gadget =
            Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_ciphertext_and_fq_high_selectors = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        let mut new_records_ciphertext_hashes = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        for record in &new_records {
            // Serialize the record into group elements and fq_high bits
            let (serialized_record, final_fq_high_bit) = RecordSerializer::<
                Components,
                Components::EncryptionModelParameters,
                Components::EncryptionGroup,
            >::serialize(&record)?;

            // Extract the fq_bits
            let final_element = &serialized_record[serialized_record.len() - 1];
            let final_element_bytes = decode_from_group::<
                Components::EncryptionModelParameters,
                Components::EncryptionGroup,
            >(final_element.into_affine(), final_fq_high_bit)?;
            let final_element_bits = bytes_to_bits(&final_element_bytes);
            let fq_high_bits = [
                &final_element_bits[1..serialized_record.len()],
                &[final_fq_high_bit][..],
            ]
            .concat();

            let mut record_field_elements = vec![];
            let mut record_group_encoding = vec![];
            let mut record_plaintexts = vec![];
            // The first fq_high selector is false to account for the c_0 element in the ciphertext

            for (i, (element, fq_high)) in serialized_record.iter().zip_eq(&fq_high_bits).enumerate() {
                let element_affine = element.into_affine();

                // Decode the field elements from the serialized group element
                // These values will be used in the inner circuit to validate bit packing and serialization
                if i == 0 {
                    // Serial number nonce
                    let record_field_element =
                        <<Components as BaseDPCComponents>::EncryptionModelParameters as ModelParameters>::BaseField::read(&to_bytes![element]?[..])?;
                    record_field_elements.push(record_field_element);
                } else {
                    // Decode the encoded groups into their respective field elements
                    let record_field_element = Elligator2::<
                        <Components as BaseDPCComponents>::EncryptionModelParameters,
                        <Components as BaseDPCComponents>::EncryptionGroup,
                    >::decode(&element_affine, *fq_high)?;

                    record_field_elements.push(record_field_element);
                }

                // Fetch the x and y coordinates of the serialized group elements
                // These values will be used in the inner circuit to validate the Elligator2 encoding
                let x =
                    <<Components as BaseDPCComponents>::EncryptionModelParameters as ModelParameters>::BaseField::read(
                        &to_bytes![element_affine.to_x_coordinate()]?[..],
                    )?;
                let y =
                    <<Components as BaseDPCComponents>::EncryptionModelParameters as ModelParameters>::BaseField::read(
                        &to_bytes![element_affine.to_y_coordinate()]?[..],
                    )?;
                record_group_encoding.push((x, y));

                // Construct the plaintext element from the serialized group elements
                // This value will be used in the inner circuit to validate the encryption
                let plaintext_element =
                    <<Components as DPCComponents>::AccountEncryption as EncryptionScheme>::Text::read(
                        &to_bytes![element]?[..],
                    )?;
                record_plaintexts.push(plaintext_element);
            }

            // Store the field elements and group encodings for each new record
            new_records_field_elements.push(record_field_elements);
            new_records_group_encoding.push(record_group_encoding);

            // Encrypt the record plaintext
            let record_public_key = record.account_address().into_repr();
            let encryption_randomness = circuit_parameters
                .account_encryption
                .generate_randomness(record_public_key, rng)?;
            let encryption_blinding_exponents = circuit_parameters.account_encryption.generate_blinding_exponents(
                record_public_key,
                &encryption_randomness,
                record_plaintexts.len(),
            )?;
            let record_ciphertext = circuit_parameters.account_encryption.encrypt(
                record_public_key,
                &encryption_randomness,
                &record_plaintexts,
            )?;

            // Compose the record ciphertext for storage in a transaction
            let mut ciphertext = vec![];

            // Compute the ciphertext hash which will be validated in the inner circuit
            let mut ciphertext_affine_x = vec![];
            let mut ciphertext_selectors = vec![];
            for ciphertext_element in record_ciphertext.iter() {
                let ciphertext_element_affine =
                    <Components as BaseDPCComponents>::EncryptionGroup::read(&to_bytes![ciphertext_element]?[..])?
                        .into_affine();
                let ciphertext_x_coordinate = ciphertext_element_affine.to_x_coordinate();

                let greatest = match <<Components as BaseDPCComponents>::EncryptionGroup as ProjectiveCurve>::Affine::from_x_coordinate(
                    ciphertext_x_coordinate.clone(),
                    true,
                ) {
                    Some(affine) => ciphertext_element_affine == affine,
                    None => false,
                };

                ciphertext_affine_x.push(ciphertext_x_coordinate);
                ciphertext_selectors.push(greatest);
                ciphertext.push(ciphertext_element.clone());
            }

            // Concatenate the fq_high selector bits and plaintext decoding selector bit
            let selector_bits = [ciphertext_selectors.clone(), vec![final_fq_high_bit]].concat();
            let selector_bytes = bits_to_bytes(&selector_bits);

            let ciphertext_hash = circuit_parameters
                .record_ciphertext_crh
                .hash(&to_bytes![ciphertext_affine_x, selector_bytes]?)?;

            new_records_encryption_randomness.push(encryption_randomness);
            new_records_encryption_blinding_exponents.push(encryption_blinding_exponents);
            new_records_encryption_ciphertexts.push(ciphertext);
            new_records_ciphertext_hashes.push(ciphertext_hash);

            new_records_ciphertext_and_fq_high_selectors_gadget.push((ciphertext_selectors.clone(), fq_high_bits));
            new_records_ciphertext_and_fq_high_selectors.push((ciphertext_selectors, final_fq_high_bit));
        }

        let inner_proof = {
            let circuit = InnerCircuit::new(
                &parameters.circuit_parameters,
                ledger.parameters(),
                &ledger_digest,
                old_records,
                &old_witnesses,
                old_account_private_keys,
                &old_serial_numbers,
                &new_records,
                &new_sn_nonce_randomness,
                &new_commitments,
                &new_records_field_elements,
                &new_records_group_encoding,
                &new_records_encryption_randomness,
                &new_records_encryption_blinding_exponents,
                &new_records_ciphertext_and_fq_high_selectors_gadget,
                &new_records_ciphertext_hashes,
                &predicate_commitment,
                &predicate_randomness,
                &local_data_commitment,
                &local_data_commitment_randomizers,
                memorandum,
                &old_value_commits,
                &old_value_commit_randomness,
                &new_value_commits,
                &new_value_commit_randomness,
                value_balance,
                &binding_signature,
                network_id,
            );

            let inner_snark_parameters = match &parameters.inner_snark_parameters.0 {
                Some(inner_snark_parameters) => inner_snark_parameters,
                None => return Err(DPCError::MissingInnerSnarkProvingParameters),
            };

            Components::InnerSNARK::prove(&inner_snark_parameters, circuit, rng)?
        };

        // Verify that the inner proof passes
        {
            let input = InnerCircuitVerifierInput {
                circuit_parameters: parameters.circuit_parameters.clone(),
                ledger_parameters: ledger.parameters().clone(),
                ledger_digest: ledger_digest.clone(),
                old_serial_numbers: old_serial_numbers.clone(),
                new_commitments: new_commitments.clone(),
                new_records_ciphertext_hashes: new_records_ciphertext_hashes.clone(),
                memo: memorandum.clone(),
                predicate_commitment: predicate_commitment.clone(),
                local_data_commitment: local_data_commitment.clone(),
                value_balance,
                network_id,
            };

            let verification_key = &parameters.inner_snark_parameters.1;

            assert!(Components::InnerSNARK::verify(verification_key, &input, &inner_proof)?);
        }

        let transaction_proof = {
            let ledger_parameters = ledger.parameters();
            let inner_snark_vk: <Components::InnerSNARK as SNARK>::VerificationParameters =
                parameters.inner_snark_parameters.1.clone().into();

            let circuit = OuterCircuit::new(
                &parameters.circuit_parameters,
                ledger_parameters,
                &ledger_digest,
                &old_serial_numbers,
                &new_commitments,
                &new_records_ciphertext_hashes,
                &memorandum,
                value_balance,
                network_id,
                &inner_snark_vk,
                &inner_proof,
                old_death_pred_attributes.as_slice(),
                new_birth_pred_attributes.as_slice(),
                &predicate_commitment,
                &predicate_randomness,
                &local_data_commitment,
            );

            let outer_snark_parameters = match &parameters.outer_snark_parameters.0 {
                Some(outer_snark_parameters) => outer_snark_parameters,
                None => return Err(DPCError::MissingOuterSnarkProvingParameters),
            };

            Components::OuterSNARK::prove(&outer_snark_parameters, circuit, rng)?
        };

        assert_eq!(new_records_encryption_ciphertexts.len(), Components::NUM_OUTPUT_RECORDS);
        assert_eq!(
            new_records_encryption_ciphertexts.len(),
            new_records_ciphertext_and_fq_high_selectors.len()
        );

        let transaction = Self::Transaction::new(
            old_serial_numbers,
            new_commitments,
            memorandum.clone(),
            ledger_digest,
            transaction_proof,
            predicate_commitment,
            local_data_commitment,
            value_balance,
            network_id,
            signatures,
            new_records_encryption_ciphertexts,
            new_records_ciphertext_and_fq_high_selectors,
        );

        end_timer!(exec_time);

        Ok((new_records, transaction))
    }

    fn verify(parameters: &Self::Parameters, transaction: &Self::Transaction, ledger: &L) -> Result<bool, DPCError> {
        let verify_time = start_timer!(|| "BaseDPC::verify");

        // Returns false if there are duplicate serial numbers in the transaction.
        if has_duplicates(transaction.old_serial_numbers().iter()) {
            eprintln!("Transaction contains duplicate serial numbers");
            return Ok(false);
        }

        // Returns false if there are duplicate commitments numbers in the transaction.
        if has_duplicates(transaction.new_commitments().iter()) {
            eprintln!("Transaction contains duplicate commitments");
            return Ok(false);
        }

        let ledger_time = start_timer!(|| "Ledger checks");

        // Returns false if the transaction memo previously existed in the ledger.
        if ledger.contains_memo(transaction.memorandum()) {
            eprintln!("Ledger already contains this transaction memo.");
            return Ok(false);
        }

        // Returns false if any transaction serial number previously existed in the ledger.
        for sn in transaction.old_serial_numbers() {
            if ledger.contains_sn(sn) {
                eprintln!("Ledger already contains this transaction serial number.");
                return Ok(false);
            }
        }

        // Returns false if any transaction commitment previously existed in the ledger.
        for cm in transaction.new_commitments() {
            if ledger.contains_cm(cm) {
                eprintln!("Ledger already contains this transaction commitment.");
                return Ok(false);
            }
        }

        // Returns false if the ledger digest in the transaction is invalid.
        if !ledger.validate_digest(&transaction.ledger_digest) {
            eprintln!("Ledger digest is invalid.");
            return Ok(false);
        }

        end_timer!(ledger_time);

        let signature_time = start_timer!(|| "Signature checks");

        let signature_message = &to_bytes![
            transaction.network_id(),
            transaction.ledger_digest(),
            transaction.old_serial_numbers(),
            transaction.new_commitments(),
            transaction.predicate_commitment(),
            transaction.local_data_commitment(),
            transaction.value_balance(),
            transaction.memorandum()
        ]?;

        let account_signature = &parameters.circuit_parameters.account_signature;
        for (pk, sig) in transaction.old_serial_numbers().iter().zip(&transaction.signatures) {
            if !Components::AccountSignature::verify(account_signature, pk, signature_message, sig)? {
                eprintln!("Signature didn't verify.");
                return Ok(false);
            }
        }

        end_timer!(signature_time);

        let mut new_records_ciphertext_hashes = Vec::with_capacity(Components::NUM_OUTPUT_RECORDS);
        for (ciphertext, (encryption_selector_bits, final_fq_high_selector_bit)) in transaction
            .record_ciphertexts
            .iter()
            .zip_eq(&transaction.new_records_ciphertext_and_fq_high_selectors)
        {
            let mut ciphertext_affine_x = vec![];
            for ciphertext_element in ciphertext {
                // Convert the ciphertext group to the affine representation to be hashed
                let ciphertext_element_affine =
                    <Components as BaseDPCComponents>::EncryptionGroup::read(&to_bytes![ciphertext_element]?[..])?
                        .into_affine();
                ciphertext_affine_x.push(ciphertext_element_affine.to_x_coordinate());
            }

            let selector_bytes =
                bits_to_bytes(&[&encryption_selector_bits[..], &[*final_fq_high_selector_bit][..]].concat());

            let ciphertext_hash = parameters
                .circuit_parameters
                .record_ciphertext_crh
                .hash(&to_bytes![ciphertext_affine_x, selector_bytes]?)?;

            new_records_ciphertext_hashes.push(ciphertext_hash);
        }

        let inner_snark_input = InnerCircuitVerifierInput {
            circuit_parameters: parameters.circuit_parameters.clone(),
            ledger_parameters: ledger.parameters().clone(),
            ledger_digest: transaction.ledger_digest().clone(),
            old_serial_numbers: transaction.old_serial_numbers().to_vec(),
            new_commitments: transaction.new_commitments().to_vec(),
            new_records_ciphertext_hashes,
            memo: transaction.memorandum().clone(),
            predicate_commitment: transaction.predicate_commitment().clone(),
            local_data_commitment: transaction.local_data_commitment().clone(),
            value_balance: transaction.value_balance(),
            network_id: transaction.network_id(),
        };

        let outer_snark_input = OuterCircuitVerifierInput {
            inner_snark_verifier_input: inner_snark_input,
            predicate_commitment: transaction.predicate_commitment().clone(),
        };

        if !Components::OuterSNARK::verify(
            &parameters.outer_snark_parameters.1,
            &outer_snark_input,
            &transaction.transaction_proof,
        )? {
            eprintln!("Transaction proof failed to verify.");
            return Ok(false);
        }

        end_timer!(verify_time);

        Ok(true)
    }

    /// Returns true iff all the transactions in the block are valid according to the ledger.
    fn verify_transactions(
        parameters: &Self::Parameters,
        transactions: &Vec<Self::Transaction>,
        ledger: &L,
    ) -> Result<bool, DPCError> {
        for transaction in transactions {
            if !Self::verify(parameters, transaction, ledger)? {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
