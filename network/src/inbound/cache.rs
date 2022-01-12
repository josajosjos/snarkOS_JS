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

use crate::Payload;

use circular_queue::CircularQueue;
use fxhash::hash64;

pub struct Cache {
    queue: CircularQueue<u64>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            queue: CircularQueue::with_capacity(64),
        }
    }
}

impl Cache {
    pub fn contains(&mut self, payload: &Payload) -> bool {
        let hash = if let Payload::Block(bytes) = payload {
            hash64(&bytes)
        } else {
            unreachable!("Only blocks are cached for now");
        };

        if self.queue.iter().any(|&e| e == hash) {
            true
        } else {
            self.queue.push(hash);
            false
        }
    }
}
