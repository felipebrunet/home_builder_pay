use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use uuid::Uuid;

use konstruado_core::{Oferta, Obra};

use crate::proto::{
    decode_obras, decode_tablero, encode_obras, encode_tablero, key_hex, Msg, PeerAddr,
};
use crate::tor::{EstadoTor, Tor};
use crate::{clave_obras, clave_tablero, PUERTO_LOCAL, RED};

struct Inner {
    id: String,
    addr: PeerAddr,
    peers: HashMap<String, PeerAddr>,
    store: HashMap<String, Vec<u8>>,
    tor: Tor,
    halt: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone)]
pub struct Nodo {
    inner: Arc<Mutex<Inner>>,
}

impl Nodo {
    pub async fn arrancar() -> std::io::Result<Self> {
        let tor = Tor::detectar().await;
        let id = Uuid::new_v4().to_string();
        let (listener, port) = bind_local().await?;
        let addr = PeerAddr::Tcp {
            host: "127.0.0.1".into(),
            port,
        };
        let (halt, _) = tokio::sync::watch::channel(false);
        let nodo = Self {
            inner: Arc::new(Mutex::new(Inner {
                id: id.clone(),
                addr: addr.clone(),
                peers: HashMap::new(),
                store: HashMap::new(),
                tor,
                halt: halt.clone(),
            })),
        };
        let accept = nodo.clone();
        let halt_accept = halt.clone();
        tokio::spawn(async move {
            let mut rx = halt_accept.subscribe();
            loop {
                tokio::select! {
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            break;
                        }
                    }
                    acc = listener.accept() => {
                        let Ok((stream, _)) = acc else { break };
                        let n = accept.clone();
                        tokio::spawn(async move {
                            let _ = n.sesion(stream).await;
                        });
                    }
                }
            }
        });
        if port != PUERTO_LOCAL {
            let _ = nodo
                .dial(PeerAddr::Tcp {
                    host: "127.0.0.1".into(),
                    port: PUERTO_LOCAL,
                })
                .await;
        }
        let tick = nodo.clone();
        let halt_g = halt;
        tokio::spawn(async move {
            let mut rx = halt_g.subscribe();
            loop {
                tokio::select! {
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {
                        tick.gossip().await;
                    }
                }
            }
        });
        Ok(nodo)
    }

    pub async fn parar(&self) {
        let _ = self.inner.lock().await.halt.send(true);
    }

    pub async fn estado_tor(&self) -> EstadoTor {
        self.inner.lock().await.tor.estado()
    }

    pub async fn addr(&self) -> PeerAddr {
        self.inner.lock().await.addr.clone()
    }

    pub async fn id(&self) -> String {
        self.inner.lock().await.id.clone()
    }

    pub async fn n_peers(&self) -> usize {
        self.inner.lock().await.peers.len()
    }

    pub async fn publicar(&self, oferta: Oferta) {
        let key = key_hex(&clave_tablero());
        let mut g = self.inner.lock().await;
        let mut list = g
            .store
            .get(&key)
            .map(|b| decode_tablero(b))
            .unwrap_or_default();
        list.retain(|o| o.id != oferta.id);
        list.insert(0, oferta);
        g.store.insert(key, encode_tablero(&list));
        drop(g);
        self.gossip().await;
    }

    pub async fn tablero(&self) -> Vec<Oferta> {
        let key = key_hex(&clave_tablero());
        let g = self.inner.lock().await;
        g.store
            .get(&key)
            .map(|b| decode_tablero(b))
            .unwrap_or_default()
    }

    pub async fn quitar(&self, oferta_id: &str) {
        let key = key_hex(&clave_tablero());
        let mut g = self.inner.lock().await;
        let mut list = g
            .store
            .get(&key)
            .map(|b| decode_tablero(b))
            .unwrap_or_default();
        list.retain(|o| o.id != oferta_id);
        g.store.insert(key, encode_tablero(&list));
        drop(g);
        self.gossip().await;
    }

    pub async fn publicar_obra(&self, obra: Obra) {
        let key = key_hex(&clave_obras());
        let mut g = self.inner.lock().await;
        let mut list = g
            .store
            .get(&key)
            .map(|b| decode_obras(b))
            .unwrap_or_default();
        list.retain(|o| o.id != obra.id);
        list.insert(0, obra);
        g.store.insert(key, encode_obras(&list));
        drop(g);
        self.gossip().await;
    }

    pub async fn obras(&self) -> Vec<Obra> {
        let key = key_hex(&clave_obras());
        let g = self.inner.lock().await;
        g.store
            .get(&key)
            .map(|b| decode_obras(b))
            .unwrap_or_default()
    }

    async fn gossip(&self) {
        let (peers, puts) = {
            let g = self.inner.lock().await;
            let peers: Vec<_> = g.peers.values().cloned().collect();
            let puts: Vec<_> = g
                .store
                .iter()
                .map(|(k, v)| Msg::Put {
                    key: k.clone(),
                    val: v.clone(),
                })
                .collect();
            (peers, puts)
        };
        for addr in peers {
            for msg in &puts {
                let _ = self.send(&addr, msg).await;
            }
        }
    }

    async fn dial(&self, addr: PeerAddr) -> std::io::Result<()> {
        let stream = self.connect(&addr).await?;
        self.sesion(stream).await
    }

    async fn connect(&self, addr: &PeerAddr) -> std::io::Result<TcpStream> {
        let tor = self.inner.lock().await.tor.clone();
        match addr {
            PeerAddr::Tcp { host, port } => TcpStream::connect((host.as_str(), *port)).await,
            PeerAddr::Onion { host, port } => tor.conectar(host, *port).await,
        }
    }

    async fn send(&self, addr: &PeerAddr, msg: &Msg) -> std::io::Result<()> {
        let mut s = self.connect(addr).await?;
        write_msg(&mut s, msg).await
    }

    async fn sesion(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let (id, addr) = {
            let g = self.inner.lock().await;
            (g.id.clone(), g.addr.clone())
        };
        write_msg(
            &mut stream,
            &Msg::Hola {
                node: id,
                addr,
                swarm: RED.into(),
            },
        )
        .await?;
        loop {
            let msg = match read_msg(&mut stream).await {
                Ok(m) => m,
                Err(_) => break,
            };
            if let Some(reply) = self.handle(msg).await {
                let _ = write_msg(&mut stream, &reply).await;
            }
        }
        Ok(())
    }

    async fn handle(&self, msg: Msg) -> Option<Msg> {
        match msg {
            Msg::Hola { node, addr, swarm } => {
                if swarm != RED {
                    return None;
                }
                let mut g = self.inner.lock().await;
                if node != g.id {
                    g.peers.insert(node, addr);
                }
                let list: Vec<_> = g
                    .peers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .chain(std::iter::once((g.id.clone(), g.addr.clone())))
                    .collect();
                Some(Msg::Peers { list })
            }
            Msg::Peers { list } => {
                let mut g = self.inner.lock().await;
                let me = g.id.clone();
                for (id, addr) in list {
                    if id != me {
                        g.peers.insert(id, addr);
                    }
                }
                None
            }
            Msg::Put { key, val } => {
                let mut g = self.inner.lock().await;
                merge_store(&mut g.store, key, val);
                None
            }
            Msg::Get { key } => {
                let g = self.inner.lock().await;
                Some(Msg::Got {
                    val: g.store.get(&key).cloned(),
                    key,
                })
            }
            Msg::Got { key, val } => {
                if let Some(val) = val {
                    let mut g = self.inner.lock().await;
                    merge_store(&mut g.store, key, val);
                }
                None
            }
            Msg::Ping => Some(Msg::Pong),
            Msg::Pong => None,
        }
    }
}

