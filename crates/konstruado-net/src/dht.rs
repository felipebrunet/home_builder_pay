use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::timeout;
use uuid::Uuid;

use konstruado_core::{ahora, Oferta, Obra, Persona};

use crate::proto::{
    decode_obras, decode_presentes, decode_tablero, encode_obras, encode_presentes, encode_tablero,
    key_hex, CajaMsg, Msg, PeerAddr,
};
use crate::rendezvous::{RENDEZVOUS_ONION, VIRT_PORT};
use crate::tor::{EstadoTor, Tor};
use crate::{clave_obras, clave_presentes, clave_tablero, PUERTO_LOCAL, RED};

/// Keepalive de una sesión que abrimos nosotros.
const PING: Duration = Duration::from_secs(20);
/// Sin nada leído en este tiempo, la sesión se cierra.
const INACTIVO: Duration = Duration::from_secs(90);
/// Cada cuánto se empuja el almacén por una sesión viva.
const EMPUJE: Duration = Duration::from_secs(2);
/// Tope del buzón por nodo.
const MAX_BUZON: usize = 64;
/// Tope de saltos de relay para la caja.
const MAX_SALTOS: u8 = 3;

/// Una sesión abierta con otro nodo. Se le puede escribir sin marcar.
struct Vivo {
    sid: u64,
    tx: UnboundedSender<Msg>,
    ultimo_put: Option<Instant>,
}

