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

use crate::Environment;
use snarkos_ledger::BlockLocators;
use snarkvm::prelude::*;

use ::bytes::{Buf, BytesMut};
use anyhow::{anyhow, Result};
use std::{marker::PhantomData, net::SocketAddr};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Clone, Debug)]
pub enum Message<N: Network, E: Environment> {
    /// BlockRequest := (start_block_height, end_block_height (inclusive))
    BlockRequest(u32, u32),
    /// BlockResponse := (block)
    BlockResponse(Block<N>),
    /// ChallengeRequest := (listener_port, block_height)
    ChallengeRequest(u16, u32),
    /// ChallengeResponse := (block_header)
    ChallengeResponse(BlockHeader<N>),
    /// Disconnect := ()
    Disconnect,
    /// PeerRequest := ()
    PeerRequest,
    /// PeerResponse := (\[peer_ip\])
    PeerResponse(Vec<SocketAddr>),
    /// Ping := (version)
    Ping(u32),
    /// Pong := (block_locators)
    Pong(BlockLocators<N>),
    /// UnconfirmedBlock := (block)
    UnconfirmedBlock(Block<N>),
    /// UnconfirmedTransaction := (transaction)
    UnconfirmedTransaction(Transaction<N>),
    /// Unused
    Unused(PhantomData<E>),
}

impl<N: Network, E: Environment> Message<N, E> {
    /// Returns the message name.
    #[inline]
    pub fn name(&self) -> &str {
        match self {
            Self::BlockRequest(..) => "BlockRequest",
            Self::BlockResponse(..) => "BlockResponse",
            Self::ChallengeRequest(..) => "ChallengeRequest",
            Self::ChallengeResponse(..) => "ChallengeResponse",
            Self::Disconnect => "Disconnect",
            Self::PeerRequest => "PeerRequest",
            Self::PeerResponse(..) => "PeerResponse",
            Self::Ping(..) => "Ping",
            Self::Pong(..) => "Pong",
            Self::UnconfirmedBlock(..) => "UnconfirmedBlock",
            Self::UnconfirmedTransaction(..) => "UnconfirmedTransaction",
            Self::Unused(..) => "Unused",
        }
    }

    /// Returns the message ID.
    #[inline]
    pub fn id(&self) -> u16 {
        match self {
            Self::BlockRequest(..) => 0,
            Self::BlockResponse(..) => 1,
            Self::ChallengeRequest(..) => 2,
            Self::ChallengeResponse(..) => 3,
            Self::Disconnect => 4,
            Self::PeerRequest => 5,
            Self::PeerResponse(..) => 6,
            Self::Ping(..) => 7,
            Self::Pong(..) => 8,
            Self::UnconfirmedBlock(..) => 9,
            Self::UnconfirmedTransaction(..) => 10,
            Self::Unused(..) => 11,
        }
    }

    /// Returns the message data as bytes.
    #[inline]
    pub fn data(&self) -> Result<Vec<u8>> {
        match self {
            Self::BlockRequest(start_block_height, end_block_height) => Ok(to_bytes_le![start_block_height, end_block_height]?),
            Self::BlockResponse(block) => Ok(bincode::serialize(block)?),
            Self::ChallengeRequest(listener_port, block_height) => Ok(to_bytes_le![listener_port, block_height]?),
            Self::ChallengeResponse(block_header) => Ok(bincode::serialize(block_header)?),
            Self::Disconnect => Ok(vec![]),
            Self::PeerRequest => Ok(vec![]),
            Self::PeerResponse(peer_ips) => Ok(bincode::serialize(peer_ips)?),
            Self::Ping(version) => Ok(bincode::serialize(version)?),
            Self::Pong(block_locators) => Ok(bincode::serialize(block_locators)?),
            Self::UnconfirmedBlock(block) => Ok(bincode::serialize(block)?),
            Self::UnconfirmedTransaction(transaction) => Ok(bincode::serialize(transaction)?),
            Self::Unused(_) => Ok(vec![]),
        }
    }

