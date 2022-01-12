use snarkos_algorithms::crh::sha256::sha256;
use snarkos_dpc::base_dpc::{
    instantiated::Components,
    outer_circuit::OuterCircuit,
    parameters::CircuitParameters,
    predicate::PrivatePredicateInput,
    predicate_circuit::PredicateCircuit,
    BaseDPCComponents,
    DPC,
};
use snarkos_errors::dpc::DPCError;
use snarkos_models::algorithms::SNARK;
use snarkos_utilities::{bytes::ToBytes, to_bytes};

use hex;
use rand::thread_rng;
use std::{
    fs::{self, File},
    io::{Result as IoResult, Write},
    path::PathBuf,
};

pub fn setup<C: BaseDPCComponents>() -> Result<(Vec<u8>, Vec<u8>), DPCError> {
    let rng = &mut thread_rng();
    let circuit_parameters = CircuitParameters::<C>::load()?;

    // TODO (howardwu): Check why is the PrivatePredicateInput necessary for running the setup? Blank should take option?
    let predicate_snark_parameters = DPC::<C>::generate_predicate_snark_parameters(&circuit_parameters, rng)?;
    let predicate_snark_proof = C::PredicateSNARK::prove(
        &predicate_snark_parameters.proving_key,
        PredicateCircuit::blank(&circuit_parameters),
        rng,
    )?;
    let private_predicate_input = PrivatePredicateInput {
        verification_key: predicate_snark_parameters.verification_key,
        proof: predicate_snark_proof,
    };

    let outer_snark_parameters =
        C::OuterSNARK::setup(OuterCircuit::blank(&circuit_parameters, &private_predicate_input), rng)?;
    let outer_snark_pk = to_bytes![outer_snark_parameters.0]?;
    let outer_snark_vk: <C::OuterSNARK as SNARK>::VerificationParameters = outer_snark_parameters.1.into();
    let outer_snark_vk = to_bytes![outer_snark_vk]?;

    println!("outer_snark_pk.params\n\tsize - {}", outer_snark_pk.len());
    println!("outer_snark_vk.params\n\tsize - {}", outer_snark_vk.len());
    Ok((outer_snark_pk, outer_snark_vk))
}

fn versioned_filename(checksum: &str) -> String {
    match checksum.get(0..7) {
        Some(sum) => format!("outer_snark_pk-{}.params", sum),
        _ => format!("outer_snark_pk.params"),
    }
}

fn store(file_path: &PathBuf, checksum_path: &PathBuf, bytes: &Vec<u8>) -> IoResult<()> {
    // Save checksum to file
    fs::write(checksum_path, hex::encode(sha256(bytes)))?;

    // Save buffer to file
    let mut file = File::create(file_path)?;
    file.write_all(&bytes)?;
    drop(file);
    Ok(())
}

pub fn main() {
    let (outer_snark_pk, outer_snark_vk) = setup::<Components>().unwrap();
    let outer_snark_pk_checksum = hex::encode(sha256(&outer_snark_pk));
    store(
        &PathBuf::from(&versioned_filename(&outer_snark_pk_checksum)),
        &PathBuf::from("outer_snark_pk.checksum"),
        &outer_snark_pk,
    )
    .unwrap();
    store(
        &PathBuf::from("outer_snark_vk.params"),
        &PathBuf::from("outer_snark_vk.checksum"),
        &outer_snark_vk,
    )
    .unwrap();
}
