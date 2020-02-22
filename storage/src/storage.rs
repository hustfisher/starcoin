// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::memory_storage::MemoryStorage;
use crate::persistence_storage::PersistenceStorage;
use anyhow::{Error, Result};
use crypto::HashValue;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

pub trait Repository {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()>;
    fn contains_key(&self, key: Vec<u8>) -> Result<bool>;
    fn remove(&self, key: Vec<u8>) -> Result<()>;
    fn get_len(&self) -> Result<u64>;
    fn keys(&self) -> Result<Vec<Vec<u8>>>;
}

pub struct Storage {
    cache: Arc<dyn Repository>,
    persistence: Arc<dyn Repository>,
}

impl Storage {
    pub fn new(cache: Arc<dyn Repository>, persistence: Arc<dyn Repository>) -> Self {
        Storage { cache, persistence }
    }
}

impl Repository for Storage {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        // first get from cache
        match self.cache.get(key) {
            Ok(v) => Ok(v),
            _ => self.persistence.get(key),
        }
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Error> {
        unimplemented!()
    }

    fn contains_key(&self, key: Vec<u8>) -> Result<bool, Error> {
        unimplemented!()
    }

    fn remove(&self, key: Vec<u8>) -> Result<(), Error> {
        unimplemented!()
    }

    fn get_len(&self) -> Result<u64, Error> {
        unimplemented!()
    }

    fn keys(&self) -> Result<Vec<Vec<u8>>, Error> {
        unimplemented!()
    }
}

pub trait KeyCodec: Sized + PartialEq + Debug {
    /// Converts `self` to bytes to be stored in DB.
    fn encode_key(&self) -> Result<Vec<u8>>;
    /// Converts bytes fetched from DB to `Self`.
    fn decode_key(data: &[u8]) -> Result<Self>;
}

pub trait ValueCodec: Sized + PartialEq + Debug {
    /// Converts `self` to bytes to be stored in DB.
    fn encode_value(&self) -> Result<Vec<u8>>;
    /// Converts bytes fetched from DB to `Self`.
    fn decode_value(data: &[u8]) -> Result<Self>;
}

pub struct CodecStorage<K, V>
where
    K: KeyCodec,
    V: ValueCodec,
{
    store: Arc<dyn Repository>,
    k: PhantomData<K>,
    v: PhantomData<V>,
}

impl<K, V> CodecStorage<K, V>
where
    K: KeyCodec,
    V: ValueCodec,
{
    pub fn new(store: Arc<dyn Repository>) -> Self {
        Self {
            store,
            k: PhantomData,
            v: PhantomData,
        }
    }
    pub fn get(&self, key: K) -> Result<Option<V>> {
        match self.store.get(key.encode_key()?.as_slice())? {
            Some(v) => Ok(Some(V::decode_value(v.as_slice())?)),
            None => Ok(None),
        }
    }
    pub fn put(&self, key: K, value: V) -> Result<()> {
        self.store.put(key.encode_key()?, value.encode_value()?)
    }
    pub fn contains_key(&self, key: K) -> Result<bool> {
        self.store.contains_key(key.encode_key()?)
    }
    pub fn remove(&self, key: K) -> Result<()> {
        self.store.remove(key.encode_key()?)
    }

    pub fn get_len(&self) -> Result<u64> {
        self.store.get_len()
    }
    pub fn keys(&self) -> Result<Vec<Vec<u8>>> {
        self.store.keys()
    }
}

impl KeyCodec for HashValue {
    fn encode_key(&self) -> Result<Vec<u8>> {
        Ok(self.to_vec())
    }

    fn decode_key(data: &[u8]) -> Result<Self, Error> {
        Ok(HashValue::from_slice(data)?)
    }
}

impl ValueCodec for HashValue {
    fn encode_value(&self) -> Result<Vec<u8>> {
        Ok(self.to_vec())
    }

    fn decode_value(data: &[u8]) -> Result<Self, Error> {
        Ok(HashValue::from_slice(data)?)
    }
}