use chrono::{DateTime, Utc};
use std::{collections::HashMap, net::SocketAddr};

/// Stores the existence of a peer and the date they were last seen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddressBook {
    addresses: HashMap<SocketAddr, DateTime<Utc>>,
}

impl AddressBook {
    /// Construct a new `AddressBook`.
    pub fn new() -> Self {
        Self {
            addresses: HashMap::default(),
        }
    }

    /// Insert or update a new date for an address. Returns true if the new date is stored.
    pub fn update(&mut self, address: SocketAddr, date: DateTime<Utc>) -> bool {
        match self.addresses.get(&address) {
            Some(stored_date) => {
                if stored_date > &date {
                    false
                } else {
                    self.addresses.insert(address, date);
                    true
                }
            }
            None => {
                self.addresses.insert(address, date);
                true
            }
        }
    }

    /// Returns true if address is stored in the mapping.
    pub fn contains(&self, address: &SocketAddr) -> bool {
        self.addresses.contains_key(address)
    }

    /// Remove an address mapping and return its last seen date.
    pub fn remove(&mut self, address: &SocketAddr) -> Option<DateTime<Utc>> {
        self.addresses.remove(address)
    }

    /// Returns the number of stored peers.
    pub fn length(&self) -> u16 {
        self.addresses.len() as u16
    }

    /// Returns copy of addresses
    pub fn get_addresses(&self) -> HashMap<SocketAddr, DateTime<Utc>> {
        self.addresses.clone()
    }
}
