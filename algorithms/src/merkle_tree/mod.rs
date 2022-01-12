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
/// Defines a Merkle tree using the provided hash and depth.
macro_rules! define_merkle_tree_parameters {
    ($struct_name:ident, $hash:ty, $depth:expr) => {
        #[allow(unused_imports)]
        use snarkos_models::algorithms::{LoadableMerkleParameters, MaskedMerkleParameters, MerkleParameters, CRH};
        #[allow(unused_imports)]
        use $crate::merkle_tree::MerkleTree;

        #[allow(unused_imports)]
        use rand::Rng;

        #[derive(Clone, PartialEq, Eq, Debug)]
        pub struct $struct_name($hash);

        impl MerkleParameters for $struct_name {
            type H = $hash;

            const DEPTH: usize = $depth;

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

        impl From<$hash> for $struct_name {
            fn from(crh: $hash) -> Self {
                Self(crh)
            }
        }

        impl LoadableMerkleParameters for $struct_name {}

        impl Default for $struct_name {
            fn default() -> Self {
                Self(<Self as MerkleParameters>::H::setup(
                    &mut $crate::merkle_tree::prng(),
                ))
            }
        }
    };
}

#[macro_export]
macro_rules! define_masked_merkle_tree_parameters {
    ($struct_name:ident, $hash:ty, $depth:expr) => {
        #[allow(unused_imports)]
        use snarkos_models::algorithms::{CRHParameters, MaskedMerkleParameters, MerkleParameters, CRH};
        #[allow(unused_imports)]
        use $crate::merkle_tree::MerkleTree;

        #[allow(unused_imports)]
        use rand::Rng;

        #[derive(Clone, PartialEq, Eq, Debug)]
        pub struct $struct_name($hash, <$hash as CRH>::Parameters);

        impl MerkleParameters for $struct_name {
            type H = $hash;

            const DEPTH: usize = $depth;

            fn setup<R: Rng>(rng: &mut R) -> Self {
                Self(Self::H::setup(rng), <Self::H as CRH>::Parameters::setup(rng))
            }

            fn crh(&self) -> &Self::H {
                &self.0
            }

            fn parameters(&self) -> &<Self::H as CRH>::Parameters {
                self.crh().parameters()
            }
        }

        impl MaskedMerkleParameters for $struct_name {
            fn mask_parameters(&self) -> &<Self::H as CRH>::Parameters {
                &self.1
            }
        }

        impl Default for $struct_name {
            fn default() -> Self {
                Self(
                    <Self as MerkleParameters>::H::setup(&mut $crate::merkle_tree::prng()),
                    <<Self as MerkleParameters>::H as CRH>::Parameters::setup(&mut $crate::merkle_tree::prng()),
                )
            }
        }
    };
}