enum Ruta {
    Vivo(UnboundedSender<Msg>),
    Marcar(PeerAddr),
    Nada,
}

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
    /// Unix time of last inbound dump (Hola/Put/Peers from a peer).
    sync_at: i64,
    /// Persona local. La caja solo se acepta si `para` coincide.
    persona: Option<String>,
    /// Persona anunciada → id de nodo.
    personas: HashMap<String, String>,
    /// Bandeja de la caja. No se replica.
    caja: Vec<CajaMsg>,
    /// Sesiones abiertas por nodo remoto.
    vivos: HashMap<String, Vivo>,
    /// Mensajes para nodos sin sesión ni dirección marcable.
    buzon: HashMap<String, Vec<Msg>>,
    sesion_seq: u64,
    /// Celular: sin dirección entrante, solo sesiones vivas y destinos fijos.
    movil: bool,
    destinos: HashSet<PeerAddr>,
    /// Obras que salí solo en este equipo: no se muestran aunque el peer las gossipée.
    obras_salidas: HashSet<String>,
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
        Self::montar(tokio::runtime::Handle::current(), bootstrap, false).await
    }

    /// Celular: no lanza `tor`. Si hay SOCKS (Orbot), marca la sala horneada y
    /// mantiene la sesión abierta; el otro lado le empuja por ahí. `destinos`
    /// son extras por TCP (emulador `10.0.2.2:17432`, `adb reverse`, pruebas).
    pub async fn arrancar_movil(
        socks: Option<(String, u16)>,
        destinos: Vec<PeerAddr>,
    ) -> std::io::Result<Self> {
        Self::arrancar_movil_en(PUERTO_LOCAL, socks, destinos).await
    }

    pub async fn arrancar_movil_en(
        bootstrap: u16,
        socks: Option<(String, u16)>,
        destinos: Vec<PeerAddr>,
    ) -> std::io::Result<Self> {
        let n = Self::montar(tokio::runtime::Handle::current(), bootstrap, true).await?;
        if let Some((host, port)) = socks {
            n.configurar_socks(&host, port);
            n.agregar_destino(PeerAddr::Onion {
                host: RENDEZVOUS_ONION.into(),
                port: VIRT_PORT,
            });
        }
        for d in destinos {
            n.agregar_destino(d);
        }
        Ok(n)
    }

    /// Cambia el SOCKS externo (Orbot). No lanza `tor`.
    pub fn configurar_socks(&self, host: &str, port: u16) {
        let tor = self.inner.lock().unwrap().tor.clone();
        tor.configurar_socks(host, port);
        tor.marcar_arrancando("Orbot: buscando sala");
    }

    pub fn socks(&self) -> Option<std::net::SocketAddr> {
        self.inner.lock().unwrap().tor.socks()
    }

    /// Mantiene una sesión saliente con `destino`: si se corta, vuelve a marcar.
    pub fn agregar_destino(&self, destino: PeerAddr) {
        if !destino.marcable() {
            return;
        }
        if !self.inner.lock().unwrap().destinos.insert(destino.clone()) {
            return;
        }
        let n = self.clone();
        self.handle.spawn(async move {
            n.mantener(destino).await;
        });
    }

    pub fn quitar_destino(&self, destino: &PeerAddr) {
        self.inner.lock().unwrap().destinos.remove(destino);
    }

    pub fn destinos(&self) -> Vec<PeerAddr> {
        self.inner.lock().unwrap().destinos.iter().cloned().collect()
    }

    pub fn n_vivos(&self) -> usize {
        self.inner.lock().unwrap().vivos.len()
    }

    async fn mantener(&self, destino: PeerAddr) {
        let es_sala = matches!(&destino, PeerAddr::Onion { host, .. } if host == RENDEZVOUS_ONION);
        let mut halt = self.inner.lock().unwrap().halt.subscribe();
        let mut n = 1u32;
        loop {
            if *halt.borrow() || !self.inner.lock().unwrap().destinos.contains(&destino) {
                return;
            }
            let tor = self.inner.lock().unwrap().tor.clone();
            if es_sala && tor.socks().is_none() {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            if es_sala && self.n_vivos() == 0 {
                tor.marcar_arrancando(format!("Orbot: buscando sala ({n})"));
            }
            let espera = match timeout(Duration::from_secs(60), self.connect(&destino)).await {
                Ok(Ok(stream)) => {
                    if es_sala {
                        tor.marcar_listo();
                    }
                    tokio::select! {
                        _ = halt.changed() => return,
                        _ = self.sesion_out(stream) => {}
                    }
                    Duration::from_secs(2)
                }
                Ok(Err(e)) => {
                    if es_sala && self.n_vivos() == 0 {
                        tor.marcar_arrancando(format!("Orbot: buscando sala ({})", Self::corto_err(&e)));
                    }
                    Duration::from_secs(4)
                }
                Err(_) => {
                    if es_sala && self.n_vivos() == 0 {
                        tor.marcar_arrancando("Orbot: buscando sala (sin respuesta)");
                    }
                    Duration::from_secs(4)
                }
            };
            n = n.saturating_add(1);
            tokio::select! {
                _ = halt.changed() => return,
                _ = tokio::time::sleep(espera) => {}
            }
        }
    }

    async fn montar(
        handle: tokio::runtime::Handle,
        bootstrap: u16,
        movil: bool,
    ) -> std::io::Result<Self> {
        let tor = Tor::ausente();
        let id = Uuid::new_v4().to_string();
        let (listener, port) = bind_local(bootstrap).await?;
        let addr = if movil {
            PeerAddr::Buzon { node: id.clone() }
        } else {
            PeerAddr::Tcp {
                host: "127.0.0.1".into(),
                port,
            }
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
                sync_at: 0,
                persona: None,
                personas: HashMap::new(),
                caja: Vec::new(),
                vivos: HashMap::new(),
                buzon: HashMap::new(),
                sesion_seq: 0,
                movil,
                destinos: HashSet::new(),
                obras_salidas: HashSet::new(),
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
        // Una PC contratista puede hospedar la sala para un celular mandante.
        let forzar = std::env::var("KONSTRUADO_HOSPEDAR_SALA").is_ok_and(|v| v == "1");
        self.inner.lock().unwrap().rol_sala = Some(mandante || forzar);
    }

    async fn unirse_tor(&self, local_port: u16) {
        let tor = self.inner.lock().unwrap().tor.clone();
        if let Err(e) = tor.subir(local_port).await {
            tor.marcar_fallo(e.to_string());
            return;
        }
        if let Some(a) = tor.onion_addr() {
            let mut g = self.inner.lock().unwrap();
            if !g.movil {
                g.addr = a;
            }
        }
        loop {
            let mandante = loop {
                if let Some(m) = self.inner.lock().unwrap().rol_sala {
                    break m;
                }
                tor.marcar_arrancando("tor listo, esperá a entrar");
                tokio::time::sleep(Duration::from_millis(400)).await;
            };
            if mandante {
                if !tor.hospeda_sala() {
                    tor.marcar_arrancando("abriendo sala");
                    if let Err(e) = tor.hospedar_sala(local_port).await {
                        tor.marcar_fallo(format!("sala: {e}"));
                        return;
                    }
                    for s in (0..30).rev() {
                        if self.inner.lock().unwrap().rol_sala != Some(true) {
                            break;
                        }
                        tor.marcar_arrancando(format!("publicando sala ({s}s)"));
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                loop {
                    if self.inner.lock().unwrap().rol_sala != Some(true) {
                        let _ = tor.dejar_sala().await;
                        break;
                    }
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
    }

    async fn marcar_sala(&self, tor: &Tor) {
        let mut n = 1u32;
        loop {
            if self.inner.lock().unwrap().rol_sala == Some(true) {
                return;
            }
            if self.n_peers() > 0 {
                tor.marcar_listo();
            } else {
                tor.marcar_arrancando(format!("buscando sala ({n})"));
            }
            match crate::tor::dial_rendezvous(tor).await {
                Ok(stream) => {
                    tor.marcar_listo();
                    let _ = timeout(Duration::from_secs(12), self.sesion_out(stream)).await;
                    tokio::time::sleep(Duration::from_secs(4)).await;
                }
                Err(e) => {
                    if self.n_peers() == 0 {
                        tor.marcar_arrancando(format!("buscando sala ({})", Self::corto_err(&e)));
                    }
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

    /// La persona de esta ventana. Sin esto, la caja que llega se descarta.
    pub fn fijar_persona(&self, id: &str) {
        if id.is_empty() {
            return;
        }
        self.inner.lock().unwrap().persona = Some(id.to_string());
    }

    /// Avisa la persona a los pares que ya conocemos. No se guarda en el DHT.
    pub fn anunciar_persona(&self) {
        let (persona, node, peers, movil) = {
            let g = self.inner.lock().unwrap();
            let Some(persona) = g.persona.clone() else {
                return;
            };
            let peers: Vec<_> = g.peers.keys().cloned().collect();
            (persona, g.id.clone(), peers, g.movil)
        };
        if peers.is_empty() {
            return;
        }
        let msg = Msg::Soy { node, persona };
        let mut marcar = Vec::new();
        for peer in peers {
            match self.ruta(&peer) {
                Ruta::Vivo(tx) => {
                    let _ = tx.send(msg.clone());
                }
                Ruta::Marcar(addr) if !movil => marcar.push(addr),
                _ => {}
            }
        }
        if marcar.is_empty() {
            return;
        }
        let n = self.clone();
        self.handle.spawn(async move {
            for addr in marcar {
                let _ = n.send(&addr, &msg).await;
            }
        });
    }

    /// Manda bytes de la caja solo al nodo que anunció `para`. False si todavía no lo vimos.
    ///
    /// Sin dirección marcable (celular), va por la sesión viva o por un relay.
    pub fn enviar_caja(&self, obra: &str, para: &str, de: &str, paso: &str, cuerpo: &[u8]) -> bool {
        let (node, relay) = {
            let g = self.inner.lock().unwrap();
            let Some(node) = g.personas.get(para).cloned() else {
                return false;
            };
            if !g.peers.contains_key(&node) && !g.vivos.contains_key(&node) {
                return false;
            }
            let relay = g
                .vivos
                .iter()
                .filter(|(k, _)| **k != node)
                .map(|(_, v)| v.tx.clone())
                .next();
            (node, relay)
        };
        let msg = Msg::Caja {
            obra: obra.to_string(),
            para: para.to_string(),
            de: de.to_string(),
            paso: paso.to_string(),
            cuerpo: hex::encode(cuerpo),
            saltos: 0,
        };
        match self.ruta(&node) {
            Ruta::Vivo(tx) => tx.send(msg).is_ok(),
            Ruta::Marcar(addr) => {
                let n = self.clone();
                self.handle.spawn(async move {
                    if n.send(&addr, &msg).await.is_err() {
                        n.encolar(&node, msg);
                    }
                });
                true
            }
            Ruta::Nada => match relay {
                Some(tx) => tx.send(msg).is_ok(),
                None => false,
            },
        }
    }

    fn ruta(&self, node: &str) -> Ruta {
        let g = self.inner.lock().unwrap();
        if let Some(v) = g.vivos.get(node) {
            return Ruta::Vivo(v.tx.clone());
        }
        match g.peers.get(node) {
            Some(a) if a.marcable() => Ruta::Marcar(a.clone()),
            _ => Ruta::Nada,
        }
    }

    fn encolar(&self, node: &str, msg: Msg) {
        let mut g = self.inner.lock().unwrap();
        encolar_en(&mut g.buzon, node, msg);
    }

    pub fn tomar_caja(&self) -> Vec<CajaMsg> {
        let mut g = self.inner.lock().unwrap();
        std::mem::take(&mut g.caja)
    }

    pub fn conoce_persona(&self, id: &str) -> bool {
        let g = self.inner.lock().unwrap();
        g.personas
            .get(id)
            .is_some_and(|node| g.peers.contains_key(node) || g.vivos.contains_key(node))
    }

    pub fn sesion_viva(&self, _yo_id: &str, otro_id: &str) -> bool {
        if self.n_peers() == 0 {
            return false;
        }
        let t = ahora();
        self.presentes().iter().any(|p| {
            p.id == otro_id && t.saturating_sub(p.visto) <= 25
        })
    }

    pub fn sync_reciente(&self) -> bool {
        let t = ahora();
        let sync = self.inner.lock().unwrap().sync_at;
        sync > 0 && t.saturating_sub(sync) <= 30
    }

    /// Catch-up before a deal blow: live counterparty and a recent dump.
    pub fn trato_alineado(&self, yo_id: &str, otro_id: &str) -> bool {
        self.sesion_viva(yo_id, otro_id) && self.sync_reciente()
    }

    fn marcar_sync(&self) {
        self.inner.lock().unwrap().sync_at = ahora();
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
            g.obras_salidas.remove(&obra.id);
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
            .into_iter()
            .filter(|o| !g.obras_salidas.contains(&o.id))
            .collect()
    }

    /// Ids de obras ocultas por salida local (para persistir).
    pub fn obras_salidas(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .obras_salidas
            .iter()
            .cloned()
            .collect()
    }

    /// Carga la lista de salidas locales al arrancar.
    pub fn fijar_obras_salidas(&self, ids: Vec<String>) {
        let mut g = self.inner.lock().unwrap();
        g.obras_salidas = ids.into_iter().collect();
    }

    /// Quita la obra de este equipo y la marca para que gossip no la devuelva.
    /// No mueve fondos ni publica un estado Abandonada al peer.
    pub fn salir_obra_local(&self, obra_id: &str) {
        let mut g = self.inner.lock().unwrap();
        g.obras_salidas.insert(obra_id.to_string());
        let key = key_hex(&clave_obras());
        let mut list = g
            .store
            .get(&key)
            .map(|b| decode_obras(b))
            .unwrap_or_default();
        list.retain(|o| o.id != obra_id);
        g.store.insert(key, encode_obras(&list));
    }

    /// Al reimportar un respaldo, la obra vuelve a ser visible.
    pub fn olvidar_salida_obra(&self, obra_id: &str) {
        self.inner.lock().unwrap().obras_salidas.remove(obra_id);
    }

    pub fn actualizar_yo(&self, persona: Persona) {
        let id = persona.id.clone();
        {
            let mut g = self.inner.lock().unwrap();
            let kt = key_hex(&clave_tablero());
            let mut tab = g
                .store
                .get(&kt)
                .map(|b| decode_tablero(b))
                .unwrap_or_default();
            for o in &mut tab {
                if o.mandante.id == id {
                    o.mandante = persona.clone();
                }
            }
            g.store.insert(kt, encode_tablero(&tab));
            let ko = key_hex(&clave_obras());
            let mut obras = g
                .store
                .get(&ko)
                .map(|b| decode_obras(b))
                .unwrap_or_default();
            for o in &mut obras {
                if o.mandante.id == id {
                    o.mandante = persona.clone();
                }
                if o.contratista.id == id {
                    o.contratista = persona.clone();
                }
            }
            g.store.insert(ko, encode_obras(&obras));
        }
        self.anunciar(persona);
    }

    pub fn anunciar(&self, mut persona: Persona) {
        persona.visto = ahora();
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
        g.store.iter().map(|(k, v)| put_de(k, v)).collect()
    }

    async fn gossip(&self) {
        let (peers, puts, port, bootstrap) = {
            let mut g = self.inner.lock().unwrap();
            let port = match &g.addr {
                PeerAddr::Tcp { port, .. } | PeerAddr::Onion { port, .. } => *port,
                PeerAddr::Buzon { .. } => g.local_port,
            };
            let puts: Vec<_> = g.store.iter().map(|(k, v)| put_de(k, v)).collect();
            // Por las sesiones vivas se empuja cada EMPUJE, sin marcar. También
            // la lista de pares: así un celular conoce a otro que llegó después.
            let lista: Vec<_> = g
                .peers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .chain(std::iter::once((g.id.clone(), g.addr.clone())))
                .collect();
            let mut puts_vivos = vec![Msg::Peers { list: lista }];
            puts_vivos.extend(puts.iter().cloned());
            let ahora_i = Instant::now();
            for v in g.vivos.values_mut() {
                if v.ultimo_put.is_none_or(|t| ahora_i.duration_since(t) >= EMPUJE) {
                    v.ultimo_put = Some(ahora_i);
                    for m in &puts_vivos {
                        let _ = v.tx.send(m.clone());
                    }
                }
            }
            let movil = g.movil;
            let peers: Vec<_> = g
                .peers
                .iter()
                .filter(|(k, a)| !movil && a.marcable() && !g.vivos.contains_key(*k))
                .map(|(_, a)| a.clone())
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
            PeerAddr::Buzon { .. } => Err(std::io::Error::other("sin dirección entrante")),
        }
    }

    async fn send(&self, addr: &PeerAddr, msg: &Msg) -> std::io::Result<()> {
        let mut s = self.connect(addr).await?;
        write_msg(&mut s, msg).await
    }

    async fn sesion_out(&self, stream: TcpStream) -> std::io::Result<()> {
        let (id, addr) = {
            let g = self.inner.lock().unwrap();
            (g.id.clone(), g.addr.clone())
        };
        let mut primeros = vec![Msg::Hola {
            node: id,
            addr,
            swarm: RED.into(),
        }];
        primeros.extend(self.puts());
        self.leer_loop(stream, false, primeros).await
    }

    async fn sesion_in(&self, stream: TcpStream) -> std::io::Result<()> {
        self.leer_loop(stream, true, Vec::new()).await
    }

    /// Lee y escribe por la misma conexión. Cuando el otro se presenta, la
    /// sesión queda registrada como viva: se le puede empujar sin marcar.
    async fn leer_loop(
        &self,
        stream: TcpStream,
        entrante: bool,
        primeros: Vec<Msg>,
    ) -> std::io::Result<()> {
        let (mut rd, mut wr) = stream.into_split();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        for m in primeros {
            let _ = tx.send(m);
        }
        let escritor = self.handle.spawn(async move {
            while let Some(m) = rx.recv().await {
                if write_msg(&mut wr, &m).await.is_err() {
                    break;
                }
            }
        });
        let pinger = if entrante {
            None
        } else {
            let txp = tx.clone();
            Some(self.handle.spawn(async move {
                loop {
                    tokio::time::sleep(PING).await;
                    if txp.send(Msg::Ping).is_err() {
                        break;
                    }
                }
            }))
        };
        let (sid, mut halt) = {
            let mut g = self.inner.lock().unwrap();
            g.sesion_seq += 1;
            (g.sesion_seq, g.halt.subscribe())
        };
        // La limpieza vive en un guard: si el futuro se cancela (halt en
        // `mantener`), igual se cierran escritor/pinger y se suelta el vivo.
        struct Guard {
            inner: Arc<Mutex<Inner>>,
            sid: u64,
            remoto: Option<String>,
            tareas: Vec<tokio::task::JoinHandle<()>>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                if let Some(r) = self.remoto.take() {
                    if let Ok(mut g) = self.inner.lock() {
                        if g.vivos.get(&r).is_some_and(|v| v.sid == self.sid) {
                            g.vivos.remove(&r);
                        }
                    }
                }
                for t in self.tareas.drain(..) {
                    t.abort();
                }
            }
        }
        let mut guard = Guard {
            inner: self.inner.clone(),
            sid,
            remoto: None,
            tareas: std::iter::once(escritor).chain(pinger).collect(),
        };
        let mut remoto: Option<String> = None;
        loop {
            let leido = tokio::select! {
                _ = halt.changed() => break,
                r = timeout(INACTIVO, read_msg(&mut rd)) => r,
            };
            let msg = match leido {
                Ok(Ok(m)) => m,
                _ => break,
            };
            if remoto.is_none() {
                if let Msg::Hola { node, .. } = &msg {
                    remoto = self.registrar_vivo(node, sid, &tx, entrante);
                    guard.remoto = remoto.clone();
                }
            }
            for reply in self.handle_de(msg, remoto.as_deref()) {
                if tx.send(reply).is_err() {
                    break;
                }
            }
        }
        drop(guard);
        Ok(())
    }

    /// Registra la sesión y le manda lo que estaba esperando: el saludo de
    /// vuelta (si entró), las personas conocidas y el buzón.
    fn registrar_vivo(
        &self,
        node: &str,
        sid: u64,
        tx: &UnboundedSender<Msg>,
        entrante: bool,
    ) -> Option<String> {
        let mut g = self.inner.lock().unwrap();
        if node == g.id {
            return None;
        }
        g.vivos.insert(
            node.to_string(),
            Vivo {
                sid,
                tx: tx.clone(),
                ultimo_put: None,
            },
        );
        let pendientes = g.buzon.remove(node).unwrap_or_default();
        let mut salida = Vec::new();
        if entrante {
            salida.push(Msg::Hola {
                node: g.id.clone(),
                addr: g.addr.clone(),
                swarm: RED.into(),
            });
        }
        if let Some(p) = g.persona.clone() {
            salida.push(Msg::Soy {
                node: g.id.clone(),
                persona: p,
            });
        }
        for (persona, n) in &g.personas {
            if n != node {
                salida.push(Msg::Soy {
                    node: n.clone(),
                    persona: persona.clone(),
                });
            }
        }
        drop(g);
        for m in salida.into_iter().chain(pendientes) {
            let _ = tx.send(m);
        }
        Some(node.to_string())
    }

    fn handle_de(&self, msg: Msg, origen: Option<&str>) -> Vec<Msg> {
        match msg {
            Msg::Soy { node, persona } => {
                if node.is_empty() || persona.is_empty() {
                    return Vec::new();
                }
                let mut g = self.inner.lock().unwrap();
                if node == g.id {
                    return Vec::new();
                }
                if g.personas.get(&persona) == Some(&node) {
                    return Vec::new();
                }
                g.personas.insert(persona.clone(), node.clone());
                // Relay: los demás que tienen sesión con nosotros también lo aprenden.
                let otros: Vec<_> = g
                    .vivos
                    .iter()
                    .filter(|(k, _)| Some(k.as_str()) != origen && **k != node)
                    .map(|(_, v)| v.tx.clone())
                    .collect();
                drop(g);
                for t in otros {
                    let _ = t.send(Msg::Soy {
                        node: node.clone(),
                        persona: persona.clone(),
                    });
                }
                Vec::new()
            }
            Msg::Caja {
                obra,
                para,
                de,
                paso,
                cuerpo,
                saltos,
            } => {
                let mut g = self.inner.lock().unwrap();
                if g.persona.as_deref() == Some(para.as_str()) {
                    if let Ok(bytes) = hex::decode(cuerpo) {
                        if bytes.len() <= 900_000 {
                            if g.caja.len() >= 32 {
                                g.caja.remove(0);
                            }
                            g.caja.push(CajaMsg {
                                obra,
                                de,
                                paso,
                                cuerpo: bytes,
                            });
                        }
                    }
                    return Vec::new();
                }
                // No es para esta persona: relay hacia el nodo que la anunció.
                if saltos >= MAX_SALTOS {
                    return Vec::new();
                }
                let Some(dest) = g.personas.get(&para).cloned() else {
                    return Vec::new();
                };
                if Some(dest.as_str()) == origen || dest == g.id {
                    return Vec::new();
                }
                let fwd = Msg::Caja {
                    obra,
                    para,
                    de,
                    paso,
                    cuerpo,
                    saltos: saltos + 1,
                };
                if let Some(v) = g.vivos.get(&dest) {
                    let _ = v.tx.send(fwd);
                } else if let Some(addr) = g.peers.get(&dest).filter(|a| a.marcable()).cloned() {
                    drop(g);
                    let n = self.clone();
                    self.handle.spawn(async move {
                        if n.send(&addr, &fwd).await.is_err() {
                            n.encolar(&dest, fwd);
                        }
                    });
                } else {
                    encolar_en(&mut g.buzon, &dest, fwd);
                }
                Vec::new()
            }
            other => self.handle(other),
        }
    }

    fn handle(&self, msg: Msg) -> Vec<Msg> {
        match msg {
            Msg::Hola { node, addr, swarm } => {
                if swarm != RED {
                    return Vec::new();
                }
                let mut g = self.inner.lock().unwrap();
                let foreign = node != g.id;
                if foreign {
                    g.peers.insert(node, addr);
                }
                let list: Vec<_> = g
                    .peers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .chain(std::iter::once((g.id.clone(), g.addr.clone())))
                    .collect();
                let puts: Vec<Msg> = g.store.iter().map(|(k, v)| put_de(k, v)).collect();
                drop(g);
                if foreign {
                    self.marcar_sync();
                }
                let mut out = vec![Msg::Peers { list }];
                out.extend(puts);
                out
            }
            Msg::Peers { list } => {
                let mut g = self.inner.lock().unwrap();
                let me = g.id.clone();
                let mut foreign = false;
                for (id, addr) in list {
                    if id != me {
                        g.peers.insert(id, addr);
                        foreign = true;
                    }
                }
                drop(g);
                if foreign {
                    self.marcar_sync();
                }
                Vec::new()
            }
            Msg::Put { key, val } => {
                let mut g = self.inner.lock().unwrap();
                merge_store(&mut g.store, key, val);
                drop(g);
                self.marcar_sync();
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
                    drop(g);
                    self.marcar_sync();
                }
                Vec::new()
            }
            Msg::Ping => vec![Msg::Pong],
            Msg::Pong => Vec::new(),
            Msg::Soy { .. } | Msg::Caja { .. } => Vec::new(),
        }
    }
}

fn encolar_en(buzon: &mut HashMap<String, Vec<Msg>>, node: &str, msg: Msg) {
    let cola = buzon.entry(node.to_string()).or_default();
    if cola.len() >= MAX_BUZON {
        cola.remove(0);
    }
    cola.push(msg);
}

fn put_de(key: &str, val: &[u8]) -> Msg {
    Msg::Put {
        key: key.to_string(),
        val: valor_para_red(key, val),
    }
}

/// El almacén local puede guardar la nota en claro. El anuncio no la lleva.
fn valor_para_red(key: &str, val: &[u8]) -> Vec<u8> {
    if key == key_hex(&clave_obras()) {
        let obras: Vec<_> = decode_obras(val)
            .into_iter()
            .map(|o| o.sin_texto_claro())
            .collect();
        return encode_obras(&obras);
    }
    val.to_vec()
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
            if let Some(ex) = a.iter_mut().find(|x| x.id == p.id) {
                if p.visto >= ex.visto {
                    *ex = p;
                }
            } else {
                a.push(p);
            }
        }
        store.insert(key, encode_presentes(&a));
    } else {
        store.entry(key).or_insert(val);
    }
}

async fn bind_local(bootstrap: u16) -> std::io::Result<(TcpListener, u16)> {
    // Pruebas en LAN (celular -> PC sin Orbot): KONSTRUADO_ESCUCHAR=0.0.0.0
    let host = std::env::var("KONSTRUADO_ESCUCHAR").unwrap_or_else(|_| "127.0.0.1".into());
    match TcpListener::bind((host.as_str(), bootstrap)).await {
        Ok(l) => Ok((l, bootstrap)),
        Err(_) => {
            let l = TcpListener::bind((host.as_str(), 0)).await?;
            let port = l.local_addr()?.port();
            Ok((l, port))
        }
    }
}

async fn write_msg<W: AsyncWrite + Unpin>(s: &mut W, msg: &Msg) -> std::io::Result<()> {
    let buf = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
    s.write_all(&len.to_be_bytes()).await?;
    s.write_all(&buf).await?;
    s.flush().await
}

async fn read_msg<R: AsyncRead + Unpin>(s: &mut R) -> std::io::Result<Msg> {
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
    use konstruado_core::{Aceptacion, ExtraPartida, NotaPartida, Oferta, Persona};

    #[test]
    fn el_anuncio_no_lleva_la_nota_en_claro() {
        let m = Persona::nueva("Alice").unwrap();
        let c = Persona::nueva("Bob").unwrap();
        let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec!["Muro".into()]).unwrap();
        let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
        let mut obra = Obra::desde_oferta(o, a).unwrap();
        obra.partidas[0].notas.push(NotaPartida {
            autor_id: c.id.clone(),
            autor_nombre: c.nombre.clone(),
            porcentaje: 80,
            texto: "Terminé el muro".into(),
            cuando: 0,
            caja: String::new(),
        });
        obra.extra = Some(ExtraPartida {
            detalle: "Techumbre secreta".into(),
            monto: 30,
            por: c,
            detalle_caja: String::new(),
        });
        let key = key_hex(&clave_obras());
        let out = valor_para_red(&key, &encode_obras(&[obra]));
        let texto = String::from_utf8(out.clone()).unwrap();
        assert!(!texto.contains("Terminé el muro"));
        assert!(!texto.contains("Techumbre secreta"));
        assert!(texto.contains("Muro"));
        let vuelta = decode_obras(&out);
        assert!(vuelta[0].partidas[0].notas[0].texto.is_empty());
        assert!(vuelta[0].extra.as_ref().unwrap().detalle.is_empty());
    }

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


    fn tcp(port: u16) -> PeerAddr {
        PeerAddr::Tcp {
            host: "127.0.0.1".into(),
            port,
        }
    }

    async fn hasta<F: FnMut() -> bool>(seg: u64, que: &str, mut f: F) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seg);
        while !f() {
            if tokio::time::Instant::now() > deadline {
                panic!("timeout: {que}");
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }

    /// Celular sin dirección entrante: la PC le empuja por la sesión que él abrió.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn celular_recibe_por_sesion_viva() {
        let pc_port = puerto_libre();
        let pc = Nodo::arrancar_en(pc_port).await.unwrap();
        let cel = Nodo::arrancar_movil_en(puerto_libre(), None, vec![tcp(pc_port)])
            .await
            .unwrap();
        let ana = Persona::nueva("Ana").unwrap();
        let beto = Persona::nueva("Beto").unwrap();
        pc.fijar_persona(&ana.id);
        cel.fijar_persona(&beto.id);
        pc.anunciar(ana.clone());
        cel.anunciar(beto.clone());
        hasta(6, "personas", || {
            pc.anunciar_persona();
            cel.anunciar_persona();
            pc.conoce_persona(&beto.id) && cel.conoce_persona(&ana.id)
        })
        .await;
        // La PC ve al celular como buzón, no como TCP.
        assert!(matches!(
            pc.inner.lock().unwrap().peers.values().find(|a| matches!(a, PeerAddr::Buzon { .. })),
            Some(_)
        ));
        assert!(pc.enviar_caja("obra-1", &beto.id, &ana.id, "dkg-commit", b"de-la-pc"));
        assert!(cel.enviar_caja("obra-1", &ana.id, &beto.id, "dkg-commit", b"del-cel"));
        hasta(6, "caja pc->cel", || {
            cel.tomar_caja().iter().any(|m| m.cuerpo == b"de-la-pc")
        })
        .await;
        hasta(6, "caja cel->pc", || pc.tomar_caja().iter().any(|m| m.cuerpo == b"del-cel")).await;
        pc.publicar(Oferta::publicar(ana.clone(), "Casa Quisco", 10_000, 2_000, vec![]).unwrap());
        hasta(8, "tablero en el cel", || {
            cel.tablero().iter().any(|o| o.nombre == "Casa Quisco")
        })
        .await;
        cel.publicar(Oferta::publicar(beto.clone(), "Galpon", 10_000, 2_000, vec![]).unwrap());
        hasta(8, "tablero en la pc", || pc.tablero().iter().any(|o| o.nombre == "Galpon")).await;
        pc.parar();
        cel.parar();
    }

    /// Dos celulares sin onion propio se hablan por la sala (relay).
    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn dos_celulares_por_relay() {
        let sala_port = puerto_libre();
        let sala = Nodo::arrancar_en(sala_port).await.unwrap();
        let a = Nodo::arrancar_movil_en(puerto_libre(), None, vec![tcp(sala_port)])
            .await
            .unwrap();
        let b = Nodo::arrancar_movil_en(puerto_libre(), None, vec![tcp(sala_port)])
            .await
            .unwrap();
        let ana = Persona::nueva("Ana").unwrap();
        let beto = Persona::nueva("Beto").unwrap();
        a.fijar_persona(&ana.id);
        b.fijar_persona(&beto.id);
        a.anunciar(ana.clone());
        b.anunciar(beto.clone());
        hasta(8, "personas por relay", || {
            a.anunciar_persona();
            b.anunciar_persona();
            a.conoce_persona(&beto.id) && b.conoce_persona(&ana.id)
        })
        .await;
        assert!(a.enviar_caja("obra-9", &beto.id, &ana.id, "fund-proposal", b"a-b"));
        assert!(b.enviar_caja("obra-9", &ana.id, &beto.id, "fund-skeleton", b"b-a"));
        hasta(6, "a->b", || b.tomar_caja().iter().any(|m| m.cuerpo == b"a-b" && m.de == ana.id)).await;
        hasta(6, "b->a", || a.tomar_caja().iter().any(|m| m.cuerpo == b"b-a")).await;
        // La sala no se queda con la caja.
        assert!(sala.tomar_caja().is_empty());
        a.publicar(Oferta::publicar(ana.clone(), "Casa Relay", 10_000, 2_000, vec![]).unwrap());
        hasta(10, "tablero a->b", || b.tablero().iter().any(|o| o.nombre == "Casa Relay")).await;
        sala.parar();
        a.parar();
        b.parar();
    }

    /// Si el celular se cae, la caja espera en el buzón del relay y llega al volver.
    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn buzon_entrega_al_volver() {
        let sala_port = puerto_libre();
        let sala = Nodo::arrancar_en(sala_port).await.unwrap();
        let a = Nodo::arrancar_movil_en(puerto_libre(), None, vec![tcp(sala_port)])
            .await
            .unwrap();
        let b = Nodo::arrancar_movil_en(puerto_libre(), None, vec![tcp(sala_port)])
            .await
            .unwrap();
        let ana = Persona::nueva("Ana").unwrap();
        let beto = Persona::nueva("Beto").unwrap();
        a.fijar_persona(&ana.id);
        b.fijar_persona(&beto.id);
        hasta(8, "personas", || {
            a.anunciar_persona();
            b.anunciar_persona();
            a.conoce_persona(&beto.id) && b.conoce_persona(&ana.id)
        })
        .await;
        // b corta su sesión con la sala; la sala lo saca de vivos.
        b.quitar_destino(&tcp(sala_port));
        b.parar();
        hasta(6, "sala suelta a b", || sala.n_vivos() == 1).await;
        assert!(a.enviar_caja("obra-2", &beto.id, &ana.id, "spend-open", b"guardado"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(sala.inner.lock().unwrap().buzon.values().any(|c| !c.is_empty()));
        sala.parar();
        a.parar();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_caja_llega_al_otro_y_no_al_dht() {
        let bootstrap = puerto_libre();
        let a = Nodo::arrancar_en(bootstrap).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        let b = Nodo::arrancar_en(bootstrap).await.unwrap();
        let jose = Persona::nueva("José").unwrap();
        let juan = Persona::nueva("Juan").unwrap();
        a.fijar_persona(&jose.id);
        b.fijar_persona(&juan.id);
        a.anunciar(jose.clone());
        b.anunciar(juan.clone());

        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        loop {
            a.anunciar_persona();
            b.anunciar_persona();
            if a.conoce_persona(&juan.id) && b.conoce_persona(&jose.id) {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("no se anunciaron las personas");
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        assert!(a.enviar_caja("obra-1", &juan.id, &jose.id, "dkg-commit", b"hola-caja"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        loop {
            let caja = b.tomar_caja();
            if caja.iter().any(|m| m.cuerpo == b"hola-caja" && m.paso == "dkg-commit" && m.de == jose.id)
            {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("la caja no llegó");
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        assert!(a.tomar_caja().is_empty());
        assert!(b.obras().is_empty());
        a.parar();
        b.parar();
    }
}
