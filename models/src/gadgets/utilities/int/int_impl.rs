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

use crate::gadgets::utilities::boolean::Boolean;

use std::fmt::Debug;

pub trait Int: Debug + Clone {
    type IntegerType;
    const SIZE: usize;

    fn one() -> Self;

    fn zero() -> Self;

    /// Returns true if all bits in this `Int` are constant
    fn is_constant(&self) -> bool;

    /// Returns true if both `Int` objects have constant bits
    fn result_is_constant(first: &Self, second: &Self) -> bool {
        first.is_constant() && second.is_constant()
    }
}

/// Implements the base struct for a signed integer gadget
macro_rules! int_impl {
    ($name: ident, $type_: ty, $size: expr) => {
        #[derive(Clone, Debug)]
        pub struct $name {
            pub bits: Vec<Boolean>,
            pub value: Option<$type_>,
        }

        impl $name {
            pub fn constant(value: $type_) -> Self {
                let mut bits = Vec::with_capacity($size);

                for i in 0..$size {
                    // shift value by i
                    let mask = 1 << i as $type_;
                    let result = value & mask;

                    // If last bit is one, push one.
                    if result == mask {
                        bits.push(Boolean::constant(true))
                    } else {
                        bits.push(Boolean::constant(false))
                    }
                }

                Self {
                    bits,
                    value: Some(value),
                }
            }
        }

        impl Int for $name {
            type IntegerType = $type_;

            const SIZE: usize = $size;

            fn one() -> Self {
                Self::constant(1 as $type_)
            }

            fn zero() -> Self {
                Self::constant(0 as $type_)
            }

            fn is_constant(&self) -> bool {
                let mut constant = true;

                // If any bits of self are allocated bits, return false
                for bit in &self.bits {
                    match *bit {
                        Boolean::Is(ref _bit) => constant = false,
                        Boolean::Not(ref _bit) => constant = false,
                        Boolean::Constant(_bit) => {}
                    }
                }

                constant
            }
        }
    };
}

int_impl!(Int64, i64, 64);
