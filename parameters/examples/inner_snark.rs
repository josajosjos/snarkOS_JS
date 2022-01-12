use snarkos_algorithms::merkle_tree::MerkleParameters;
use snarkos_dpc::base_dpc::{
    inner_circuit::InnerCircuit,
    instantiated::Components,
    parameters::CircuitParameters,
    BaseDPCComponents,
};
use snarkos_errors::dpc::DPCError;
use snarkos_models::{algorithms::SNARK, parameters::Parameters};
use snarkos_parameters::LedgerMerkleTreeParameters;
use snarkos_utilities::{
    bytes::{FromBytes, ToBytes},
    to_bytes,
};

use rand::thread_rng;
use std::{
    fs::File,
    io::{Result as IoResult, Write},
    path::PathBuf,
};

pub fn setup<C: BaseDPCComponents>() -> Result<(Vec<u8>, Vec<u8>), DPCError> {
    let rng = &mut thread_rng();

    // TODO (howardwu): Resolve this inconsistency on import structure with a new model once MerkleParameters are refactored.
    let merkle_tree_hash_parameters: <C::MerkleParameters as MerkleParameters>::H =
        From::from(FromBytes::read(&LedgerMerkleTreeParameters::load_bytes()?[..])?);
    let ledger_merkle_tree_parameters = From::from(merkle_tree_hash_parameters);

    let circuit_parameters = CircuitParameters::<C>::load()?;
    let inner_snark_parameters = C::InnerSNARK::setup(
        InnerCircuit::blank(&circuit_parameters, &ledger_merkle_tree_parameters),
        rng,
    )?;
    let inner_snark_pk = to_bytes![inner_snark_parameters.0]?;
    let inner_snark_vk: <C::InnerSNARK as SNARK>::VerificationParameters = inner_snark_parameters.1.into();
    let inner_snark_vk = to_bytes![inner_snark_vk]?;

    println!("inner_snark_pk.params\n\tsize - {}", inner_snark_pk.len());
    println!("inner_snark_vk.params\n\tsize - {}", inner_snark_vk.len());
    Ok((inner_snark_pk, inner_snark_vk))
}

pub fn store(path: &PathBuf, bytes: &Vec<u8>) -> IoResult<()> {
    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    drop(file);
    Ok(())
}

pub fn main() {
    let (inner_snark_pk, inner_snark_vk) = setup::<Components>().unwrap();
    store(&PathBuf::from("inner_snark_pk.params"), &inner_snark_pk).unwrap();
    store(&PathBuf::from("inner_snark_vk.params"), &inner_snark_vk).unwrap();
}
