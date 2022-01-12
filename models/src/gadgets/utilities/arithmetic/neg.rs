// Copyright (C) 2019-2020 Aleo Systems Inc.
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

use crate::{
    curves::Field,
    gadgets::{
        r1cs::ConstraintSystem,
        utilities::{bits::RippleCarryAdder, boolean::Boolean},
    },
};
use snarkos_errors::gadgets::SynthesisError;

/// Returns a negated representation of `self` in the constraint system.
pub trait Neg<F: Field>
where
    Self: std::marker::Sized,
{
    type ErrorType;

    #[must_use]
    fn neg<CS: ConstraintSystem<F>>(&self, cs: CS) -> Result<Self, Self::ErrorType>;
}

impl<F: Field> Neg<F> for Vec<Boolean> {
    type ErrorType = SynthesisError;

    fn neg<CS: ConstraintSystem<F>>(&self, mut cs: CS) -> Result<Self, SynthesisError> {
        // flip all bits
        let flipped: Self = self.iter().map(|bit| bit.not()).collect();

        // add one
        let mut one = vec![Boolean::constant(true)];
        one.append(&mut vec![Boolean::Constant(false); self.len() - 1]);

        let mut bits = flipped.add_bits(cs.ns(|| format!("add one")), &one)?;
        let _carry = bits.pop(); // we already accounted for overflow above

        Ok(bits)
    }
}
