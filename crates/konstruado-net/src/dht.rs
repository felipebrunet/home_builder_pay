use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use konstruado_core::{Oferta, Obra, Persona};

use crate::proto::{
    decode_obras, decode_presentes, decode_tablero, encode_obras, encode_presentes, encode_tablero,
    key_hex, Msg, PeerAddr,
};
use crate::tor::{EstadoTor, Tor};
use crate::{clave_obras, clave_presentes, clave_tablero, PUERTO_LOCAL, RED};

struct Inner {
    id: String,
    addr: PeerAddr,
    local_port: u16,
    bootstrap: u16,
    peers: HashMap<String, PeerAddr>,
    store: HashMap<String, Vec<u8>>,
    tor: Tor,
    halt: tokio::sync::watch::Sender<bool>,
    /// Some(true) = mandante hosts the baked room. Some(false) = only dial.
    rol_sala: Option<bool>,
}

#[derive(Clone)]
pub struct Nodo {
    inner: Arc<Mutex<Inner>>,
    handle: tokio::runtime::Handle,
}

impl Nodo {
    pub async fn arrancar() -> std::io::Result<Self> {
        let n = Self::arrancar_en(PUERTO_LOCAL).await?;
        n.lanzar_tor();
        Ok(n)
    }

    pub async fn arrancar_en(bootstrap: u16) -> std::io::Result<Self> {
        Self::montar(tokio::runtime::Handle::current(), bootstrap).await
    }

