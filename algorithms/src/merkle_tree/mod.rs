pub mod merkle_parameters;
pub use self::merkle_parameters::*;

pub mod merkle_path;
pub use self::merkle_path::*;

pub mod merkle_tree;
pub use self::merkle_tree::*;

#[cfg(test)]
pub mod tests;

use rand::{Rng, SeedableRng};

// TODO: How should this seed be chosen?
const PRNG_SEED: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

// PRNG to instantiate the Merkle Tree parameters
pub fn prng() -> impl Rng {
    rand_chacha::ChaChaRng::from_seed(PRNG_SEED)
}

#[macro_export]
/// Defines a Merkle Tree using the provided hash and height.
macro_rules! define_merkle_tree_parameters {
    ($struct_name:ident, $hash:ty, $height:expr) => {
        use snarkos_models::algorithms::crh::CRH;
        use $crate::merkle_tree::{MerkleParameters, MerkleTree};

        use rand::Rng;

        #[derive(Clone, PartialEq, Eq)]
        pub struct $struct_name($hash);

        impl MerkleParameters for $struct_name {
            type H = $hash;

            const HEIGHT: usize = $height;

            fn setup<R: Rng>(rng: &mut R) -> Self {
                Self(Self::H::setup(rng))
            }

            fn crh(&self) -> &Self::H {
                &self.0
            }

            fn parameters(&self) -> &<Self::H as CRH>::Parameters {
                self.crh().parameters()
            }
        }

        impl Default for $struct_name {
            fn default() -> Self {
                Self(<Self as MerkleParameters>::H::setup(
                    &mut $crate::merkle_tree::prng(),
                ))
            }
        }

        impl From<$hash> for $struct_name {
            fn from(crh: $hash) -> Self {
                Self(crh)
            }
        }
    };
}
