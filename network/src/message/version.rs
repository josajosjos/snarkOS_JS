// Copyright (C) 2019-2021 Aleo Systems Inc.
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

// todo: readd in Rust 1.54
// #[cfg_attr(nightly, doc(include = "../../documentation/network_messages/version.md"))]
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Version {
    /// The version number of the sender's node server.
    pub version: u64,
    /// The listening port of the sender.
    pub listening_port: u16,
    /// The node id of the sender.
    pub node_id: u64,
}

impl Version {
    pub fn new(version: u64, listening_port: u16, node_id: u64) -> Self {
        Self {
            version,
            listening_port,
            node_id,
        }
    }
}
