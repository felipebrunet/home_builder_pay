//! TCP Kademlia overlay. Peers find each other over Tor SOCKS or direct TCP.
//!
//! Mainline BitTorrent DHT is UDP and does not traverse a Tor SOCKS5 proxy.
//! This is a purpose-built TCP Kademlia (`FIND_NODE` / `FIND_VALUE` / `STORE`
//! / `PING` / `DELIVER`).
//!
//! Two isolated onions are not a connected graph. After Tor is up, announce
//! also hits a public HTTPS rendezvous (see [`crate::rendezvous`]) so **Buscar
//! por nombre** works without pasting onions. Onion paste remains fallback.

use std::collections::{BTreeMap, HashSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::dht::{
    parse_node_id, person_topic_key, work_topic_key, xor_distance, DhtRecord, PeerInfo,
    WorkAnnounce,
};
use crate::message::NetMessage;
use crate::wire::{connect_peer, read_frame, write_frame, Op, PeerAddr, ResBody, WireMsg};

const K: usize = 8;
const ALPHA: usize = 3;
const MAX_PEERS: usize = 64;

#[derive(Clone)]
pub struct OverlayConfig {
    pub listen: SocketAddr,
    pub socks: Option<SocketAddr>,
    pub advertised: Option<PeerAddr>,
    pub connect_timeout: Duration,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        // Product path: WAN / .onion via Tor SOCKS. Loopback stays direct
        // in `connect_peer`, so localhost tests are unaffected.
        let socks = crate::tor::TorConfig::from_env().socks().parse().ok();
        Self {
            listen: "127.0.0.1:3848".parse().expect("static"),
            socks,
            advertised: None,
            connect_timeout: Duration::from_secs(8),
        }
    }
}

struct State {
    node_id: [u8; 32],
    advertised: PeerAddr,
    socks: Option<SocketAddr>,
    timeout: Duration,
    peers: Vec<PeerInfo>,
    store: BTreeMap<[u8; 32], DhtRecord>,
    inbox: Vec<NetMessage>,
}

#[derive(Clone)]
pub struct OverlayHandle {
    state: Arc<Mutex<State>>,
    local_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    rpc_ids: Arc<AtomicU64>,
}

