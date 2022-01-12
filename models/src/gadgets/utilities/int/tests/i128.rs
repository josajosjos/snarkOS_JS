use crate::{
    curves::{One, Zero},
    gadgets::{
        r1cs::{ConstraintSystem, Fr, TestConstraintSystem},
        utilities::{alloc::AllocGadget, arithmetic::Add, boolean::Boolean, int::Int128},
    },
};

use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

fn check_all_constant_bits(mut expected: i128, actual: Int128) {
    for b in actual.bits.iter() {
        match b {
            &Boolean::Is(_) => panic!(),
            &Boolean::Not(_) => panic!(),
            &Boolean::Constant(b) => {
                assert!(b == (expected & 1 == 1));
            }
        }

        expected >>= 1;
    }
}

fn check_all_allocated_bits(mut expected: i128, actual: Int128) {
    for b in actual.bits.iter() {
        match b {
            &Boolean::Is(ref b) => {
                assert!(b.get_value().unwrap() == (expected & 1 == 1));
            }
            &Boolean::Not(ref b) => {
                assert!(!b.get_value().unwrap() == (expected & 1 == 1));
            }
            &Boolean::Constant(_) => unreachable!(),
        }

        expected >>= 1;
    }
}

#[test]
fn test_int128_add_constants() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    for _ in 0..1000 {
        let mut cs = TestConstraintSystem::<Fr>::new();

        let a: i128 = rng.gen();
        let b: i128 = rng.gen();

        let a_bit = Int128::constant(a);
        let b_bit = Int128::constant(b);

        let expected = match a.checked_add(b) {
            Some(valid) => valid,
            None => continue,
        };

        let r = a_bit.add(cs.ns(|| "addition"), &b_bit).unwrap();

        assert!(r.value == Some(expected));

        check_all_constant_bits(expected, r);
    }
}

#[test]
fn test_int128_add() {
    let mut rng = XorShiftRng::seed_from_u64(1231275789u64);

    for _ in 0..100 {
        let mut cs = TestConstraintSystem::<Fr>::new();

        let a: i128 = rng.gen();
        let b: i128 = rng.gen();

        let expected = match a.checked_add(b) {
            Some(valid) => valid,
            None => continue,
        };

        let a_bit = Int128::alloc(cs.ns(|| "a_bit"), || Ok(a)).unwrap();
        let b_bit = Int128::alloc(cs.ns(|| "b_bit"), || Ok(b)).unwrap();

        let r = a_bit.add(cs.ns(|| "addition"), &b_bit).unwrap();

        assert!(cs.is_satisfied());

        assert!(r.value == Some(expected));

        check_all_allocated_bits(expected, r);

        // Flip a bit_gadget and see if the addition constraint still works
        if cs.get("addition/result bit_gadget 0/boolean").is_zero() {
            cs.set("addition/result bit_gadget 0/boolean", Fr::one());
        } else {
            cs.set("addition/result bit_gadget 0/boolean", Fr::zero());
        }

        assert!(!cs.is_satisfied());
    }
}