    async fn montar(
        handle: tokio::runtime::Handle,
        bootstrap: u16,
    ) -> std::io::Result<Self> {
        let tor = Tor::ausente();
        let id = Uuid::new_v4().to_string();
        let (listener, port) = bind_local(bootstrap).await?;
        let addr = PeerAddr::Tcp {
            host: "127.0.0.1".into(),
            port,
        };
        let (halt, _) = tokio::sync::watch::channel(false);
        let nodo = Self {
            inner: Arc::new(Mutex::new(Inner {
                id: id.clone(),
                addr: addr.clone(),
                local_port: port,
                bootstrap,
                peers: HashMap::new(),
                store: HashMap::new(),
                tor,
                halt: halt.clone(),
                rol_sala: None,
            })),
            handle: handle.clone(),
        };
        let accept = nodo.clone();
        let halt_accept = halt.clone();
        handle.spawn(async move {
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
                            let _ = n.sesion_in(stream).await;
                        });
                    }
                }
            }
        });
        if port != bootstrap {
            let n = nodo.clone();
            handle.spawn(async move {
                let _ = n
                    .dial(PeerAddr::Tcp {
                        host: "127.0.0.1".into(),
                        port: bootstrap,
                    })
                    .await;
            });
        }
        let tick = nodo.clone();
        let halt_g = halt;
        handle.spawn(async move {
            let mut rx = halt_g.subscribe();
            loop {
                tokio::select! {
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        tick.gossip().await;
                    }
                }
            }
        });
        Ok(nodo)
    }

    fn lanzar_tor(&self) {
        if !crate::tor::hay_tor() {
            return;
        }
        self.inner
            .lock()
            .unwrap()
            .tor
            .marcar_arrancando("tor…");
        let n = self.clone();
        let port = n.inner.lock().unwrap().local_port;
        self.handle.spawn(async move {
            n.unirse_tor(port).await;
        });
    }

    pub fn entrar_en_sala(&self, mandante: bool) {
        self.inner.lock().unwrap().rol_sala = Some(mandante);
    }

    async fn unirse_tor(&self, local_port: u16) {
        let tor = self.inner.lock().unwrap().tor.clone();
        if let Err(e) = tor.subir(local_port).await {
            tor.marcar_fallo(e.to_string());
            return;
        }
        if let Some(a) = tor.onion_addr() {
            self.inner.lock().unwrap().addr = a;
        }
        let mandante = loop {
            if let Some(m) = self.inner.lock().unwrap().rol_sala {
                break m;
            }
            tor.marcar_arrancando("tor listo, esperá a entrar");
            tokio::time::sleep(Duration::from_millis(400)).await;
        };
        if mandante {
            tor.marcar_arrancando("abriendo sala");
            if let Err(e) = tor.hospedar_sala(local_port).await {
                tor.marcar_fallo(format!("sala: {e}"));
                return;
            }
            for s in (0..30).rev() {
                tor.marcar_arrancando(format!("publicando sala ({s}s)"));
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            loop {
                if self.n_peers() > 0 {
                    tor.marcar_listo();
                } else {
                    tor.marcar_arrancando("sala abierta, esperando al contratista");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        } else {
            self.marcar_sala(&tor).await;
        }
    }

    async fn marcar_sala(&self, tor: &Tor) {
        let mut n = 1u32;
        loop {
            if self.n_peers() > 0 {
                tor.marcar_listo();
                return;
            }
            tor.marcar_arrancando(format!("buscando sala ({n})"));
            match crate::tor::dial_rendezvous(tor).await {
                Ok(stream) => {
                    tor.marcar_listo();
                    let _ = self.sesion_out(stream).await;
                    if self.n_peers() > 0 {
                        tor.marcar_listo();
                        return;
                    }
                }
                Err(e) => {
                    tor.marcar_arrancando(format!("buscando sala ({})", Self::corto_err(&e)));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
            n += 1;
        }
    }

    pub fn parar(&self) {
        let _ = self.inner.lock().unwrap().halt.send(true);
    }

    fn corto_err(e: &std::io::Error) -> String {
        let s = e.to_string();
        if s.contains("timeout") || s.contains("elapsed") {
            "sin respuesta".into()
        } else if s.contains("HostUnreachable")
            || s.contains("NetworkUnreachable")
            || s.contains("ttl")
            || s.contains("TTL")
        {
            "sala aún no visible".into()
        } else {
            s.chars().take(42).collect()
        }
    }

    pub fn estado_tor(&self) -> EstadoTor {
        self.inner.lock().unwrap().tor.estado()
    }

    pub fn n_peers(&self) -> usize {
        self.inner.lock().unwrap().peers.len()
    }

    /// Contractor "Buscar ofertas": gossip plus a dial to the baked room
    /// if we are not the porter (dialing our own onion is a no-op).
    pub fn buscar(&self) {
        self.spawn_gossip();
        let tor = self.inner.lock().unwrap().tor.clone();
        if tor.hospeda_sala() || tor.socks().is_none() {
            return;
        }
        let n = self.clone();
        self.handle.spawn(async move {
            if let Ok(stream) = crate::tor::dial_rendezvous(&tor).await {
                let _ = n.sesion_out(stream).await;
            }
        });
    }

    pub fn publicar(&self, oferta: Oferta) {
        let key = key_hex(&clave_tablero());
        {
            let mut g = self.inner.lock().unwrap();
            let mut list = g
                .store
                .get(&key)
                .map(|b| decode_tablero(b))
                .unwrap_or_default();
            list.retain(|o| o.id != oferta.id);
            list.insert(0, oferta);
            g.store.insert(key, encode_tablero(&list));
        }
        self.spawn_gossip();
    }

    pub fn tablero(&self) -> Vec<Oferta> {
        let key = key_hex(&clave_tablero());
        let g = self.inner.lock().unwrap();
        g.store
            .get(&key)
            .map(|b| decode_tablero(b))
            .unwrap_or_default()
    }

    pub fn quitar(&self, oferta_id: &str) {
        let key = key_hex(&clave_tablero());
        {
            let mut g = self.inner.lock().unwrap();
            let mut list = g
                .store
                .get(&key)
                .map(|b| decode_tablero(b))
                .unwrap_or_default();
            list.retain(|o| o.id != oferta_id);
            g.store.insert(key, encode_tablero(&list));
        }
        self.spawn_gossip();
    }

    pub fn publicar_obra(&self, obra: Obra) {
        let key = key_hex(&clave_obras());
        {
            let mut g = self.inner.lock().unwrap();
            let mut list = g
                .store
                .get(&key)
                .map(|b| decode_obras(b))
                .unwrap_or_default();
            list.retain(|o| o.id != obra.id);
            list.insert(0, obra);
            g.store.insert(key, encode_obras(&list));
        }
        self.spawn_gossip();
    }

    pub fn obras(&self) -> Vec<Obra> {
        let key = key_hex(&clave_obras());
        let g = self.inner.lock().unwrap();
        g.store
            .get(&key)
            .map(|b| decode_obras(b))
            .unwrap_or_default()
    }

    pub fn anunciar(&self, persona: Persona) {
        let key = key_hex(&clave_presentes());
        {
            let mut g = self.inner.lock().unwrap();
            let mut list = g
                .store
                .get(&key)
                .map(|b| decode_presentes(b))
                .unwrap_or_default();
            list.retain(|p| p.id != persona.id);
            list.push(persona);
            g.store.insert(key, encode_presentes(&list));
        }
        self.spawn_gossip();
    }

    pub fn hidratar(&self, ofertas: Vec<Oferta>, obras: Vec<Obra>, presentes: Vec<Persona>) {
        {
            let mut g = self.inner.lock().unwrap();
            if !ofertas.is_empty() {
                g.store
                    .insert(key_hex(&clave_tablero()), encode_tablero(&ofertas));
            }
            if !obras.is_empty() {
                g.store.insert(key_hex(&clave_obras()), encode_obras(&obras));
            }
            if !presentes.is_empty() {
                g.store
                    .insert(key_hex(&clave_presentes()), encode_presentes(&presentes));
            }
        }
        self.spawn_gossip();
    }

    pub fn presentes(&self) -> Vec<Persona> {
        let key = key_hex(&clave_presentes());
        let g = self.inner.lock().unwrap();
        g.store
            .get(&key)
            .map(|b| decode_presentes(b))
            .unwrap_or_default()
    }

    pub async fn esperar(&self, d: Duration) {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        self.handle.spawn(async move {
            tokio::time::sleep(d).await;
            let _ = tx.send(());
        });
        let _ = rx.await;
    }

    fn spawn_gossip(&self) {
        let n = self.clone();
        self.handle.spawn(async move {
            n.gossip().await;
        });
    }

    fn puts(&self) -> Vec<Msg> {
        let g = self.inner.lock().unwrap();
        g.store
            .iter()
            .map(|(k, v)| Msg::Put {
                key: k.clone(),
                val: v.clone(),
            })
            .collect()
    }

    async fn gossip(&self) {
        let (peers, puts, port, bootstrap) = {
            let g = self.inner.lock().unwrap();
            let port = match &g.addr {
                PeerAddr::Tcp { port, .. } | PeerAddr::Onion { port, .. } => *port,
            };
            let peers: Vec<_> = g.peers.values().cloned().collect();
            let puts: Vec<_> = g
                .store
                .iter()
                .map(|(k, v)| Msg::Put {
                    key: k.clone(),
                    val: v.clone(),
                })
                .collect();
            (peers, puts, port, g.bootstrap)
        };
        if peers.is_empty() && port != bootstrap {
            let n = self.clone();
            self.handle.spawn(async move {
                let _ = n
                    .dial(PeerAddr::Tcp {
                        host: "127.0.0.1".into(),
                        port: bootstrap,
                    })
                    .await;
            });
        }
        for addr in peers {
            for msg in &puts {
                let _ = self.send(&addr, msg).await;
            }
        }
    }

    async fn dial(&self, addr: PeerAddr) -> std::io::Result<()> {
        let stream = self.connect(&addr).await?;
        self.sesion_out(stream).await
    }

    async fn connect(&self, addr: &PeerAddr) -> std::io::Result<TcpStream> {
        let tor = self.inner.lock().unwrap().tor.clone();
        match addr {
            PeerAddr::Tcp { host, port } => TcpStream::connect((host.as_str(), *port)).await,
            PeerAddr::Onion { host, port } => tor.conectar(host, *port).await,
        }
    }

    async fn send(&self, addr: &PeerAddr, msg: &Msg) -> std::io::Result<()> {
        let mut s = self.connect(addr).await?;
        write_msg(&mut s, msg).await
    }

    async fn sesion_out(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let (id, addr) = {
            let g = self.inner.lock().unwrap();
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
        for put in self.puts() {
            write_msg(&mut stream, &put).await?;
        }
        self.leer_loop(&mut stream).await
    }

    async fn sesion_in(&self, mut stream: TcpStream) -> std::io::Result<()> {
        self.leer_loop(&mut stream).await
    }

    async fn leer_loop(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        loop {
            let msg = match read_msg(stream).await {
                Ok(m) => m,
                Err(_) => break,
            };
            for reply in self.handle(msg) {
                if write_msg(stream, &reply).await.is_err() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn handle(&self, msg: Msg) -> Vec<Msg> {
        match msg {
            Msg::Hola { node, addr, swarm } => {
                if swarm != RED {
                    return Vec::new();
                }
                let mut g = self.inner.lock().unwrap();
                if node != g.id {
                    g.peers.insert(node, addr);
                }
                let list: Vec<_> = g
                    .peers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .chain(std::iter::once((g.id.clone(), g.addr.clone())))
                    .collect();
                let puts: Vec<Msg> = g
                    .store
                    .iter()
                    .map(|(k, v)| Msg::Put {
                        key: k.clone(),
                        val: v.clone(),
                    })
                    .collect();
                drop(g);
                let mut out = vec![Msg::Peers { list }];
                out.extend(puts);
                out
            }
            Msg::Peers { list } => {
                let mut g = self.inner.lock().unwrap();
                let me = g.id.clone();
                for (id, addr) in list {
                    if id != me {
                        g.peers.insert(id, addr);
                    }
                }
                Vec::new()
            }
            Msg::Put { key, val } => {
                let mut g = self.inner.lock().unwrap();
                merge_store(&mut g.store, key, val);
                Vec::new()
            }
            Msg::Get { key } => {
                let g = self.inner.lock().unwrap();
                vec![Msg::Got {
                    val: g.store.get(&key).cloned(),
                    key,
                }]
            }
            Msg::Got { key, val } => {
                if let Some(val) = val {
                    let mut g = self.inner.lock().unwrap();
                    merge_store(&mut g.store, key, val);
                }
                Vec::new()
            }
            Msg::Ping => vec![Msg::Pong],
            Msg::Pong => Vec::new(),
        }
    }
}

fn merge_store(store: &mut HashMap<String, Vec<u8>>, key: String, val: Vec<u8>) {
    if key == key_hex(&crate::clave_tablero()) {
        let mut a = store
            .get(&key)
            .map(|b| decode_tablero(b))
            .unwrap_or_default();
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
            if let Some(ex) = a.iter_mut().find(|x| x.id == o.id) {
                ex.fusionar(o);
            } else {
                a.push(o);
            }
        }
        store.insert(key, encode_obras(&a));
    } else if key == key_hex(&crate::clave_presentes()) {
        let mut a = store
            .get(&key)
            .map(|b| decode_presentes(b))
            .unwrap_or_default();
        let b = decode_presentes(&val);
        for p in b {
            a.retain(|x| x.id != p.id);
            a.push(p);
        }
        store.insert(key, encode_presentes(&a));
    } else {
        store.entry(key).or_insert(val);
    }
}

async fn bind_local(bootstrap: u16) -> std::io::Result<(TcpListener, u16)> {
    match TcpListener::bind(("127.0.0.1", bootstrap)).await {
        Ok(l) => Ok((l, bootstrap)),
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
        let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec![]).unwrap();
        let id = o.id.clone();
        let key = key_hex(&crate::clave_tablero());
        merge_store(&mut store, key.clone(), encode_tablero(&[o]));
        let tab = decode_tablero(store.get(&key).unwrap());
        assert_eq!(tab[0].id, id);
        assert_eq!(tab[0].n_partidas_sugeridas, 5);
    }

    fn puerto_libre() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dos_nodos_ven_oferta_y_nombre() {
        let bootstrap = puerto_libre();
        let a = Nodo::arrancar_en(bootstrap).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let b = Nodo::arrancar_en(bootstrap).await.unwrap();

        let jose = Persona::nueva("José").unwrap();
        let juan = Persona::nueva("Juan").unwrap();
        a.anunciar(jose.clone());
        b.anunciar(juan.clone());
        a.publicar(Oferta::publicar(jose.clone(), "Casa El Quisco", 10_000, 2_000, vec![]).unwrap());

        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        loop {
            let a_peers = a.n_peers();
            let b_peers = b.n_peers();
            let tab = b.tablero();
            let nombres: Vec<_> = b.presentes().into_iter().map(|p| p.nombre).collect();
            if a_peers >= 1
                && b_peers >= 1
                && tab.iter().any(|o| o.nombre == "Casa El Quisco")
                && nombres.iter().any(|n| n == "José")
            {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!(
                    "no se vieron: a_peers={a_peers} b_peers={b_peers} tab={} presentes={nombres:?}",
                    tab.len()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        a.parar();
        b.parar();
    }
}