impl OverlayHandle {
    pub fn bind(mut cfg: OverlayConfig) -> crate::Result<Self> {
        let listener = TcpListener::bind(cfg.listen)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let mut node_id = [0u8; 32];
        getrandom::getrandom(&mut node_id).map_err(|e| crate::Error::msg(e.to_string()))?;
        let advertised = cfg.advertised.take().unwrap_or_else(|| PeerAddr {
            host: local_addr.ip().to_string(),
            port: local_addr.port(),
        });
        let state = Arc::new(Mutex::new(State {
            node_id,
            advertised,
            socks: cfg.socks,
            timeout: cfg.connect_timeout,
            peers: Vec::new(),
            store: BTreeMap::new(),
            inbox: Vec::new(),
        }));
        let shutdown = Arc::new(AtomicBool::new(false));
        let rpc_ids = Arc::new(AtomicU64::new(1));
        {
            let state = Arc::clone(&state);
            let shutdown = Arc::clone(&shutdown);
            thread::Builder::new()
                .name("hbp-dht".into())
                .spawn(move || accept_loop(listener, state, shutdown))
                .map_err(|e| crate::Error::msg(e.to_string()))?;
        }
        Ok(Self {
            state,
            local_addr,
            shutdown,
            rpc_ids,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn node_id_hex(&self) -> String {
        hex::encode(self.state.lock().expect("dht").node_id)
    }

    pub fn advertised(&self) -> PeerAddr {
        self.state.lock().expect("dht").advertised.clone()
    }

    pub fn set_advertised(&self, addr: PeerAddr) {
        self.state.lock().expect("dht").advertised = addr;
    }

    pub fn set_socks(&self, socks: Option<SocketAddr>) {
        self.state.lock().expect("dht").socks = socks;
    }

    pub fn socks(&self) -> Option<SocketAddr> {
        self.state.lock().expect("dht").socks
    }

    pub fn peer_count(&self) -> usize {
        self.state.lock().expect("dht").peers.len()
    }

    pub fn store_len(&self) -> usize {
        self.state.lock().expect("dht").store.len()
    }

    pub fn peers(&self) -> Vec<PeerInfo> {
        self.state.lock().expect("dht").peers.clone()
    }

    pub fn take_inbox(&self) -> Vec<NetMessage> {
        std::mem::take(&mut self.state.lock().expect("dht").inbox)
    }

    pub fn bootstrap(&self, peers: &[PeerAddr]) -> crate::Result<usize> {
        let mut ok = 0usize;
        for p in peers {
            if self.ping(p).is_ok() {
                ok += 1;
            }
        }
        if !peers.is_empty() {
            let target = self.state.lock().expect("dht").node_id;
            let _ = self.iterative_find_node(target);
        }
        Ok(ok)
    }

    pub fn ping(&self, dest: &PeerAddr) -> crate::Result<ResBody> {
        let (node_id, listen) = {
            let s = self.state.lock().expect("dht");
            (hex::encode(s.node_id), s.advertised.clone())
        };
        self.call(dest, Op::Ping { node_id, listen })
    }

    pub fn announce_work(&self, ann: &WorkAnnounce) -> crate::Result<[u8; 32]> {
        let key = work_topic_key(&ann.work_name);
        self.store_record(key, ann)?;
        if !ann.person_name.trim().is_empty() {
            let _ = self.store_record(person_topic_key(&ann.person_name), ann);
        }
        Ok(key)
    }

    fn store_record(&self, key: [u8; 32], ann: &WorkAnnounce) -> crate::Result<()> {
        let record = DhtRecord {
            key: hex::encode(key),
            value: serde_json::to_vec(ann)?,
            publisher: Some(self.advertised()),
            ttl_secs: 86_400,
        };
        {
            let mut s = self.state.lock().expect("dht");
            s.store.insert(key, record.clone());
        }
        let mut targets = self.peers();
        for p in self.iterative_find_node(key) {
            if targets.iter().all(|x| x.node_id != p.node_id) {
                targets.push(p);
            }
        }
        for p in targets {
            let _ = self.call(
                &p.addr,
                Op::Store {
                    record: record.clone(),
                },
            );
        }
        Ok(())
    }

    /// DHT lookup (normalized name). Does not hit the public rendezvous.
    pub fn lookup_work(&self, work_name: &str) -> crate::Result<Option<WorkAnnounce>> {
        self.lookup_key(work_topic_key(work_name))
    }

    pub fn lookup_person(&self, person_name: &str) -> crate::Result<Option<WorkAnnounce>> {
        self.lookup_key(person_topic_key(person_name))
    }

    fn lookup_key(&self, key: [u8; 32]) -> crate::Result<Option<WorkAnnounce>> {
        match self.iterative_find_value(key)? {
            Some(rec) => Ok(Some(serde_json::from_slice(&rec.value)?)),
            None => {
                let s = self.state.lock().expect("dht");
                match s.store.get(&key) {
                    Some(rec) => Ok(Some(serde_json::from_slice(&rec.value)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// DHT by obra name, then by mandante name, then public rendezvous.
    pub fn discover_work(&self, query: &str) -> crate::Result<Option<WorkAnnounce>> {
        if let Some(ann) = self.lookup_work(query)? {
            return Ok(Some(ann));
        }
        if let Some(ann) = self.lookup_person(query)? {
            return Ok(Some(ann));
        }
        crate::rendezvous::lookup_announce(self.socks(), query)
    }

    /// Publish to the public name board. Best-effort; DHT announce is separate.
    pub fn publish_rendezvous(&self, ann: &WorkAnnounce) -> crate::Result<String> {
        crate::rendezvous::publish_announce(self.socks(), ann)
    }

    pub fn deliver(&self, dest: &PeerAddr, msg: &NetMessage) -> crate::Result<()> {
        match self.call(dest, Op::Deliver { msg: msg.clone() })? {
            ResBody::Delivered => Ok(()),
            other => Err(crate::Error::msg(format!(
                "unexpected deliver reply: {other:?}"
            ))),
        }
    }

    fn call(&self, dest: &PeerAddr, op: Op) -> crate::Result<ResBody> {
        let (socks, timeout, us) = {
            let s = self.state.lock().expect("dht");
            (s.socks, s.timeout, s.advertised.clone())
        };
        if dest.host == us.host && dest.port == us.port && dest.host != "0.0.0.0" {
            return Ok(dispatch(&self.state, op));
        }
        let timeout = if dest.is_onion() {
            timeout.max(Duration::from_secs(40))
        } else {
            timeout
        };
        let mut stream = connect_peer(dest, socks, timeout)?;
        let id = self.rpc_ids.fetch_add(1, Ordering::Relaxed);
        write_frame(&mut stream, &serde_json::to_vec(&WireMsg::Req { id, op })?)?;
        let raw = read_frame(&mut stream)?;
        match serde_json::from_slice::<WireMsg>(&raw)? {
            WireMsg::Res { id: rid, body } if rid == id => {
                if let ResBody::Pong { node_id, listen } = &body {
                    learn(
                        &self.state,
                        PeerInfo {
                            node_id: node_id.clone(),
                            addr: listen.clone(),
                        },
                    );
                }
                Ok(body)
            }
            WireMsg::Err { error, .. } => Err(crate::Error::msg(error)),
            other => Err(crate::Error::msg(format!("bad rpc reply: {other:?}"))),
        }
    }

    fn iterative_find_node(&self, target: [u8; 32]) -> Vec<PeerInfo> {
        self.iterative(target, false).1
    }

    fn iterative_find_value(&self, key: [u8; 32]) -> crate::Result<Option<DhtRecord>> {
        Ok(self.iterative(key, true).0)
    }

    fn iterative(&self, target: [u8; 32], want_value: bool) -> (Option<DhtRecord>, Vec<PeerInfo>) {
        let mut shortlist = closest_locked(&self.state, &target, K);
        let mut queried: HashSet<String> = HashSet::new();
        let target_hex = hex::encode(target);
        for _ in 0..8 {
            let batch: Vec<PeerInfo> = shortlist
                .iter()
                .filter(|p| !queried.contains(&p.node_id))
                .take(ALPHA)
                .cloned()
                .collect();
            if batch.is_empty() {
                break;
            }
            let prev_best = shortlist
                .first()
                .and_then(|p| parse_node_id(&p.node_id).ok())
                .map(|id| xor_distance(&id, &target));
            for p in batch {
                queried.insert(p.node_id.clone());
                let op = if want_value {
                    Op::FindValue {
                        key: target_hex.clone(),
                    }
                } else {
                    Op::FindNode {
                        target: target_hex.clone(),
                    }
                };
                match self.call(&p.addr, op) {
                    Ok(ResBody::Value { record }) if want_value => {
                        return (Some(record), shortlist);
                    }
                    Ok(ResBody::Nodes { nodes }) => {
                        for n in nodes {
                            learn(&self.state, n.clone());
                            if shortlist.iter().all(|x| x.node_id != n.node_id) {
                                shortlist.push(n);
                            }
                        }
                    }
                    _ => {}
                }
            }
            sort_by_distance(&mut shortlist, &target);
            shortlist.truncate(K);
            let new_best = shortlist
                .first()
                .and_then(|p| parse_node_id(&p.node_id).ok())
                .map(|id| xor_distance(&id, &target));
            if new_best >= prev_best && prev_best.is_some() {
                for p in shortlist
                    .iter()
                    .filter(|p| !queried.contains(&p.node_id))
                    .cloned()
                    .collect::<Vec<_>>()
                {
                    queried.insert(p.node_id.clone());
                    let op = if want_value {
                        Op::FindValue {
                            key: target_hex.clone(),
                        }
                    } else {
                        Op::FindNode {
                            target: target_hex.clone(),
                        }
                    };
                    if let Ok(ResBody::Value { record }) = self.call(&p.addr, op) {
                        if want_value {
                            return (Some(record), shortlist);
                        }
                    } else if let Ok(ResBody::Nodes { nodes }) = self.call(
                        &p.addr,
                        Op::FindNode {
                            target: target_hex.clone(),
                        },
                    ) {
                        for n in nodes {
                            learn(&self.state, n.clone());
                            if shortlist.iter().all(|x| x.node_id != n.node_id) {
                                shortlist.push(n);
                            }
                        }
                    }
                }
                sort_by_distance(&mut shortlist, &target);
                shortlist.truncate(K);
                break;
            }
        }
        (None, shortlist)
    }
}

impl Drop for OverlayHandle {
    fn drop(&mut self) {
        // Clones share the listener; only the last handle stops accept.
        if Arc::strong_count(&self.state) <= 1 {
            self.shutdown.store(true, Ordering::Relaxed);
        }
    }
}

fn accept_loop(listener: TcpListener, state: Arc<Mutex<State>>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(8)));
                let _ = handle_conn(stream, &state);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn handle_conn(mut stream: TcpStream, state: &Arc<Mutex<State>>) -> crate::Result<()> {
    let raw = read_frame(&mut stream)?;
    let req: WireMsg = serde_json::from_slice(&raw)?;
    let reply = match req {
        WireMsg::Req { id, op } => {
            let body = dispatch(state, op);
            WireMsg::Res { id, body }
        }
        other => WireMsg::Err {
            id: 0,
            error: format!("expected req, got {other:?}"),
        },
    };
    write_frame(&mut stream, &serde_json::to_vec(&reply)?)?;
    Ok(())
}

fn dispatch(state: &Arc<Mutex<State>>, op: Op) -> ResBody {
    match op {
        Op::Ping { node_id, listen } => {
            learn(
                state,
                PeerInfo {
                    node_id,
                    addr: listen,
                },
            );
            let s = state.lock().expect("dht");
            ResBody::Pong {
                node_id: hex::encode(s.node_id),
                listen: s.advertised.clone(),
            }
        }
        Op::FindNode { target } => {
            let t = parse_node_id(&target).unwrap_or([0u8; 32]);
            ResBody::Nodes {
                nodes: closest_locked(state, &t, K),
            }
        }
        Op::FindValue { key } => {
            let k = parse_node_id(&key).unwrap_or([0u8; 32]);
            let s = state.lock().expect("dht");
            if let Some(rec) = s.store.get(&k) {
                ResBody::Value {
                    record: rec.clone(),
                }
            } else {
                drop(s);
                ResBody::Nodes {
                    nodes: closest_locked(state, &k, K),
                }
            }
        }
        Op::Store { record } => {
            if let Ok(k) = parse_node_id(&record.key) {
                state.lock().expect("dht").store.insert(k, record);
            }
            ResBody::Stored
        }
        Op::Deliver { msg } => {
            state.lock().expect("dht").inbox.push(msg);
            ResBody::Delivered
        }
    }
}

fn learn(state: &Arc<Mutex<State>>, info: PeerInfo) {
    if parse_node_id(&info.node_id).is_err() {
        return;
    }
    let mut s = state.lock().expect("dht");
    if hex::encode(s.node_id) == info.node_id {
        return;
    }
    if let Some(existing) = s.peers.iter_mut().find(|p| p.node_id == info.node_id) {
        existing.addr = info.addr;
        return;
    }
    s.peers.push(info);
    if s.peers.len() > MAX_PEERS {
        s.peers.remove(0);
    }
}

fn closest_locked(state: &Arc<Mutex<State>>, target: &[u8; 32], n: usize) -> Vec<PeerInfo> {
    let s = state.lock().expect("dht");
    let mut peers = s.peers.clone();
    drop(s);
    sort_by_distance(&mut peers, target);
    peers.truncate(n);
    peers
}

fn sort_by_distance(peers: &mut [PeerInfo], target: &[u8; 32]) {
    peers.sort_by(|a, b| {
        let da = parse_node_id(&a.node_id)
            .map(|id| xor_distance(&id, target))
            .unwrap_or([0xff; 32]);
        let db = parse_node_id(&b.node_id)
            .map(|id| xor_distance(&id, target))
            .unwrap_or([0xff; 32]);
        da.cmp(&db)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind_local() -> OverlayHandle {
        let mut cfg = OverlayConfig::default();
        cfg.listen = "127.0.0.1:0".parse().unwrap();
        OverlayHandle::bind(cfg).unwrap()
    }

    #[test]
    fn two_peers_announce_and_lookup_over_tcp() {
        let a = bind_local();
        let b = bind_local();
        let a_addr = PeerAddr::new("127.0.0.1", a.local_addr().port());
        assert_eq!(b.bootstrap(&[a_addr.clone()]).unwrap(), 1);
        assert!(b.peer_count() >= 1);
        a.announce_work(&WorkAnnounce {
            work_name: "Casa Norte".into(),
            onion: "alice.onion".into(),
            offer_id: Some("ab".into()),
            role: "mandante".into(),
            person_name: "Don José".into(),
        })
        .unwrap();
        // B must learn the record via FIND_VALUE on A.
        let found = b.lookup_work("casa  NORTE").unwrap().expect("lookup");
        assert_eq!(found.onion, "alice.onion");
        assert_eq!(found.offer_id.as_deref(), Some("ab"));
        let by_person = b.lookup_person("don josé").unwrap().expect("person");
        assert_eq!(by_person.work_name, "Casa Norte");
    }

    #[test]
    fn clone_does_not_stop_listener() {
        let a = bind_local();
        let b = a.clone();
        drop(b);
        let addr = PeerAddr::new("127.0.0.1", a.local_addr().port());
        a.announce_work(&WorkAnnounce {
            work_name: "X".into(),
            onion: "x.onion".into(),
            offer_id: None,
            role: "mandante".into(),
            person_name: String::new(),
        })
        .unwrap();
        let c = bind_local();
        c.bootstrap(&[addr]).unwrap();
        assert!(c.lookup_work("X").unwrap().is_some());
    }

    #[test]
    fn three_peers_iterative_lookup() {
        let a = bind_local();
        let b = bind_local();
        let c = bind_local();
        let a_addr = PeerAddr::new("127.0.0.1", a.local_addr().port());
        let b_addr = PeerAddr::new("127.0.0.1", b.local_addr().port());
        b.bootstrap(&[a_addr]).unwrap();
        c.bootstrap(&[b_addr]).unwrap();
        a.announce_work(&WorkAnnounce {
            work_name: "Obra".into(),
            onion: "a.onion".into(),
            offer_id: None,
            role: "mandante".into(),
            person_name: String::new(),
        })
        .unwrap();
        let found = c.lookup_work("Obra").unwrap().expect("c finds via b→a");
        assert_eq!(found.onion, "a.onion");
    }

    #[test]
    fn deliver_offer_reaches_inbox() {
        let a = bind_local();
        let b = bind_local();
        let a_addr = PeerAddr::new("127.0.0.1", a.local_addr().port());
        let msg = NetMessage::Ping {
            work_name: "x".into(),
        };
        b.deliver(&a_addr, &msg).unwrap();
        let inbox = a.take_inbox();
        assert_eq!(inbox, vec![msg]);
    }

    #[test]
    fn deliver_hello_carries_onion_to_inbox() {
        let mandante = bind_local();
        let contratista = bind_local();
        let dest = PeerAddr::new("127.0.0.1", mandante.local_addr().port());
        let msg = NetMessage::Hello {
            work_name: "casa2".into(),
            onion: "felipe.onion".into(),
            person_name: "Felipe".into(),
            role: "contratista".into(),
        };
        contratista.deliver(&dest, &msg).unwrap();
        match mandante.take_inbox().into_iter().next() {
            Some(NetMessage::Hello { onion, .. }) => assert_eq!(onion, "felipe.onion"),
            other => panic!("expected hello, got {other:?}"),
        }
    }
}