    /// Serializes the given message into bytes.
    #[inline]
    pub fn serialize(&self) -> Result<Vec<u8>> {
        Ok([self.id().to_le_bytes().to_vec(), self.data()?].concat())
    }

    /// Deserializes the given buffer into a message.
    #[inline]
    pub fn deserialize(buffer: &[u8]) -> Result<Self> {
        // Ensure the buffer contains at least the length of an ID.
        if buffer.len() < 2 {
            return Err(anyhow!("Invalid message buffer"));
        }

        // Split the buffer into the ID and data portion.
        let (id, data) = (u16::from_le_bytes([buffer[0], buffer[1]]), &buffer[2..]);

        // Deserialize the data field.
        let message = match id {
            0 => Self::BlockRequest(bincode::deserialize(&data[0..4])?, bincode::deserialize(&data[4..8])?),
            1 => Self::BlockResponse(bincode::deserialize(data)?),
            2 => Self::ChallengeRequest(bincode::deserialize(&data[0..2])?, bincode::deserialize(&data[2..6])?),
            3 => Self::ChallengeResponse(bincode::deserialize(data)?),
            4 => match data.len() == 0 {
                true => Self::Disconnect,
                false => return Err(anyhow!("Invalid 'Disconnect' message: {:?} {:?}", buffer, data)),
            },
            5 => match data.len() == 0 {
                true => Self::PeerRequest,
                false => return Err(anyhow!("Invalid 'PeerRequest' message: {:?} {:?}", buffer, data)),
            },
            6 => Self::PeerResponse(bincode::deserialize(data)?),
            7 => Self::Ping(bincode::deserialize(&data[0..4])?),
            8 => Self::Pong(bincode::deserialize(data)?),
            9 => Self::UnconfirmedBlock(bincode::deserialize(data)?),
            10 => Self::UnconfirmedTransaction(bincode::deserialize(data)?),
            _ => return Err(anyhow!("Invalid message ID {}", id)),
        };

        Ok(message)
    }
}

impl<N: Network, E: Environment> Encoder<Message<N, E>> for Message<N, E> {
    type Error = anyhow::Error;

    fn encode(&mut self, message: Message<N, E>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Serialize the message into a buffer.
        let buffer = message.serialize()?;

        // Ensure the message does not exceed the maximum length limit.
        if buffer.len() > E::MAXIMUM_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", buffer.len()),
            )
            .into());
        }

        // Convert the length into a byte array.
        // The cast to u32 cannot overflow due to the length check above.
        let len_slice = u32::to_le_bytes(buffer.len() as u32);

        // Reserve space in the buffer.
        dst.reserve(4 + buffer.len());

        // Write the length and string to the buffer.
        dst.extend_from_slice(&len_slice);
        dst.extend_from_slice(&buffer);
        Ok(())
    }
}

impl<N: Network, E: Environment> Decoder for Message<N, E> {
    type Error = std::io::Error;
    type Item = Message<N, E>;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Ensure there is enough bytes to read the length marker.
        if source.len() < 4 {
            return Ok(None);
        }

        // Read the length marker.
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&source[..4]);
        let length = u32::from_le_bytes(length_bytes) as usize;

        // Check that the length is not too large to avoid a denial of
        // service attack where the node server runs out of memory.
        if length > E::MAXIMUM_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", length),
            ));
        }

        if source.len() < 4 + length {
            // The full message has not yet arrived.
            //
            // We reserve more space in the buffer. This is not strictly
            // necessary, but is a good idea performance-wise.
            source.reserve(4 + length - source.len());

            // We inform `Framed` that we need more bytes to form the next frame.
            return Ok(None);
        }

        // Use `advance` to modify the source such that it no longer contains this frame.
        let buffer = source[4..4 + length].to_vec();
        source.advance(4 + length);

        // Convert the buffer to a message, or fail if it is not valid.
        match Message::deserialize(&buffer) {
            Ok(message) => Ok(Some(message)),
            Err(error) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        }
    }
}
