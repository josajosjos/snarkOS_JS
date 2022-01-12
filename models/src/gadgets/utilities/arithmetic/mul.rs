use crate::{curves::Field, gadgets::r1cs::ConstraintSystem};

/// Returns multiplication of `self` * `other` in the constraint system.
pub trait Mul<F: Field, Rhs = Self>
where
    Self: std::marker::Sized,
{
    type ErrorType;

    #[must_use]
    fn mul<CS: ConstraintSystem<F>>(&self, cs: CS, other: &Self) -> Result<Self, Self::ErrorType>;
}
