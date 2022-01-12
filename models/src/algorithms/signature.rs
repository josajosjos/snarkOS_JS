use snarkos_errors::algorithms::SignatureError;
use snarkos_utilities::bytes::ToBytes;

use rand::Rng;
use std::hash::Hash;

pub trait SignatureScheme {
    type Parameters: Clone + Send + Sync;
    type PublicKey: ToBytes + Hash + Eq + Clone + Default + Send + Sync;
    type PrivateKey: ToBytes + Clone + Default;
    type Output: ToBytes + Clone + Default + Send + Sync;

    fn setup<R: Rng>(rng: &mut R) -> Result<Self::Parameters, SignatureError>;

    fn keygen<R: Rng>(
        public_parameters: &Self::Parameters,
        rng: &mut R,
    ) -> Result<(Self::PublicKey, Self::PrivateKey), SignatureError>;

    fn sign<R: Rng>(
        public_parameters: &Self::Parameters,
        private_key: &Self::PrivateKey,
        message: &[u8],
        rng: &mut R,
    ) -> Result<Self::Output, SignatureError>;

    fn verify(
        public_parameters: &Self::Parameters,
        public_key: &Self::PublicKey,
        message: &[u8],
        signature: &Self::Output,
    ) -> Result<bool, SignatureError>;

    fn randomize_public_key(
        public_parameters: &Self::Parameters,
        public_key: &Self::PublicKey,
        randomness: &[u8],
    ) -> Result<Self::PublicKey, SignatureError>;

    fn randomize_signature(
        public_parameters: &Self::Parameters,
        signature: &Self::Output,
        randomness: &[u8],
    ) -> Result<Self::Output, SignatureError>;
}
