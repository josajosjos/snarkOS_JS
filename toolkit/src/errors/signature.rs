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

#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("{}: {}", _0, _1)]
    Crate(&'static str, String),
}

impl From<hex::FromHexError> for SignatureError {
    fn from(error: hex::FromHexError) -> Self {
        SignatureError::Crate("hex", format!("{:?}", error))
    }
}

impl From<snarkvm_errors::objects::account::AccountError> for SignatureError {
    fn from(error: snarkvm_errors::objects::account::AccountError) -> Self {
        SignatureError::Crate("snarkvm_errors::objects::account", format!("{:?}", error))
    }
}

impl From<snarkvm_errors::algorithms::signature::SignatureError> for SignatureError {
    fn from(error: snarkvm_errors::algorithms::signature::SignatureError) -> Self {
        SignatureError::Crate("snarkvm_errors::algorithms::signature", format!("{:?}", error))
    }
}

impl From<std::io::Error> for SignatureError {
    fn from(error: std::io::Error) -> Self {
        SignatureError::Crate("std::io", format!("{:?}", error))
    }
}
