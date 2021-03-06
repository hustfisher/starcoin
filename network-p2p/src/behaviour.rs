// Copyright 2019-2020 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

use crate::discovery::DiscoveryConfig;
use crate::protocol::{CustomMessageOutcome, Protocol};
use crate::{
    debug_info, discovery::DiscoveryBehaviour, discovery::DiscoveryOut, protocol::event::DhtEvent,
    protocol::event::Event, DiscoveryNetBehaviour, ProtocolId,
};
use libp2p::core::{Multiaddr, PeerId, PublicKey};
use libp2p::kad::record;
use libp2p::swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters,
};
use libp2p::NetworkBehaviour;
use log::debug;
use std::collections::VecDeque;
use std::{iter, task::Context, task::Poll};

/// General behaviour of the network. Combines all protocols together.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourOut", poll_method = "poll")]
pub struct Behaviour {
    protocol: Protocol,
    /// Periodically pings and identifies the nodes we are connected to, and store information in a
    /// cache.
    debug_info: debug_info::DebugInfoBehaviour,
    /// Discovers nodes of the network.
    discovery: DiscoveryBehaviour,
    /// Queue of events to produce for the outside.
    #[behaviour(ignore)]
    events: VecDeque<BehaviourOut>,
}

/// Event generated by `Behaviour`.
#[derive(Debug, Clone)]
pub enum BehaviourOut {
    Event(Event),
    RandomKademliaStarted(ProtocolId),
}

#[derive(Debug, Clone)]
pub struct RpcRequest {
    remote: PeerId,
    data: Vec<u8>,
}

impl Behaviour {
    /// Builds a new `Behaviour`.
    pub async fn new(
        protocol: Protocol,
        user_agent: String,
        local_public_key: PublicKey,
        disco_config: DiscoveryConfig,
    ) -> Self {
        Behaviour {
            protocol,
            debug_info: debug_info::DebugInfoBehaviour::new(user_agent, local_public_key),
            discovery: disco_config.finish(),
            events: VecDeque::new(),
        }
    }

    /// Returns the list of nodes that we know exist in the network.
    pub fn known_peers(&mut self) -> impl Iterator<Item = &PeerId> {
        self.discovery.known_peers()
    }

    /// Adds a hard-coded address for the given peer, that never expires.
    pub fn add_known_address(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.discovery.add_known_address(peer_id, addr)
    }

    pub fn get_address(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.discovery.addresses_of_peer(peer_id)
    }

    /// Returns true if we have a channel open with this node.
    pub fn is_open(&self, peer_id: &PeerId) -> bool {
        self.protocol.is_open(peer_id)
    }

    /// Borrows `self` and returns a struct giving access to the information about a node.
    ///
    /// Returns `None` if we don't know anything about this node. Always returns `Some` for nodes
    /// we're connected to, meaning that if `None` is returned then we're not connected to that
    /// node.
    pub fn node(&self, peer_id: &PeerId) -> Option<debug_info::Node> {
        self.debug_info.node(peer_id)
    }

    /// Start querying a record from the DHT. Will later produce either a `ValueFound` or a `ValueNotFound` event.
    pub fn get_value(&mut self, key: &record::Key) {
        self.discovery.get_value(key);
    }

    /// Starts putting a record into DHT. Will later produce either a `ValuePut` or a `ValuePutFailed` event.
    pub fn put_value(&mut self, key: record::Key, value: Vec<u8>) {
        self.discovery.put_value(key, value);
    }

    /// Returns a shared reference to the user protocol.
    pub fn user_protocol(&self) -> &Protocol {
        &self.protocol
    }

    /// Returns a mutable reference to the user protocol.
    pub fn user_protocol_mut(&mut self) -> &mut Protocol {
        &mut self.protocol
    }
}

impl NetworkBehaviourEventProcess<void::Void> for Behaviour {
    fn inject_event(&mut self, event: void::Void) {
        void::unreachable(event)
    }
}

impl NetworkBehaviourEventProcess<CustomMessageOutcome> for Behaviour {
    fn inject_event(&mut self, event: CustomMessageOutcome) {
        match event {
            CustomMessageOutcome::NotificationStreamOpened { remote, info } => {
                self.events
                    .push_back(BehaviourOut::Event(Event::NotificationStreamOpened {
                        remote,
                        info,
                    }));
            }
            CustomMessageOutcome::NotificationStreamClosed { remote } => {
                self.events
                    .push_back(BehaviourOut::Event(Event::NotificationStreamClosed {
                        remote,
                    }));
            }
            CustomMessageOutcome::NotificationsReceived {
                remote,
                protocol_name,
                messages,
            } => {
                let ev = Event::NotificationsReceived {
                    remote,
                    protocol_name,
                    messages,
                };
                self.events.push_back(BehaviourOut::Event(ev));
            }
            CustomMessageOutcome::None => {}
        }
    }
}

impl NetworkBehaviourEventProcess<debug_info::DebugInfoEvent> for Behaviour {
    fn inject_event(&mut self, event: debug_info::DebugInfoEvent) {
        let debug_info::DebugInfoEvent::Identified { peer_id, mut info } = event;
        if info.listen_addrs.len() > 30 {
            debug!(target: "sub-libp2p", "Node {:?} has reported more than 30 addresses; \
                it is identified by {:?} and {:?}", peer_id, info.protocol_version,
                info.agent_version
            );
            info.listen_addrs.truncate(30);
        }
        for addr in &info.listen_addrs {
            self.discovery
                .add_self_reported_address(&peer_id, addr.clone());
        }
        self.protocol.add_discovered_nodes(iter::once(peer_id));
    }
}

impl NetworkBehaviourEventProcess<DiscoveryOut> for Behaviour {
    fn inject_event(&mut self, out: DiscoveryOut) {
        match out {
            DiscoveryOut::UnroutablePeer(_peer_id) => {
                // Obtaining and reporting listen addresses for unroutable peers back
                // to Kademlia is handled by the `Identify` protocol, part of the
                // `DebugInfoBehaviour`. See the `NetworkBehaviourEventProcess`
                // implementation for `DebugInfoEvent`.
            }
            DiscoveryOut::Discovered(peer_id) => {
                self.protocol.add_discovered_nodes(iter::once(peer_id));
            }
            DiscoveryOut::ValueFound(results) => {
                self.events
                    .push_back(BehaviourOut::Event(Event::Dht(DhtEvent::ValueFound(
                        results,
                    ))));
            }
            DiscoveryOut::ValueNotFound(key) => {
                self.events
                    .push_back(BehaviourOut::Event(Event::Dht(DhtEvent::ValueNotFound(
                        key,
                    ))));
            }
            DiscoveryOut::ValuePut(key) => {
                self.events
                    .push_back(BehaviourOut::Event(Event::Dht(DhtEvent::ValuePut(key))));
            }
            DiscoveryOut::ValuePutFailed(key) => {
                self.events
                    .push_back(BehaviourOut::Event(Event::Dht(DhtEvent::ValuePutFailed(
                        key,
                    ))));
            }
            DiscoveryOut::RandomKademliaStarted(protocols) => {
                for protocol in protocols {
                    self.events
                        .push_back(BehaviourOut::RandomKademliaStarted(protocol));
                }
            }
        }
    }
}

impl Behaviour {
    fn poll<TEv>(
        &mut self,
        _: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<TEv, BehaviourOut>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}
