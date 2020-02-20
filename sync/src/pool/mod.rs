use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};
use std::time::Duration;
use types::{block::BlockNumber, peer_info::PeerInfo};

#[derive(Eq, PartialEq, Clone, Debug)]
struct TTLEntry<E>
where
    E: Ord + Clone,
{
    data: E,
    expiration_time: Duration,
    block_number: BlockNumber,
    peers: HashSet<PeerInfo>,
}

impl<E> TTLEntry<E>
where
    E: Ord + Clone,
{
    fn expiration_time(&self) -> Duration {
        self.expiration_time
    }

    fn new(peer: PeerInfo, block_number: BlockNumber, entry: E) -> Self {
        let mut peers = HashSet::new();
        peers.insert(peer);
        TTLEntry {
            data: entry,
            expiration_time: Duration::from_secs(60 * 60),
            block_number,
            peers,
        }
    }
}

impl<E> PartialOrd for TTLEntry<E>
where
    E: Ord + Clone,
{
    fn partial_cmp(&self, other: &TTLEntry<E>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<E> Ord for TTLEntry<E>
where
    E: Ord + Clone,
{
    fn cmp(&self, other: &Self) -> Ordering {
        match self.block_number.cmp(&other.block_number) {
            Ordering::Equal => {
                return self.data.cmp(&other.data);
            }
            ordering => return ordering,
        }
    }
}

pub struct TTLPool<E>
where
    E: Ord + Clone,
{
    data: BTreeSet<TTLEntry<E>>,
}

impl<E> TTLPool<E>
where
    E: Ord + Clone,
{
    pub(crate) fn new() -> Self {
        Self {
            data: BTreeSet::new(),
        }
    }

    /// add entry to pool
    pub(crate) fn insert(&mut self, peer: PeerInfo, number: BlockNumber, entry: E) {
        let mut ttl_entry = TTLEntry::new(peer.clone(), number, entry);
        if self.data.contains(&ttl_entry) {
            ttl_entry = self.data.take(&ttl_entry).expect("entry not exist.")
        };

        ttl_entry.peers.insert(peer);
        self.data.insert(ttl_entry);
    }

    /// take entry from pool
    pub(crate) fn take(&mut self, size: usize) -> Vec<E> {
        let mut set_iter = self.data.iter();
        let mut entries = Vec::new();
        loop {
            if entries.len() >= size {
                break;
            }

            let entry = set_iter.next();

            if entry.is_none() {
                break;
            }

            let ttl_entry = entry.expect("entry is none.").clone();
            entries.push(ttl_entry);
        }

        drop(set_iter);

        if !entries.is_empty() {
            entries.iter().for_each(|e| {
                self.data.remove(e);
            });
        }

        entries.iter().map(|e| e.data.clone()).collect()
    }

    pub(crate) fn gc(&mut self, now: Duration) -> Vec<E> {
        //todo
        unimplemented!()
    }

    pub(crate) fn size(&self) -> usize {
        self.data.len()
    }
}