use crate::account::{Address, PrivateKey};

use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use std::str::FromStr;

#[test]
pub fn private_key_test() {
    let rng = &mut ChaChaRng::seed_from_u64(1231275789u64);
    let private_key = PrivateKey::new(rng);
    assert!(private_key.is_ok());

    let expected_private_key = "AKEY1hES1RbMfbcybzaiwFYm7JnY1D1xNqaYF5vWPK5ejf2Nm";
    let candidate_private_key = private_key.unwrap().to_string();

    println!("{} == {}", expected_private_key, candidate_private_key);
    assert_eq!(expected_private_key, candidate_private_key);
}

#[test]
pub fn address_test() {
    let private_key = PrivateKey::from_str("AKEY1hES1RbMfbcybzaiwFYm7JnY1D1xNqaYF5vWPK5ejf2Nm").unwrap();
    let address = Address::from(&private_key);
    assert!(address.is_ok());

    let expected_address = "aleo1p3nt2dk5w4hf007ruc88nxa5amnhufrm6lcet255a93ktw9905yqqeu4rg";
    let candidate_address = address.unwrap().to_string();

    println!("{} == {}", expected_address, candidate_address);
    assert_eq!(expected_address, candidate_address);
}
