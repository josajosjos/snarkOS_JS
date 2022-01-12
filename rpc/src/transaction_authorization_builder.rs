// Copyright (C) 2019-2021 Aleo Systems Inc.
// This file is part of the snarkOS library.

// The snarkOS library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkOS library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkOS library. If not, see <https://www.gnu.org/licenses/>.

use snarkvm::{
    dpc::{
        testnet1::{Testnet1DPC, Testnet1Parameters},
        Address,
        DPCScheme,
        Parameters,
        PrivateKey,
        Record,
        RecordScheme,
        TransactionAuthorization as TransactionAuthorizationNative,
        *,
    },
    utilities::{to_bytes_le, ToBytes},
};

use rand::{CryptoRng, Rng};
use std::{fmt, ops::Deref, str::FromStr, sync::Arc};

#[derive(Clone, Debug)]
pub struct TransactionInput {
    pub(crate) private_key: PrivateKey<Testnet1Parameters>,
    pub(crate) record: Record<Testnet1Parameters>,
}

#[derive(Clone, Debug)]
pub struct TransactionOutput {
    pub(crate) recipient: Address<Testnet1Parameters>,
    pub(crate) amount: u64,
    // TODO (raychu86): Add support for payloads and birth/death program ids.
    // pub(crate) payload: Option<Vec<u8>>,
}

pub struct TransactionAuthorization {
    pub(crate) authorization: TransactionAuthorizationNative<Testnet1Parameters>,
}

impl TransactionAuthorization {
    /// Returns an offline transaction authorization
    pub(crate) fn new<R: Rng + CryptoRng>(
        spenders: Vec<PrivateKey<Testnet1Parameters>>,
        records_to_spend: Vec<Record<Testnet1Parameters>>,
        recipients: Vec<Address<Testnet1Parameters>>,
        recipient_amounts: Vec<u64>,
        _network_id: u8, // TODO (howardwu): Keep this around to use for network modularization.
        memo: Option<[u8; 64]>,
        rng: &mut R,
    ) -> Result<Self, DPCError> {
        let dpc = <Testnet1DPC as DPCScheme<Testnet1Parameters>>::load(false).unwrap();

        assert!(!spenders.is_empty());
        assert_eq!(spenders.len(), records_to_spend.len());

        assert!(!recipients.is_empty());
        assert_eq!(recipients.len(), recipient_amounts.len());

        // Construct the new records
        let mut input_records = vec![];
        for record in records_to_spend {
            input_records.push(record);
        }

        let mut private_keys = vec![];
        for private_key in spenders {
            private_keys.push(private_key);
        }

        while input_records.len() < Testnet1Parameters::NUM_INPUT_RECORDS {
            let private_key = private_keys[0].clone();
            let address = Address::<Testnet1Parameters>::from_private_key(&private_key)?;

            input_records.push(Record::<Testnet1Parameters>::new_noop_input(
                dpc.noop_program.deref(),
                address,
                rng,
            )?);
            private_keys.push(private_key);
        }

        assert_eq!(input_records.len(), Testnet1Parameters::NUM_INPUT_RECORDS);

        // Enforce that the old record addresses correspond with the private keys
        for (private_key, record) in private_keys.iter().zip(&input_records) {
            let address = Address::<Testnet1Parameters>::from_private_key(private_key)?;
            assert_eq!(address, record.owner());
        }

        assert_eq!(input_records.len(), Testnet1Parameters::NUM_INPUT_RECORDS);
        assert_eq!(private_keys.len(), Testnet1Parameters::NUM_INPUT_RECORDS);

        // Decode new recipient data
        let mut new_record_owners = vec![];
        let mut new_values = vec![];
        for (recipient, amount) in recipients.iter().zip(recipient_amounts) {
            new_record_owners.push(recipient.clone());
            new_values.push(amount);
        }

        // Fill any unused new_record indices with dummy output values
        while new_record_owners.len() < Testnet1Parameters::NUM_OUTPUT_RECORDS {
            new_record_owners.push(new_record_owners[0].clone());
            new_values.push(0);
        }

        assert_eq!(new_record_owners.len(), Testnet1Parameters::NUM_OUTPUT_RECORDS);
        assert_eq!(new_values.len(), Testnet1Parameters::NUM_OUTPUT_RECORDS);

        let mut joint_serial_numbers = vec![];
        for i in 0..Testnet1Parameters::NUM_INPUT_RECORDS {
            let (sn, _) = input_records[i].to_serial_number(private_keys[i].compute_key())?;
            joint_serial_numbers.extend_from_slice(&to_bytes_le![sn]?);
        }

        let mut output_records = vec![];
        for j in 0..Testnet1Parameters::NUM_OUTPUT_RECORDS {
            output_records.push(Record::new_output(
                dpc.noop_program.deref(),
                new_record_owners[j],
                true,
                new_values[j],
                Default::default(),
                (Testnet1Parameters::NUM_OUTPUT_RECORDS + j) as u8,
                &joint_serial_numbers,
                rng,
            )?);
        }

        // TODO (raychu86): Genericize this model to allow for generic programs.
        let noop = Arc::new(dpc.noop_program.clone());

        let mut builder = StateTransition::builder();

        for (private_key, input_record) in private_keys.iter().zip(input_records.iter()) {
            builder = builder.add_input(Input::new(
                private_key.compute_key(),
                input_record.clone(),
                None,
                noop.clone(),
            )?);
        }

        for output_record in output_records.iter() {
            builder = builder.add_output(Output::new(
                output_record.owner(),
                AleoAmount::from_bytes(output_record.value() as i64),
                output_record.payload().clone(),
                None,
                noop.clone(),
            )?);
        }

        match memo {
            Some(memo) => builder = builder.append_memo(&memo.to_vec()),
            None => (),
        };

        let state = builder.build(noop.clone(), rng)?;

        // Offline execution to generate a transaction authorization.
        let authorization = dpc.authorize::<R>(&private_keys, &state, rng)?;

        Ok(Self { authorization })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut output = vec![];
        self.authorization
            .write_le(&mut output)
            .expect("serialization to bytes failed");
        output
    }
}

