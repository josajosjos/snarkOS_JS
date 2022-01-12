#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(test, not(feature = "std")))]
#[macro_use]
extern crate std;

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
#[doc(hidden)]
pub use alloc::{boxed::Box, format, vec, vec::Vec};

#[cfg(feature = "std")]
#[allow(unused_imports)]
#[doc(hidden)]
pub use std::{boxed::Box, format, vec, vec::Vec};

pub mod biginteger;
pub use self::biginteger::*;

pub mod bititerator;
pub use self::bititerator::*;

#[macro_use]
pub mod bytes;
pub use self::bytes::*;

pub mod error;
pub use self::error::*;

pub mod iterator;
pub use self::iterator::*;

pub mod math;
pub use self::math::*;

pub mod rand;
pub use self::rand::*;

pub mod serialize;
pub use self::serialize::*;

pub mod variable_length_integer;
pub use self::variable_length_integer::*;

#[cfg(not(feature = "std"))]
pub mod io;

#[cfg(feature = "std")]
pub use std::io;

#[cfg(not(feature = "std"))]
pub fn error(_msg: &'static str) -> io::Error {
    io::Error
}

#[cfg(feature = "std")]
pub fn error(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg)
}

#[macro_export]
macro_rules! unwrap_option_or_continue {
    ( $e:expr ) => {
        match $e {
            Some(x) => x,
            None => continue,
        }
    };
}

#[macro_export]
macro_rules! unwrap_result_or_continue {
    ( $e:expr ) => {
        match $e {
            Ok(x) => x,
            Err(_) => continue,
        }
    };
}

#[macro_export]
macro_rules! unwrap_option_or_error {
    ($e:expr; $err:expr) => {
        match $e {
            Some(val) => val,
            None => return Err($err),
        }
    };
}
