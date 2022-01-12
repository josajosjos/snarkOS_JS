use crate::{
    commitment::{PedersenCommitment, PedersenCommitmentParameters},
    crh::PedersenSize,
};
use snarkos_errors::algorithms::CommitmentError;
use snarkos_models::{
    algorithms::CommitmentScheme,
    curves::{AffineCurve, Group, ProjectiveCurve},
    storage::Storage,
};
use snarkos_utilities::bytes::{FromBytes, ToBytes};

use rand::Rng;
use std::{
    io::{Read, Result as IoResult, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PedersenCompressedCommitment<G: Group + ProjectiveCurve, S: PedersenSize> {
    pub parameters: PedersenCommitmentParameters<G, S>,
}

impl<G: Group + ProjectiveCurve, S: PedersenSize> CommitmentScheme for PedersenCompressedCommitment<G, S> {
    type Output = <G::Affine as AffineCurve>::BaseField;
    type Parameters = PedersenCommitmentParameters<G, S>;
    type Randomness = <G as Group>::ScalarField;

    fn setup<R: Rng>(rng: &mut R) -> Self {
        Self {
            parameters: PedersenCommitmentParameters::new(rng),
        }
    }

    /// Returns the affine x-coordinate as the commitment.
    fn commit(&self, input: &[u8], randomness: &Self::Randomness) -> Result<Self::Output, CommitmentError> {
        let commitment = PedersenCommitment::<G, S> {
            parameters: self.parameters.clone(),
        };

        let output = commitment.commit(input, randomness)?;
        let affine = output.into_affine();
        debug_assert!(affine.is_in_correct_subgroup_assuming_on_curve());
        Ok(affine.to_x_coordinate())
    }

    fn parameters(&self) -> &Self::Parameters {
        &self.parameters
    }
}

impl<G: Group + ProjectiveCurve, S: PedersenSize> Storage for PedersenCompressedCommitment<G, S> {
    /// Store the Pedersen compressed commitment parameters to a file at the given path.
    fn store(&self, path: &PathBuf) -> IoResult<()> {
        self.parameters.store(path)?;
        Ok(())
    }

    /// Load the Pedersen compressed commitment parameters from a file at the given path.
    fn load(path: &PathBuf) -> IoResult<Self> {
        let parameters = PedersenCommitmentParameters::<G, S>::load(path)?;

        Ok(Self { parameters })
    }
}

impl<G: Group + ProjectiveCurve, S: PedersenSize> ToBytes for PedersenCompressedCommitment<G, S> {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.parameters.write(&mut writer)
    }
}

impl<G: Group + ProjectiveCurve, S: PedersenSize> FromBytes for PedersenCompressedCommitment<G, S> {
    #[inline]
    fn read<R: Read>(mut reader: R) -> IoResult<Self> {
        let parameters: PedersenCommitmentParameters<G, S> = FromBytes::read(&mut reader)?;

        Ok(Self { parameters })
    }
}