fn merge_store(store: &mut HashMap<String, Vec<u8>>, key: String, val: Vec<u8>) {
    if key == key_hex(&crate::clave_tablero()) {
        let mut a = store.get(&key).map(|b| decode_tablero(b)).unwrap_or_default();
        let b = decode_tablero(&val);
        for o in b {
            if !a.iter().any(|x| x.id == o.id) {
                a.push(o);
            }
        }
        store.insert(key, encode_tablero(&a));
    } else if key == key_hex(&crate::clave_obras()) {
        let mut a = store.get(&key).map(|b| decode_obras(b)).unwrap_or_default();
        let b = decode_obras(&val);
        for o in b {
            a.retain(|x| x.id != o.id);
            a.push(o);
        }
        store.insert(key, encode_obras(&a));
    } else {
        store.entry(key).or_insert(val);
    }
}

async fn bind_local() -> std::io::Result<(TcpListener, u16)> {
    match TcpListener::bind(("127.0.0.1", PUERTO_LOCAL)).await {
        Ok(l) => Ok((l, PUERTO_LOCAL)),
        Err(_) => {
            let l = TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = l.local_addr()?.port();
            Ok((l, port))
        }
    }
}

async fn write_msg(s: &mut TcpStream, msg: &Msg) -> std::io::Result<()> {
    let buf = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
    s.write_all(&len.to_be_bytes()).await?;
    s.write_all(&buf).await?;
    s.flush().await
}

async fn read_msg(s: &mut TcpStream) -> std::io::Result<Msg> {
    let mut h = [0u8; 4];
    s.read_exact(&mut h).await?;
    let n = u32::from_be_bytes(h) as usize;
    if n > 1_000_000 {
        return Err(std::io::Error::other("msg too big"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use konstruado_core::{Oferta, Persona};

    #[test]
    fn merge_une_tableros() {
        let mut store = HashMap::new();
        let m = Persona::nueva("Felipe").unwrap();
        let o = Oferta::publicar(m, "Casa", 10_000, 2_000).unwrap();
        let id = o.id.clone();
        let key = key_hex(&crate::clave_tablero());
        merge_store(&mut store, key.clone(), encode_tablero(&[o]));
        let tab = decode_tablero(store.get(&key).unwrap());
        assert_eq!(tab[0].id, id);
        assert_eq!(tab[0].n_partidas_sugeridas, 5);
    }
}
