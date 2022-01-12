use snarkos_dpc::base_dpc::{
    instantiated::*,
    program::{DummyProgram, NoopProgram},
    record::record_encryption::RecordEncryption,
    record_payload::RecordPayload,
    BaseDPCComponents,
    DPC,
};
use snarkos_models::{
    algorithms::CRH,
    dpc::{DPCScheme, Program},
    objects::{LedgerScheme, Transaction},
};
use snarkos_objects::{
    dpc::DPCTransactions,
    merkle_root,
    AccountViewKey,
    Block,
    BlockHeader,
    BlockHeaderHash,
    MerkleRootHash,
    PedersenMerkleRootHash,
    ProofOfSuccinctWork,
};
use snarkos_testing::{dpc::*, storage::*};
use snarkos_utilities::{
    bytes::{FromBytes, ToBytes},
    to_bytes,
};

use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use std::time::{SystemTime, UNIX_EPOCH};

type L = Ledger<Tx, CommitmentMerkleParameters>;

#[test]
fn base_dpc_integration_test() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    // Generate or load parameters for the ledger, commitment schemes, and CRH
    let (ledger_parameters, parameters) = setup_or_load_parameters(false, &mut rng);

    // Generate accounts
    let [genesis_account, recipient, _] = generate_test_accounts(&parameters, &mut rng);

    // Specify network_id
    let network_id: u8 = 0;

    // Create a genesis block

    let genesis_block = Block {
        header: BlockHeader {
            previous_block_hash: BlockHeaderHash([0u8; 32]),
            merkle_root_hash: MerkleRootHash([0u8; 32]),
            pedersen_merkle_root_hash: PedersenMerkleRootHash([0u8; 32]),
            time: 0,
            difficulty_target: 0x07FF_FFFF_FFFF_FFFF_u64,
            nonce: 0,
            proof: ProofOfSuccinctWork::default(),
        },
        transactions: DPCTransactions::new(),
    };

    let ledger = initialize_test_blockchain::<Tx, CommitmentMerkleParameters>(ledger_parameters, genesis_block);

    let dummy_program_id = to_bytes![
        ProgramVerificationKeyHash::hash(
            &parameters.system_parameters.program_verification_key_hash,
            &to_bytes![parameters.dummy_program_snark_parameters().verification_key].unwrap()
        )
        .unwrap()
    ]
    .unwrap();

    let noop_program_id = to_bytes![
        ProgramVerificationKeyHash::hash(
            &parameters.system_parameters.program_verification_key_hash,
            &to_bytes![parameters.noop_program_snark_parameters().verification_key].unwrap()
        )
        .unwrap()
    ]
    .unwrap();

    // Generate dummy input records having as address the genesis address.
    let old_account_private_keys = vec![genesis_account.private_key.clone(); NUM_INPUT_RECORDS];
    let mut old_records = vec![];
    for i in 0..NUM_INPUT_RECORDS {
        let old_sn_nonce = SerialNumberNonce::hash(
            &parameters.system_parameters.serial_number_nonce,
            &[64u8 + (i as u8); 1],
        )
        .unwrap();
        let old_record = DPC::generate_record(
            &parameters.system_parameters,
            &old_sn_nonce,
            &genesis_account.address,
            true, // The input record is dummy
            0,
            &RecordPayload::default(),
            &dummy_program_id,
            &dummy_program_id,
            &mut rng,
        )
        .unwrap();
        old_records.push(old_record);
    }

    // Construct new records.

    // Set the new records' program to be the "always-accept" program.
    let new_record_owners = vec![recipient.address.clone(); NUM_OUTPUT_RECORDS];
    let new_is_dummy_flags = vec![false; NUM_OUTPUT_RECORDS];
    let new_values = vec![10; NUM_OUTPUT_RECORDS];
    let new_payloads = vec![RecordPayload::default(); NUM_OUTPUT_RECORDS];
    let new_birth_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];
    let new_death_program_ids = vec![noop_program_id.clone(); NUM_OUTPUT_RECORDS];

    let memo = [4u8; 32];

    // Offline execution to generate a DPC transaction
    let execute_context = <InstantiatedDPC as DPCScheme<L>>::execute_offline(
        &parameters.system_parameters,
        &old_records,
        &old_account_private_keys,
        &new_record_owners,
        &new_is_dummy_flags,
        &new_values,
        &new_payloads,
        &new_birth_program_ids,
        &new_death_program_ids,
        &memo,
        network_id,
        &mut rng,
    )
    .unwrap();

    let local_data = execute_context.into_local_data();

    // Generate the program proofs

    let noop_program = NoopProgram::<_, <Components as BaseDPCComponents>::NoopProgramSNARK>::new(noop_program_id);
    let dummy_program = DummyProgram::<_, <Components as BaseDPCComponents>::DummyProgramSNARK>::new(dummy_program_id);

    let mut old_death_program_proofs = vec![];
    for i in 0..NUM_INPUT_RECORDS {
        let private_input = dummy_program
            .execute(
                &parameters.dummy_program_snark_parameters.proving_key,
                &parameters.dummy_program_snark_parameters.verification_key,
                &local_data,
                i as u8,
                &mut rng,
            )
            .unwrap();

        old_death_program_proofs.push(private_input);
    }

    let mut new_birth_program_proofs = vec![];
    for j in 0..NUM_OUTPUT_RECORDS {
        let private_input = noop_program
            .execute(
                &parameters.noop_program_snark_parameters.proving_key,
                &parameters.noop_program_snark_parameters.verification_key,
                &local_data,
                (NUM_INPUT_RECORDS + j) as u8,
                &mut rng,
            )
            .unwrap();

        new_birth_program_proofs.push(private_input);
    }

    let (new_records, transaction) = InstantiatedDPC::execute_online(
        &parameters,
        execute_context,
        &old_death_program_proofs,
        &new_birth_program_proofs,
        &ledger,
        &mut rng,
    )
    .unwrap();

    // Check that the transaction is serialized and deserialized correctly
    let transaction_bytes = to_bytes![transaction].unwrap();
    let recovered_transaction = Tx::read(&transaction_bytes[..]).unwrap();

    assert_eq!(transaction, recovered_transaction);

    {
        // Check that new_records can be decrypted from the transaction

        let encrypted_records = transaction.encrypted_records();
        let new_account_private_keys = vec![recipient.private_key.clone(); NUM_OUTPUT_RECORDS];

        for ((encrypted_record, private_key), new_record) in
            encrypted_records.iter().zip(new_account_private_keys).zip(new_records)
        {
            let account_view_key = AccountViewKey::from_private_key(
                &parameters.system_parameters.account_signature,
                &parameters.system_parameters.account_commitment,
                &private_key,
            )
            .unwrap();

            let decrypted_record =
                RecordEncryption::decrypt_record(&parameters.system_parameters, &account_view_key, encrypted_record)
                    .unwrap();

            assert_eq!(decrypted_record, new_record);
        }
    }

    // Craft the block

    let previous_block = ledger.get_latest_block().unwrap();

    let mut transactions = DPCTransactions::new();
    transactions.push(transaction);

    let transaction_ids: Vec<Vec<u8>> = transactions
        .to_transaction_ids()
        .unwrap()
        .iter()
        .map(|id| id.to_vec())
        .collect();

    let mut merkle_root_bytes = [0u8; 32];
    merkle_root_bytes[..].copy_from_slice(&merkle_root(&transaction_ids));

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;

    let header = BlockHeader {
        previous_block_hash: previous_block.header.get_hash(),
        merkle_root_hash: MerkleRootHash(merkle_root_bytes),
        time,
        difficulty_target: previous_block.header.difficulty_target,
        nonce: 0,
        pedersen_merkle_root_hash: PedersenMerkleRootHash([0u8; 32]),
        proof: ProofOfSuccinctWork::default(),
    };

    assert!(InstantiatedDPC::verify_transactions(&parameters, &transactions.0, &ledger).unwrap());

    let block = Block { header, transactions };

    ledger.insert_and_commit(&block).unwrap();
    assert_eq!(ledger.len(), 2);

    kill_storage(ledger);
}