impl FromStr for TransactionAuthorization {
    type Err = DPCError;

    fn from_str(transaction_authorization: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            authorization: TransactionAuthorizationNative::<Testnet1Parameters>::from_str(transaction_authorization)?,
        })
    }
}

impl fmt::Display for TransactionAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.authorization.to_string())
    }
}

// TODO (raychu86) Look into genericizing this model into `dpc`.
#[derive(Clone, Debug, Default)]
pub struct TransactionAuthorizationBuilder {
    /// Transaction inputs
    pub(crate) inputs: Vec<TransactionInput>,
    /// Transaction outputs
    pub(crate) outputs: Vec<TransactionOutput>,
    /// Network ID
    pub(crate) network_id: u8,
    /// Transaction memo
    pub(crate) memo: Option<[u8; 64]>,
}

impl TransactionAuthorizationBuilder {
    pub fn new() -> Self {
        // TODO (raychu86) update the default to `0` for mainnet.
        Self {
            inputs: vec![],
            outputs: vec![],
            network_id: Testnet1Parameters::NETWORK_ID,
            memo: None,
        }
    }

    ///
    /// Returns a new transaction builder with the added transaction input.
    /// Otherwise, returns a `DPCError`.
    ///
    pub fn add_input(
        self,
        private_key: PrivateKey<Testnet1Parameters>,
        record: Record<Testnet1Parameters>,
    ) -> Result<Self, DPCError> {
        // Check that the transaction is limited to `Testnet1Parameters::NUM_INPUT_RECORDS` inputs.
        if self.inputs.len() > Testnet1Parameters::NUM_INPUT_RECORDS {
            return Err(DPCError::InvalidNumberOfInputs(
                self.inputs.len() + 1,
                Testnet1Parameters::NUM_INPUT_RECORDS,
            ));
        }

        // Construct the transaction input.
        let input = TransactionInput { private_key, record };

        // Update the current builder instance.
        let mut builder = self;
        builder.inputs.push(input);

        Ok(builder)
    }

    ///
    /// Returns a new transaction builder with the added transaction output.
    /// Otherwise, returns a `DPCError`.
    ///
    pub fn add_output(self, recipient: Address<Testnet1Parameters>, amount: u64) -> Result<Self, DPCError> {
        // Check that the transaction is limited to `Testnet1Parameters::NUM_OUTPUT_RECORDS` outputs.
        if self.outputs.len() > Testnet1Parameters::NUM_OUTPUT_RECORDS {
            return Err(DPCError::InvalidNumberOfOutputs(
                self.outputs.len() + 1,
                Testnet1Parameters::NUM_OUTPUT_RECORDS,
            ));
        }

        // Construct the transaction output.
        let output = TransactionOutput { recipient, amount };

        // Update the current builder instance.
        let mut builder = self;
        builder.outputs.push(output);

        Ok(builder)
    }

    ///
    /// Returns a new transaction builder with the updated network id.
    ///
    pub fn network_id(self, network_id: u8) -> Self {
        let mut builder = self;
        builder.network_id = network_id;
        builder
    }

    ///
    /// Returns a new transaction builder with the updated network id.
    ///
    pub fn memo(self, memo: [u8; 64]) -> Self {
        let mut builder = self;
        builder.memo = Some(memo);
        builder
    }

    ///
    /// Returns the transaction authorization derived from the provided builder
    /// attributes.
    ///
    /// Otherwise, returns `DPCError`.
    ///
    pub fn build<R: Rng + CryptoRng>(&self, rng: &mut R) -> Result<TransactionAuthorization, DPCError> {
        // Check that the transaction is limited to `Testnet1Parameters::NUM_INPUT_RECORDS` inputs.
        match self.inputs.len() {
            1 | 2 => {}
            num_inputs => {
                return Err(DPCError::InvalidNumberOfInputs(
                    num_inputs,
                    Testnet1Parameters::NUM_INPUT_RECORDS,
                ));
            }
        }

        // Check that the transaction has at least one output and is limited to `Testnet1Parameters::NUM_OUTPUT_RECORDS` outputs.
        match self.outputs.len() {
            0 => {
                return Err(DPCError::Message(
                    "Transaction authorization is missing outputs".to_string(),
                ));
            }
            1 | 2 => {}
            num_inputs => {
                return Err(DPCError::InvalidNumberOfInputs(
                    num_inputs,
                    Testnet1Parameters::NUM_INPUT_RECORDS,
                ));
            }
        }

        // Construct the parameters from the given transaction inputs.
        let mut spenders = vec![];
        let mut records_to_spend = vec![];

        for input in &self.inputs {
            spenders.push(input.private_key.clone());
            records_to_spend.push(input.record.clone());
        }

        // Construct the parameters from the given transaction outputs.
        let mut recipients = vec![];
        let mut recipient_amounts = vec![];

        for output in &self.outputs {
            recipients.push(output.recipient.clone());
            recipient_amounts.push(output.amount);
        }

        // Construct the transaction authorization
        TransactionAuthorization::new(
            spenders,
            records_to_spend,
            recipients,
            recipient_amounts,
            self.network_id,
            self.memo,
            rng,
        )
    }
}
