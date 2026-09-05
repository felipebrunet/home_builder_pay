//! Native product window (egui/eframe). Not the throwaway `hbp-ui` localhost.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, RichText, Vec2};
use hbp_app::{
    apply_incoming_quote, apply_verified_p1_funding, build_our_partial, classify_funding_psbt,
    coin_from_watched, complete_incoming_partial, completed_steps, contratista_accept, coop_finish,
    coop_propose, coop_sign, default_works_root, draft_equal_stages, draft_quote, escrow_addrs,
    export_backup, format_unix_local_es, fund_handshake_step, funding_wire, funding_wire_hex,
    hex_artifact, hex_from_artifact, import_backup, import_signed, lock_quote_if_ready,
    mandante_commit, next_step, our_funding_need, parse_fee, parse_psbt, partida_ui_enabled,
    party_role, pay_stage, preview_quote_sats, price_minor_from_major, psbt_to_hex,
    quote_fully_signed, read_backup_file, sign_our_quote, spanish_now, suggest_watched,
    validate_deadline_order, write_backup_file, DeadlineFields, FundHandshakeStep, NextKind,
    PayStage, PayUiDraft, UiPrefs, WorkEntry, WorkProgress, WorkStore, ART_COOP, ART_ONESIG,
    ART_PARTIAL, ART_PSBT, ART_TX, MONTHS_ES,
};
use hbp_bitcoin::{
    address_at, default_esplora_urls, scan_watch, sign_body, CoopFile, Identity, OfferedCoin,
    WatchScan,
};
use hbp_core::{
    bond_minor, format_major_amount, parse_major_amount, Offer, Quote, Role, SignedContract, Unit,
    DEFAULT_BOND_BPS, PRODUCT_NETWORK,
};
use hbp_net::{
    announce_topics, bring_up_tor_with_hint, env_bootstrap_peers, esplora_address_txs,
    esplora_address_utxos, esplora_try_bases, esplora_tx_hex, find_dual_amount_txid, literal_topic,
    parse_bootstrap_list, preview_sats, quote_btc, FxQuote, NetMessage, OverlayConfig,
    OverlayHandle, PeerAddr, TorConfig, TorRuntime, WorkAnnounce,
};

enum JobEvent {
    Progress(String),
    ConnectDone(Result<TorRuntime, String>),
    AnnounceDone(Result<(String, String), String>),
    LookupDone(Result<Vec<WorkAnnounce>, String>),
    BootstrapDone(Result<usize, String>),
    DeliverDone(Result<String, String>),
    /// Background Hello / retry. Must not clear a user-facing `busy` job.
    QuietDeliver(Result<String, String>),
    FxDone(Result<FxQuote, String>),
    ScanDone(Result<(WatchScan, String), String>),
    ChainDone(Result<Option<String>, String>),
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_title("home_builder_pay"),
        ..Default::default()
    };
    eframe::run_native(
        "home_builder_pay",
        opts,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NetLight {
    Off,
    Connecting,
    Ok,
    Partial,
    Err,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MainTab {
    Obra,
    Buscar,
    Red,
    Trato,
    Pago,
}

struct App {
    store: WorkStore,
    prefs: UiPrefs,
    selected: Option<String>,
    last_slug: Option<String>,
    new_name: String,
    total_major: String,
    unit: Unit,
    t1: DeadlineFields,
    t2: DeadlineFields,
    stage_descs: String,
    accept_path: String,
    backup_path: String,
    xpub_draft: String,
    passphrase: String,
    log: String,
    overlay: Option<OverlayHandle>,
    tor_rt: Option<TorRuntime>,
    own_onion: String,
    peer_onion: String,
    bootstrap: String,
    lookup_name: String,
    last_error: String,
    net_light: NetLight,
    net_line: String,
    busy: Option<String>,
    job_tx: mpsc::Sender<JobEvent>,
    job_rx: mpsc::Receiver<JobEvent>,
    fx: Option<FxQuote>,
    fx_line: String,
    name_draft: String,
    last_log: String,
    log_hits: u32,
    change_profile: bool,
    picking_role: bool,
    tab: MainTab,
    help_open: bool,
    search_hits: Vec<WorkAnnounce>,
    pay: PayUiDraft,
    watch_scan: Option<WatchScan>,
    esplora_base: String,
    pay_chain_line: String,
    last_chain_poll: std::time::Instant,
    chain_busy: bool,
}

impl App {
    fn new() -> Self {
        let store = WorkStore::open(default_works_root()).unwrap_or_else(|_| WorkStore {
            root: default_works_root(),
            index: Default::default(),
        });
        let prefs = store.load_prefs();
        let picking_role = prefs.first_run();
        let start_tab = if prefs.role == Role::Contratista {
            MainTab::Buscar
        } else {
            MainTab::Obra
        };
        let (job_tx, job_rx) = mpsc::channel();
        Self {
            store,
            prefs,
            selected: None,
            last_slug: None,
            new_name: String::new(),
            total_major: "100".into(),
            unit: Unit::Usd,
            t1: DeadlineFields::days_from_now(7),
            t2: DeadlineFields::days_from_now(14),
            stage_descs: String::new(),
            accept_path: String::new(),
            backup_path: String::new(),
            xpub_draft: String::new(),
            passphrase: String::new(),
            log: "Signet. Sin árbitro. Si no hay acuerdo, el dinero se quema en dos plazos.\n"
                .into(),
            overlay: None,
            tor_rt: None,
            own_onion: String::new(),
            peer_onion: String::new(),
            bootstrap: String::new(),
            lookup_name: String::new(),
            last_error: String::new(),
            net_light: NetLight::Off,
            net_line: "Aún no estás en la red. Pulsa Conectarme cuando quieras hablar.".into(),
            busy: None,
            job_tx,
            job_rx,
            fx: None,
            fx_line: String::new(),
            name_draft: String::new(),
            last_log: String::new(),
            log_hits: 0,
            change_profile: false,
            picking_role,
            tab: start_tab,
            help_open: false,
            search_hits: Vec::new(),
            pay: PayUiDraft::default(),
            watch_scan: None,
            esplora_base: String::new(),
            pay_chain_line: String::new(),
            last_chain_poll: std::time::Instant::now(),
            chain_busy: false,
        }
    }

    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.append_log(&s, false);
        self.last_error.clear();
    }

    fn fail(&mut self, e: impl std::fmt::Display) {
        let e = e.to_string();
        self.last_error = e.clone();
        self.append_log(&format!("error: {e}"), true);
    }

    fn append_log(&mut self, line: &str, is_error: bool) {
        let key = if is_error {
            self.last_error.clone()
        } else {
            line.to_string()
        };
        if key == self.last_log && !key.is_empty() {
            self.log_hits = self.log_hits.saturating_add(1);
            return;
        }
        if self.log_hits > 1 {
            self.log
                .push_str(&format!("  (igual ×{})\n", self.log_hits));
        }
        self.last_log = key;
        self.log_hits = 1;
        self.log.push_str(line);
        if !line.ends_with('\n') {
            self.log.push('\n');
        }
    }

    fn clear_log(&mut self) {
        self.log.clear();
        self.last_log.clear();
        self.log_hits = 0;
        self.last_error.clear();
    }

    fn selected_entry(&self) -> Option<&WorkEntry> {
        let slug = self.selected.as_deref()?;
        self.store.index.works.iter().find(|w| w.slug == slug)
    }

    fn works_for_role(&self) -> Vec<WorkEntry> {
        self.store
            .index
            .works
            .iter()
            .filter(|w| w.role == self.prefs.role)
            .cloned()
            .collect()
    }

    fn socks(&self) -> Option<std::net::SocketAddr> {
        self.overlay
            .as_ref()
            .and_then(|o| o.socks())
            .or_else(|| self.tor_rt.as_ref().map(|t| t.socks))
    }

    fn start_job<F>(&mut self, thinking: impl Into<String>, f: F)
    where
        F: FnOnce(mpsc::Sender<JobEvent>) + Send + 'static,
    {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(thinking.into());
        let tx = self.job_tx.clone();
        thread::Builder::new()
            .name("hbp-job".into())
            .spawn(move || f(tx))
            .ok();
    }

    fn spawn_quiet<F>(&mut self, f: F)
    where
        F: FnOnce(mpsc::Sender<JobEvent>) + Send + 'static,
    {
        let tx = self.job_tx.clone();
        thread::Builder::new()
            .name("hbp-quiet".into())
            .spawn(move || f(tx))
            .ok();
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let mut n = 0u32;
        while let Ok(ev) = self.job_rx.try_recv() {
            n += 1;
            match ev {
                JobEvent::Progress(s) => {
                    self.net_line = s;
                }
                JobEvent::ConnectDone(Ok(rt)) => {
                    self.busy = None;
                    self.apply_tor(rt);
                }
                JobEvent::ConnectDone(Err(e)) => {
                    self.busy = None;
                    self.net_light = NetLight::Err;
                    self.net_line = e.clone();
                    self.fail(e);
                }
                JobEvent::AnnounceDone(Ok((slug, msg))) => {
                    self.busy = None;
                    let _ = self.store.mark_published(&slug);
                    self.note(msg);
                }
                JobEvent::AnnounceDone(Err(e)) => {
                    self.busy = None;
                    self.fail(e);
                }
                JobEvent::LookupDone(Ok(hits)) => {
                    self.busy = None;
                    if hits.is_empty() {
                        self.fail(
                            "No aparece. ¿El mandante se conectó y publicó? Prueba su nombre (Don José) o el de la obra. Si no, usen Avanzado.",
                        );
                    } else {
                        self.on_found_catalog(hits);
                    }
                }
                JobEvent::LookupDone(Err(e)) => {
                    self.busy = None;
                    self.fail(e);
                }
                JobEvent::BootstrapDone(Ok(_)) => {
                    self.busy = None;
                    self.note("Encontré su señal; esperando que el mandante pueda enviarte.");
                    self.retry_hello_if_contratista();
                }
                JobEvent::BootstrapDone(Err(e)) => {
                    self.busy = None;
                    if !self.peer_onion.trim().is_empty() {
                        self.note("Encontré su señal; esperando que el mandante pueda enviarte.");
                        self.retry_hello_if_contratista();
                    } else {
                        self.fail(e);
                    }
                }
                JobEvent::DeliverDone(Ok(msg)) => {
                    self.busy = None;
                    self.note(msg);
                }
                JobEvent::DeliverDone(Err(e)) => {
                    self.busy = None;
                    self.fail(e);
                }
                JobEvent::QuietDeliver(Ok(msg)) => {
                    if !msg.is_empty() {
                        self.note(msg);
                    }
                }
                JobEvent::QuietDeliver(Err(e)) => {
                    self.append_log(&format!("error: {e}"), true);
                }
                JobEvent::FxDone(Ok(q)) => {
                    self.busy = None;
                    self.fx_line =
                        format!("1 BTC ≈ {:.2} {} ({})", q.btc_price_major, q.unit, q.source);
                    self.fx = Some(q);
                }
                JobEvent::FxDone(Err(e)) => {
                    self.busy = None;
                    self.fx_line = format!(
                        "Sin precio ahora ({e}). Sigue igual: el trato es en la moneda de la obra."
                    );
                }
                JobEvent::ScanDone(Ok((scan, base))) => {
                    self.busy = None;
                    self.esplora_base = base.clone();
                    let n = scan.utxos.len();
                    if self.pay.selected_outpoint.is_empty() {
                        if let Some(slug) = self.selected.clone() {
                            if let (Ok(project), Ok(id)) = (
                                self.store.ensure_pay_project(&slug),
                                self.store.load_identity(&slug),
                            ) {
                                if let Ok(role) = party_role(&id, &project.contract.body) {
                                    let fee = parse_fee(&self.pay.fee_sats).unwrap_or(500);
                                    if let Ok(need) = our_funding_need(&project, role, fee) {
                                        if let Some(sug) = suggest_watched(&scan, need).cloned() {
                                            let change = scan.change.clone();
                                            self.pay_take_utxo(&slug, role, &sug, &change);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.watch_scan = Some(scan);
                    self.note(format!("Encontré {n} moneda(s) en Signet ({base})."));
                }
                JobEvent::ScanDone(Err(e)) => {
                    self.busy = None;
                    self.fail(e);
                }
                JobEvent::ChainDone(Ok(Some(hex))) => {
                    self.chain_busy = false;
                    self.pay.funding_tx_hex = hex;
                    self.pay_note_chain_found();
                }
                JobEvent::ChainDone(Ok(None)) => {
                    self.chain_busy = false;
                    self.pay_chain_line = "Esperando el fondeo en Signet…".into();
                }
                JobEvent::ChainDone(Err(e)) => {
                    self.chain_busy = false;
                    self.pay_chain_line = format!("Esplora: {e}");
                }
            }
            if n >= 16 {
                break;
            }
        }
        if n > 0 || self.busy.is_some() || self.overlay.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn apply_tor(&mut self, rt: TorRuntime) {
        let hint = rt.hint_es.clone();
        let onion = rt.onion.clone();
        let socks = rt.socks;
        let findable = rt.findable;
        if let Some(o) = &self.overlay {
            o.set_socks(Some(socks));
            if let Some(ref onion) = onion {
                o.set_advertised(PeerAddr::new(onion.clone(), 80));
            }
        }
        self.net_light = if findable {
            NetLight::Ok
        } else {
            NetLight::Partial
        };
        self.net_line = hint.clone();
        self.note(hint);
        if let Some(onion) = onion {
            self.own_onion = onion;
        }
        self.tor_rt = Some(rt);
        self.bootstrap_known_peers();
        if findable {
            if let Some(entry) = self.selected_entry().cloned() {
                if entry.role == Role::Mandante {
                    self.spawn_announce(&entry);
                } else if !self.peer_onion.trim().is_empty() {
                    self.send_request(&self.peer_onion.clone(), &entry.name);
                }
            }
        }
    }

    fn on_found_catalog(&mut self, hits: Vec<WorkAnnounce>) {
        self.search_hits = hits;
        let n = self.search_hits.len();
        let who = self
            .search_hits
            .iter()
            .find(|a| !a.person_name.trim().is_empty())
            .map(|a| a.person_name.trim().to_string())
            .unwrap_or_else(|| self.lookup_name.trim().to_string());
        self.note(format!(
            "Encontré {n} obra(s) de {who}. Elige una y pide la propuesta. No se acepta sola."
        ));
        self.tab = MainTab::Buscar;
        if let Some(onion) = self
            .search_hits
            .iter()
            .map(|a| a.onion.trim().to_string())
            .find(|o| !o.is_empty())
        {
            self.spawn_bootstrap(&onion);
        }
    }

    fn request_obra(&mut self, ann: WorkAnnounce) {
        let onion = ann.onion.trim().to_string();
        if onion.is_empty() {
            return self.fail("Esa obra no trae código de red.");
        }
        let peer = if ann.person_name.trim().is_empty() {
            None
        } else {
            Some(ann.person_name.as_str())
        };
        match self.store.ensure_contratista_work(&ann.work_name, peer) {
            Ok(entry) => {
                let _ = self.store.remember_peer(&entry.slug, &onion, peer);
                self.peer_onion = onion.clone();
                self.selected = Some(entry.slug);
                self.prefs.role = Role::Contratista;
                let _ = self.store.save_prefs(&self.prefs);
                self.tab = MainTab::Trato;
                self.note(format!(
                    "Pedí la propuesta de {}. Revísala cuando llegue.",
                    ann.work_name
                ));
                self.send_request(&onion, &ann.work_name);
                self.send_hello(&onion, &ann.work_name, false);
            }
            Err(e) => self.fail(e),
        }
    }

    fn send_request(&mut self, dest_onion: &str, work_name: &str) {
        let Some(onion) = self.own_handle() else {
            self.note("Sin mi código aún; reintenta Ver propuesta cuando estés en la red.");
            return;
        };
        let dest = match PeerAddr::parse_flexible(dest_onion) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(o) = self.overlay.clone() else {
            return;
        };
        let msg = NetMessage::Request {
            work_name: work_name.to_string(),
            onion,
            person_name: self.prefs.display_name().to_string(),
        };
        self.spawn_quiet(move |tx| {
            let r = o
                .deliver(&dest, &msg)
                .map(|_| "Pedí la propuesta.".to_string())
                .map_err(|e| format!("No pude pedir la propuesta ({e})"));
            let _ = tx.send(JobEvent::QuietDeliver(r));
        });
    }

    fn own_handle(&self) -> Option<String> {
        let o = self.own_onion.trim();
        if !o.is_empty() {
            return Some(o.to_string());
        }
        let adv = self.overlay.as_ref()?.advertised();
        let host = adv.host.trim();
        if host.is_empty() || host == "0.0.0.0" {
            return None;
        }
        Some(adv.display())
    }

    fn send_hello(&mut self, dest_onion: &str, work_name: &str, reply: bool) {
        let Some(onion) = self.own_handle() else {
            if !reply {
                self.note("Encontré su señal; cuando tenga mi código, el mandante podrá enviarte.");
            }
            return;
        };
        let dest = match PeerAddr::parse_flexible(dest_onion) {
            Ok(p) => p,
            Err(_) => return,
        };
        let Some(o) = self.overlay.clone() else {
            return;
        };
        let msg = NetMessage::Hello {
            work_name: work_name.to_string(),
            onion,
            person_name: self.prefs.display_name().to_string(),
            role: match self.prefs.role {
                Role::Mandante => "mandante".into(),
                Role::Contratista => "contratista".into(),
            },
        };
        let note = if reply {
            String::new()
        } else {
            "Le mandé mi señal al mandante para que pueda enviarte.".into()
        };
        self.spawn_quiet(move |tx| {
            let r = o
                .deliver(&dest, &msg)
                .map(|_| note)
                .map_err(|e| format!("No pude devolver la señal ({e})"));
            let _ = tx.send(JobEvent::QuietDeliver(r));
        });
    }

    fn retry_hello_if_contratista(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.role != Role::Contratista {
            return;
        }
        let dest = self.peer_onion.trim().to_string();
        if dest.is_empty() {
            return;
        }
        self.send_request(&dest, &entry.name);
        self.send_hello(&dest, &entry.name, false);
    }

    fn apply_peer_on(&mut self, slug: &str, onion: &str, person_name: &str) {
        self.peer_onion = onion.to_string();
        let name = person_name.trim();
        let peer = if name.is_empty() { None } else { Some(name) };
        let _ = self.store.remember_peer(slug, onion, peer);
    }

    fn drain_inbox(&mut self) {
        let Some(o) = self.overlay.clone() else {
            return;
        };
        let inbox = o.take_inbox();
        for msg in inbox {
            match msg {
                NetMessage::Hello {
                    work_name,
                    onion,
                    person_name,
                    role,
                } => self.on_inbox_hello(work_name, onion, person_name, role),
                NetMessage::Request {
                    work_name,
                    onion,
                    person_name,
                } => self.on_inbox_request(work_name, onion, person_name),
                NetMessage::Offer { offer } => self.on_inbox_offer(offer),
                NetMessage::Accept { pending } => self.on_inbox_accept(pending),
                NetMessage::Commit { signed } => self.on_inbox_commit(signed),
                NetMessage::Quote { quote } => self.on_inbox_quote(quote),
                NetMessage::Artifact { name, json } => self.on_inbox_artifact(name, json),
                other => self.note(format!("Llegó un recado ({})", other.kind())),
            }
        }
    }

    fn on_inbox_hello(
        &mut self,
        work_name: String,
        onion: String,
        person_name: String,
        role: String,
    ) {
        let onion = onion.trim().to_string();
        if onion.is_empty() {
            return;
        }
        let slug = self
            .store
            .find_by_work_name(&work_name)
            .map(|e| e.slug)
            .or_else(|| self.selected.clone())
            .or_else(|| self.works_for_role().into_iter().next().map(|e| e.slug));
        let Some(slug) = slug else {
            return;
        };
        self.apply_peer_on(&slug, &onion, &person_name);
        let they_contratista = role.eq_ignore_ascii_case("contratista");
        if self.prefs.role == Role::Mandante && they_contratista {
            self.note("El contratista está en la red. Esperando que pida una obra del catálogo.");
            self.send_hello(&onion, &work_name, true);
        }
    }

    fn on_inbox_request(&mut self, work_name: String, onion: String, person_name: String) {
        if self.prefs.role != Role::Mandante {
            return;
        }
        let onion = onion.trim().to_string();
        if onion.is_empty() {
            return;
        }
        let Some(entry) = self.store.find_by_work_name(&work_name) else {
            return self.note(format!(
                "Pidieron {work_name}, pero no tengo esa obra aquí."
            ));
        };
        self.apply_peer_on(&entry.slug, &onion, &person_name);
        self.selected = Some(entry.slug.clone());
        self.tab = MainTab::Trato;
        let who = if person_name.trim().is_empty() {
            "El contratista".to_string()
        } else {
            person_name.trim().to_string()
        };
        self.note(format!("{who} pidió {work_name}. Envío la propuesta."));
        if matches!(self.store.load_offer(&entry.slug), Ok(Some(_))) {
            self.spawn_send_offer(&entry.slug);
        } else {
            self.note("Aún no está firmada. Fírmala en Obra y luego Enviar.");
        }
    }

    fn on_inbox_offer(&mut self, offer: Offer) {
        let name = if offer.body.work_name.trim().is_empty() {
            "obra".to_string()
        } else {
            offer.body.work_name.clone()
        };
        match self.store.ensure_contratista_work(&name, None) {
            Ok(entry) => {
                if let Err(e) = self.store.save_offer(&entry.slug, &offer) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_draft(&entry.slug, &offer.body) {
                    return self.fail(e);
                }
                self.selected = Some(entry.slug);
                self.tab = MainTab::Trato;
                self.note("Llegó la propuesta. Revísala en Trato (total, plazos, partidas) antes de aceptar.");
            }
            Err(e) => self.fail(e),
        }
    }

    fn on_inbox_accept(&mut self, pending: SignedContract) {
        let Some(slug) = self.selected.clone() else {
            return self.note("Llegó una aceptación, abre la obra del mandante.");
        };
        let Some(id) = self.store.load_identity(&slug).ok() else {
            return;
        };
        if id.role != Some(Role::Mandante) {
            return;
        }
        let Some(offer) = self.store.load_offer(&slug).ok().flatten() else {
            return self.fail("Llegó una aceptación pero no tengo la oferta local.");
        };
        match mandante_commit(&offer, pending, &id) {
            Ok(signed) => {
                if let Err(e) = self.store.save_signed(&slug, &signed) {
                    return self.fail(e);
                }
                self.note("Trato cerrado. Sigue en Pago: cotizar y fondear boleta + partida 1.");
                self.spawn_deliver(NetMessage::Commit { signed }, "Mandé la confirmación.");
            }
            Err(e) => self.fail(e),
        }
    }

    fn on_inbox_commit(&mut self, signed: SignedContract) {
        let Some(slug) = self.selected.clone() else {
            return;
        };
        match import_signed(signed) {
            Ok(signed) => {
                if let Err(e) = self.store.save_signed(&slug, &signed) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_draft(&slug, &signed.body) {
                    return self.fail(e);
                }
                self.note(
                    "El mandante confirmó. Trato cerrado. Sigue en Pago: boleta + partida 1.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn slug_for_contract(&self, contract_id: &str) -> Option<String> {
        if let Some(slug) = &self.selected {
            if let Ok(Some(s)) = self.store.load_signed(slug) {
                if s.id().ok().as_deref() == Some(contract_id) {
                    return Some(slug.clone());
                }
            }
        }
        for w in &self.store.index.works {
            if let Ok(Some(s)) = self.store.load_signed(&w.slug) {
                if s.id().ok().as_deref() == Some(contract_id) {
                    return Some(w.slug.clone());
                }
            }
        }
        self.selected.clone()
    }

    fn on_inbox_quote(&mut self, quote: Quote) {
        let Some(slug) = self.slug_for_contract(&quote.contract_id) else {
            return self.note("Llegó una cotización. Abre la obra del trato.");
        };
        self.selected = Some(slug.clone());
        let local = self.store.load_pay_quote(&slug).ok().flatten();
        match apply_incoming_quote(local, quote) {
            Ok(q) => {
                if let Err(e) = self.store.save_pay_quote(&slug, &q) {
                    return self.fail(e);
                }
                if let Ok(mut project) = self.store.ensure_pay_project(&slug) {
                    if let Ok(true) = lock_quote_if_ready(&mut project, &q) {
                        let _ = self.store.save_pay_project(&slug, &project);
                    }
                }
                self.tab = MainTab::Pago;
                if quote_fully_signed(&q) {
                    self.note("Cotización con las dos firmas. Ahora: fondear boleta + partida 1.");
                } else {
                    self.note("Llegó la cotización. Revísala y fírmala en Pago.");
                }
            }
            Err(e) => self.fail(e),
        }
    }

    fn on_inbox_artifact(&mut self, name: String, json: serde_json::Value) {
        let Some(slug) = self.selected.clone() else {
            return self.note(format!("Llegó {name}. Abre la obra del trato."));
        };
        if name.contains("coin") {
            match serde_json::from_value::<OfferedCoin>(json) {
                Ok(coin) => {
                    if let Err(e) = self.store.save_offered_coin(&slug, &coin) {
                        return self.fail(e);
                    }
                    self.note("Llegó una moneda (UTXO) para el fondeo. Ábrela en Pago.");
                    self.tab = MainTab::Pago;
                }
                Err(e) => self.fail(format!("moneda: {e}")),
            }
            return;
        }
        if name.contains("coop") {
            match serde_json::from_value::<CoopFile>(json) {
                Ok(coop) => {
                    if let Err(e) = self.store.save_pay_coop(&slug, &coop) {
                        return self.fail(e);
                    }
                    self.pay.coop_paste = serde_json::to_string_pretty(&coop).unwrap_or_default();
                    self.note("Llegó una ronda MuSig2 de cierre. Fírmala en Pago.");
                    self.tab = MainTab::Pago;
                }
                Err(e) => self.fail(format!("cierre: {e}")),
            }
            return;
        }
        if let Some(hex) = funding_wire_hex(&json).or_else(|| hex_from_artifact(&json)) {
            if name.contains("onesig") || (name.contains("signed") && name.contains("psbt")) {
                self.pay.onesig_hex = hex;
                self.pay.awaiting_chain = true;
                self.pay_chain_line = "Esperando el fondeo en Signet…".into();
                self.note(
                    "Llegó un PSBT con una firma. Ábrelo en Electrum / Sparrow, firma y difunde.",
                );
            } else if name.contains("partial") {
                self.pay.unsigned_psbt_hex = hex;
                self.note("Llegó un PSBT parcial. Completa con tu moneda en Pago.");
            } else if name.contains("unsigned") || name.contains("psbt") {
                self.pay.unsigned_psbt_hex = hex;
                self.note("Llegó el PSBT completo. Expórtalo a Electrum / Sparrow para firmar.");
            } else {
                self.pay.funding_tx_hex = hex;
                self.note("Llegó el tx de fondeo. Lo verifico si hace falta.");
            }
            let _ = self.store.save_pay_draft(&slug, &self.pay);
            self.tab = MainTab::Pago;
            return;
        }
        self.note(format!("Llegó un archivo ({name})."));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.prefs.dark);
        self.poll_jobs(ctx);
        self.drain_inbox();
        if self.selected != self.last_slug {
            if let Some(prev) = self.last_slug.clone() {
                let _ = self.store.save_pay_draft(&prev, &self.pay);
            }
            if let Some(slug) = self.selected.clone() {
                if let Ok(Some(draft)) = self.store.load_draft(&slug) {
                    if let Some((t1, t2)) = draft.dispute.fee_burn_deadlines() {
                        self.t1 = DeadlineFields::from_unix(t1);
                        self.t2 = DeadlineFields::from_unix(t2);
                    }
                    self.total_major = format_major_amount(draft.total_minor(), draft.unit);
                    self.unit = draft.unit;
                }
                if let Some(p) = self.store.load_peer_onion(&slug) {
                    self.peer_onion = p;
                } else {
                    self.peer_onion.clear();
                }
                self.pay = self.store.load_pay_draft(&slug).unwrap_or_default();
                self.watch_scan = None;
                self.pay_chain_line.clear();
            } else {
                self.pay = PayUiDraft::default();
                self.watch_scan = None;
            }
            self.last_slug = self.selected.clone();
        }

        if self.picking_role || self.prefs.needs_name() || self.change_profile {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.show_onboarding(ui);
            });
            return;
        }

        self.maybe_poll_funding_chain();
        self.show_chrome(ctx);
        self.show_log_panel(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_tabs(ui);
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("tab-body")
                .show(ui, |ui| {
                    self.show_tab_body(ui);
                });
        });
    }
}

impl App {
    fn show_onboarding(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.0);
        ui.heading("home_builder_pay");
        ui.label("Para obras chicas. Sin banco de por medio.");
        ui.add_space(16.0);
        if self.picking_role || self.change_profile {
            ui.label(RichText::new("¿Quién eres en esta obra?").strong());
            ui.add_space(8.0);
            if big_btn(ui, "Yo pago — Mandante").clicked() {
                self.prefs.role = Role::Mandante;
                self.picking_role = false;
                self.change_profile = false;
                self.selected = None;
                self.tab = MainTab::Obra;
                self.name_draft = self.prefs.mandante_name.clone();
                let _ = self.store.save_prefs(&self.prefs);
            }
            ui.add_space(6.0);
            if big_btn(ui, "Yo construyo — Contratista").clicked() {
                self.prefs.role = Role::Contratista;
                self.picking_role = false;
                self.change_profile = false;
                self.selected = None;
                self.tab = MainTab::Buscar;
                self.name_draft = self.prefs.contratista_name.clone();
                let _ = self.store.save_prefs(&self.prefs);
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Después te pedimos cómo te dicen. Eso se ve arriba — no un nombre de carpeta.",
                )
                .small()
                .weak(),
            );
            if !self.prefs.display_name().is_empty() {
                ui.add_space(8.0);
                if ui.button("Cancelar").clicked() {
                    self.picking_role = false;
                    self.change_profile = false;
                }
            }
            return;
        }
        if self.prefs.needs_name() {
            ui.label(RichText::new("¿Cómo te dicen?").strong());
            ui.label(match self.prefs.role {
                Role::Mandante => "Ejemplo: Don José. Así te busca el maestro.",
                Role::Contratista => "Ejemplo: Don José. Así te reconoce el mandante.",
            });
            show_field(
                ui,
                &mut self.name_draft,
                "Tu nombre",
                self.prefs.dark,
                280.0,
            );
            if big_btn(ui, "Seguir").clicked() {
                if self.name_draft.trim().is_empty() {
                    self.fail("Escribe cómo te dicen");
                } else {
                    self.prefs.set_display_name(self.name_draft.trim());
                    let _ = self.store.save_prefs(&self.prefs);
                    self.name_draft.clear();
                    self.note(format!("Hola, {}.", self.prefs.display_name()));
                }
            }
        }
    }

    fn chrome_label(&self) -> String {
        let role = match self.prefs.role {
            Role::Mandante => "Mandante",
            Role::Contratista => "Contratista",
        };
        let who = format!("{} · {}", self.prefs.display_name(), role);
        match self.selected_entry() {
            Some(e) if e.role == Role::Contratista && !e.peer_name.is_empty() => {
                format!("{}  ·  {} — con {}", who, e.name, e.peer_name)
            }
            Some(e) => format!("{}  ·  {}", who, e.name),
            None => format!("{who}  ·  —"),
        }
    }

    fn show_chrome(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("chrome")
            .exact_height(52.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let current = self.chrome_label();
                    let works = self.works_for_role();
                    let mut pick: Option<String> = None;
                    let mut new_obra = false;
                    let mut change_profile = false;
                    egui::ComboBox::from_id_salt("wallet-switcher")
                        .width(420.0)
                        .selected_text(RichText::new(current).strong().size(16.0))
                        .show_ui(ui, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    self.prefs.display_name(),
                                    match self.prefs.role {
                                        Role::Mandante => "Mandante",
                                        Role::Contratista => "Contratista",
                                    }
                                ))
                                .weak(),
                            );
                            if ui.selectable_label(false, "Cambiar perfil…").clicked() {
                                change_profile = true;
                            }
                            ui.separator();
                            if works.is_empty() {
                                ui.label(RichText::new("Sin obras todavía").weak().italics());
                            }
                            for w in &works {
                                let label =
                                    if w.role == Role::Contratista && !w.peer_name.is_empty() {
                                        format!("{} — con {}", w.name, w.peer_name)
                                    } else {
                                        w.name.clone()
                                    };
                                let sel = self.selected.as_deref() == Some(w.slug.as_str());
                                if ui.selectable_label(sel, label).clicked() {
                                    pick = Some(w.slug.clone());
                                }
                            }
                            if self.prefs.role == Role::Mandante {
                                ui.separator();
                                if ui.selectable_label(false, "＋ Nueva obra…").clicked() {
                                    new_obra = true;
                                }
                            }
                        });
                    if change_profile {
                        self.change_profile = true;
                        self.picking_role = true;
                        self.selected = None;
                    }
                    if let Some(slug) = pick {
                        self.selected = Some(slug);
                        self.tab = if self.prefs.role == Role::Contratista {
                            MainTab::Trato
                        } else {
                            MainTab::Obra
                        };
                    }
                    if new_obra {
                        self.selected = None;
                        self.tab = MainTab::Obra;
                    }

                    if let Some(b) = &self.busy {
                        ui.add_space(8.0);
                        ui.spinner();
                        ui.label(RichText::new(b).small().italics());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(if self.prefs.dark { "Claro" } else { "Oscuro" })
                            .clicked()
                        {
                            self.prefs.dark = !self.prefs.dark;
                            let _ = self.store.save_prefs(&self.prefs);
                        }
                        net_badge(ui, self.net_light, &self.net_line);
                    });
                });
            });
    }

    fn show_tabs(&mut self, ui: &mut egui::Ui) {
        let tabs: &[(MainTab, &str)] = match self.prefs.role {
            Role::Mandante => &[
                (MainTab::Obra, "Obra"),
                (MainTab::Red, "Red"),
                (MainTab::Trato, "Trato"),
                (MainTab::Pago, "Pago"),
            ],
            Role::Contratista => &[
                (MainTab::Buscar, "Buscar"),
                (MainTab::Trato, "Trato"),
                (MainTab::Red, "Red"),
                (MainTab::Pago, "Pago"),
            ],
        };
        let pago_ok = self
            .selected
            .as_ref()
            .and_then(|s| self.store.load_signed(s).ok().flatten())
            .is_some();
        ui.horizontal(|ui| {
            for (tab, label) in tabs {
                let selected = self.tab == *tab;
                let disabled = *tab == MainTab::Pago && !pago_ok;
                let text = if disabled {
                    RichText::new(*label).weak()
                } else if selected {
                    RichText::new(*label)
                        .strong()
                        .color(accent_blue(self.prefs.dark))
                } else {
                    RichText::new(*label)
                };
                if ui
                    .add_enabled(!disabled, egui::SelectableLabel::new(selected, text))
                    .clicked()
                {
                    self.tab = *tab;
                }
            }
        });
        ui.separator();
    }

    fn show_tab_body(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            MainTab::Obra => self.show_tab_obra(ui),
            MainTab::Buscar => self.show_tab_buscar(ui),
            MainTab::Red => self.show_tab_red(ui),
            MainTab::Trato => self.show_tab_trato(ui),
            MainTab::Pago => self.show_tab_pago(ui),
        }
    }

    fn show_tab_obra(&mut self, ui: &mut egui::Ui) {
        if self.prefs.role != Role::Mandante {
            ui.label("La obra la arma el mandante.");
            return;
        }
        let Some(slug) = self.selected.clone() else {
            ui.add_space(8.0);
            panel_card(ui, self.prefs.dark, |ui| {
                ui.label(RichText::new("Nueva obra").strong());
                ui.label("Nombre de la faena, no el tuyo.");
                show_field(ui, &mut self.new_name, "ej. casa2", self.prefs.dark, 280.0);
                ui.add_space(8.0);
                if primary_btn(ui, "Crear obra", self.prefs.dark).clicked() {
                    self.create_mandante_work();
                }
            });
            return;
        };
        let entry = match self.selected_entry().cloned() {
            Some(e) => e,
            None => return,
        };
        let id = match self.store.load_identity(&slug) {
            Ok(id) => id,
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let has_draft = matches!(self.store.load_draft(&slug), Ok(Some(_)));
        let has_offer = matches!(self.store.load_offer(&slug), Ok(Some(_)));
        let has_signed = matches!(self.store.load_signed(&slug), Ok(Some(_)));
        self.show_construction(ui, &slug, &id, &entry, has_draft, has_offer, has_signed);
        ui.add_space(12.0);
        ui.collapsing("Billetera y respaldo", |ui| {
            self.show_wallet(ui, &slug);
            self.show_backup(ui, &slug);
        });
    }

    fn show_tab_buscar(&mut self, ui: &mut egui::Ui) {
        panel_card(ui, self.prefs.dark, |ui| {
            ui.label(RichText::new("Buscar mandante").strong());
            ui.label("Su nombre. Verás sus obras; no se abre un trato solo.");
            show_field(
                ui,
                &mut self.lookup_name,
                "ej. Felipe",
                self.prefs.dark,
                280.0,
            );
            ui.add_space(8.0);
            let net_ok = self.overlay.is_some();
            if !net_ok {
                if primary_btn(ui, "Conectarme", self.prefs.dark).clicked() {
                    self.connect_network();
                }
            } else if primary_btn(ui, "Buscar", self.prefs.dark).clicked() {
                self.spawn_lookup();
            }
        });
        if !self.search_hits.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Obras publicadas").strong());
            let hits = self.search_hits.clone();
            let mut pick = None;
            for ann in &hits {
                let who = if ann.person_name.trim().is_empty() {
                    String::new()
                } else {
                    format!(" · {}", ann.person_name.trim())
                };
                panel_card(ui, self.prefs.dark, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&ann.work_name).strong().size(16.0));
                        ui.label(RichText::new(who).weak());
                        if primary_btn(ui, "Ver propuesta", self.prefs.dark).clicked() {
                            pick = Some(ann.clone());
                        }
                    });
                });
                ui.add_space(6.0);
            }
            if let Some(ann) = pick {
                self.request_obra(ann);
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Mis tratos").weak());
        self.work_list(ui, true);
    }

    fn show_tab_red(&mut self, ui: &mut egui::Ui) {
        let entry = self.selected_entry().cloned();
        panel_card(ui, self.prefs.dark, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Red").strong());
                net_badge(ui, self.net_light, &self.net_line);
            });
            ui.label(RichText::new(&self.net_line).small());
            ui.add_space(6.0);
            let busy = self.busy.is_some() || self.net_light == NetLight::Connecting;
            let connected = self.overlay.is_some();
            if !connected {
                if primary_btn(
                    ui,
                    if busy { "Conectando…" } else { "Conectarme" },
                    self.prefs.dark,
                )
                .clicked()
                    && !busy
                {
                    self.connect_network();
                }
            } else if self.prefs.role == Role::Mandante {
                if let Some(e) = entry.as_ref() {
                    if e.published {
                        ui.label(
                            RichText::new(format!(
                                "Te buscan como {} · obra {}",
                                if e.publisher_name.is_empty() {
                                    self.prefs.display_name()
                                } else {
                                    e.publisher_name.as_str()
                                },
                                e.name
                            ))
                            .color(accent_green(self.prefs.dark)),
                        );
                    } else if primary_btn(ui, "Publicar", self.prefs.dark).clicked() {
                        self.spawn_announce(e);
                    }
                } else {
                    ui.label("Crea una obra en la pestaña Obra.");
                }
            } else {
                ui.label(
                    RichText::new("En la red. Busca al mandante en Buscar.")
                        .color(accent_green(self.prefs.dark)),
                );
            }
        });
        ui.add_space(8.0);
        ui.collapsing("Avanzado", |ui| {
            if !self.own_onion.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Tu código: {}", self.own_onion)).small());
                    if ui.small_button("Copiar").clicked() {
                        ui.output_mut(|o| o.copied_text = self.own_onion.clone());
                        self.note("Código copiado");
                    }
                });
            }
            ui.horizontal(|ui| {
                show_field(
                    ui,
                    &mut self.bootstrap,
                    "xxxx.onion",
                    self.prefs.dark,
                    280.0,
                );
                if ui
                    .add_enabled(
                        self.overlay.is_some() && self.busy.is_none(),
                        egui::Button::new("Usar código"),
                    )
                    .clicked()
                {
                    let raw = self.bootstrap.clone();
                    self.spawn_bootstrap(&raw);
                }
            });
            if let Some(o) = &self.overlay {
                ui.label(
                    RichText::new(format!("contactos {} · {}", o.peer_count(), o.local_addr()))
                        .small()
                        .weak(),
                );
            }
        });
    }

    fn show_tab_trato(&mut self, ui: &mut egui::Ui) {
        let Some(slug) = self.selected.clone() else {
            ui.label(RichText::new("Elige una obra arriba, o búsca al mandante.").weak());
            return;
        };
        let entry = match self.selected_entry().cloned() {
            Some(e) => e,
            None => return,
        };
        let id = match self.store.load_identity(&slug) {
            Ok(id) => id,
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let has_draft = matches!(self.store.load_draft(&slug), Ok(Some(_)));
        let has_offer = matches!(self.store.load_offer(&slug), Ok(Some(_)));
        let has_pending = matches!(self.store.load_pending(&slug), Ok(Some(_)));
        let has_signed = matches!(self.store.load_signed(&slug), Ok(Some(_)));
        self.show_next_card(
            ui,
            &slug,
            &id,
            &entry,
            has_draft,
            has_offer,
            has_pending,
            has_signed,
        );
        if has_draft || has_offer {
            ui.add_space(10.0);
            self.show_proposal_review(ui, &slug);
        } else if entry.role == Role::Contratista {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Todavía no hay propuesta. Pídela desde Buscar → Ver propuesta.")
                    .weak(),
            );
        }
        if entry.role == Role::Contratista && has_offer && !has_pending && !has_signed {
            ui.add_space(8.0);
            if primary_btn(ui, "Aceptar trato", self.prefs.dark).clicked() {
                self.accept_from_store(&slug, &id);
            }
        }
    }

    fn show_proposal_review(&self, ui: &mut egui::Ui, slug: &str) {
        let Ok(Some(draft)) = self.store.load_draft(slug) else {
            return;
        };
        let bond = bond_minor(draft.total_minor(), draft.bond_bps).unwrap_or(0);
        let dark = self.prefs.dark;
        panel_card(ui, dark, |ui| {
            ui.label(RichText::new("Propuesta — revísala").strong());
            ui.label(format!(
                "Total {} {} · boleta 10% = {} {}",
                format_major_amount(draft.total_minor(), draft.unit),
                draft.unit,
                format_major_amount(bond, draft.unit),
                draft.unit
            ));
            if let Some((t1, t2)) = draft.dispute.fee_burn_deadlines() {
                ui.label(format!("Primer plazo: {}", format_unix_local_es(t1)));
                ui.label(format!("Segundo plazo: {}", format_unix_local_es(t2)));
            }
            ui.label(
                RichText::new(
                    "Si ambos están de acuerdo, se paga. Si no, se quema a esos dos plazos.",
                )
                .small()
                .weak(),
            );
            ui.add_space(6.0);
            egui::Grid::new("review-stages")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("partida");
                    ui.strong("monto");
                    ui.end_row();
                    for p in &draft.partidas {
                        ui.label(p.id.to_string());
                        ui.label(&p.description);
                        ui.label(format_major_amount(p.amount_minor, draft.unit));
                        ui.end_row();
                    }
                });
        });
    }

    fn show_log_panel(&mut self, ctx: &egui::Context) {
        let open = self.prefs.log_open;
        let mut panel = egui::TopBottomPanel::bottom("log").resizable(open);
        if open {
            panel = panel.default_height(120.0).min_height(64.0);
        } else {
            panel = panel.exact_height(36.0);
        }
        panel.show(ctx, |ui| {
            ui.horizontal(|ui| {
                let arrow = if self.prefs.log_open {
                    "▾ Notas"
                } else {
                    "▸ Notas"
                };
                if ui.strong(arrow).clicked()
                    || ui
                        .small_button(if open { "Ocultar" } else { "Ver" })
                        .clicked()
                {
                    self.prefs.log_open = !self.prefs.log_open;
                    let _ = self.store.save_prefs(&self.prefs);
                }
                if self.log_hits > 1 {
                    ui.label(RichText::new(format!("×{}", self.log_hits)).weak());
                }
                if ui.small_button("Limpiar").clicked() {
                    self.clear_log();
                }
                if !self.last_error.is_empty() && !self.prefs.log_open {
                    ui.colored_label(
                        Color32::from_rgb(220, 80, 80),
                        RichText::new(&self.last_error).small(),
                    );
                }
            });
            if self.prefs.log_open {
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(200.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.log)
                                .desired_width(ui.available_width())
                                .desired_rows(5)
                                .text_color(edit_fg(self.prefs.dark)),
                        );
                    });
                if !self.last_error.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 80, 80), &self.last_error);
                }
            }
        });
    }

    fn work_list(&mut self, ui: &mut egui::Ui, as_trato: bool) {
        let works = self.works_for_role();
        if works.is_empty() {
            ui.label(RichText::new("Todavía no hay nada aquí.").weak());
            return;
        }
        for w in works {
            let label = if as_trato && !w.peer_name.is_empty() {
                format!("{} — con {}", w.name, w.peer_name)
            } else {
                w.name.clone()
            };
            if ui
                .selectable_label(self.selected.as_deref() == Some(w.slug.as_str()), label)
                .clicked()
            {
                self.selected = Some(w.slug);
                self.tab = MainTab::Trato;
            }
        }
    }

    fn create_mandante_work(&mut self) {
        if self.new_name.trim().is_empty() {
            return self.fail("Escribe el nombre de la obra primero");
        }
        match self
            .store
            .create_product_work(&self.new_name, Role::Mandante, None)
        {
            Ok(e) => {
                let _ = self
                    .store
                    .set_publisher_name(&e.slug, self.prefs.display_name());
                self.note(format!("Obra creada: {}", e.name));
                self.selected = Some(e.slug);
                self.tab = MainTab::Obra;
                self.new_name.clear();
            }
            Err(e) => self.fail(friendly_store_err(&e.to_string())),
        }
    }

    fn show_next_card(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        id: &Identity,
        entry: &WorkEntry,
        has_draft: bool,
        has_offer: bool,
        has_pending: bool,
        has_signed: bool,
    ) {
        let progress = WorkProgress {
            has_draft,
            has_offer,
            has_pending,
            has_signed,
            net_up: self.overlay.is_some()
                && matches!(self.net_light, NetLight::Ok | NetLight::Partial),
            has_peer: !self.peer_onion.trim().is_empty(),
            published: entry.published,
        };
        let step = next_step(entry.role, progress);
        let done = completed_steps(entry.role, progress);
        if !done.is_empty() {
            for label in done {
                ui.label(RichText::new(format!("✓ {label}")).small().weak());
            }
            ui.add_space(4.0);
        }
        let waiting = step.button.is_none() && step.kind == NextKind::None && !has_signed;
        let (fill, stroke) = if has_signed {
            (
                if self.prefs.dark {
                    Color32::from_rgb(28, 48, 36)
                } else {
                    Color32::from_rgb(226, 242, 230)
                },
                accent_green(self.prefs.dark),
            )
        } else if waiting {
            (
                if self.prefs.dark {
                    Color32::from_rgb(52, 42, 24)
                } else {
                    Color32::from_rgb(255, 243, 220)
                },
                accent_amber(self.prefs.dark),
            )
        } else {
            (
                if self.prefs.dark {
                    Color32::from_rgb(28, 40, 56)
                } else {
                    Color32::from_rgb(226, 236, 252)
                },
                accent_blue(self.prefs.dark),
            )
        };
        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .inner_margin(12.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Qué hacer ahora").strong());
                ui.label(&step.sentence);
                if let Some(label) = step.button {
                    ui.add_space(6.0);
                    if primary_btn(ui, label, self.prefs.dark).clicked() {
                        self.run_next_kind(step.kind, slug, id, entry);
                    }
                }
            });
    }

    fn run_next_kind(&mut self, kind: NextKind, slug: &str, id: &Identity, entry: &WorkEntry) {
        match kind {
            NextKind::Prepare => self.build_draft(slug, id, entry),
            NextKind::Sign => self.emit_offer(slug, id),
            NextKind::Connect => self.connect_network(),
            NextKind::Publish => self.spawn_announce(entry),
            NextKind::Send => self.spawn_send_offer(slug),
            NextKind::Accept => self.accept_from_store(slug, id),
            NextKind::Search => {
                if self.lookup_name.trim().is_empty() && !entry.peer_name.is_empty() {
                    self.lookup_name = entry.peer_name.clone();
                }
                self.tab = MainTab::Buscar;
                self.spawn_lookup();
            }
            NextKind::Pay => self.tab = MainTab::Pago,
            NextKind::None => {}
        }
    }

    fn show_construction(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        id: &Identity,
        entry: &WorkEntry,
        has_draft: bool,
        has_offer: bool,
        has_signed: bool,
    ) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Obra").strong().size(18.0));
            if ui.small_button("¿Qué es esto?").clicked() {
                self.help_open = !self.help_open;
            }
        });
        if self.help_open {
            ui.label(
                RichText::new(format!(
                    "{} Total ÷ 10 = boleta y cada partida. Sin acuerdo se quema a dos plazos.",
                    unit_helper(self.unit)
                ))
                .small()
                .weak(),
            );
        }
        if has_signed {
            ui.label(RichText::new("Trato cerrado.").color(accent_green(self.prefs.dark)));
        }

        match entry.role {
            Role::Mandante => {
                ui.add_space(6.0);
                ui.label(RichText::new("Monto y plazos").strong());
                ui.horizontal(|ui| {
                    ui.label("Total");
                    show_field(ui, &mut self.total_major, "100", self.prefs.dark, 80.0);
                    ui.label("moneda");
                    egui::ComboBox::from_id_salt("contract-unit")
                        .selected_text(self.unit.as_str())
                        .show_ui(ui, |ui| {
                            for u in Unit::ALL {
                                ui.selectable_value(&mut self.unit, u, u.as_str());
                            }
                        });
                });
                self.show_fx_line(ui);
                if let Ok(total) = parse_major_amount(&self.total_major, self.unit) {
                    if let Ok(bond) = bond_minor(total, DEFAULT_BOND_BPS) {
                        let n = hbp_core::equal_stage_count(DEFAULT_BOND_BPS).unwrap_or(0);
                        let bond_txt = format_major_amount(bond, self.unit);
                        ui.label(
                            RichText::new(format!(
                                "Boleta 10% = {bond_txt} {}. Serán {n} partidas de {bond_txt}.",
                                self.unit
                            ))
                            .weak(),
                        );
                    }
                }
                ui.label("Primer plazo");
                deadline_editor(ui, "t1", &mut self.t1);
                ui.label("Segundo plazo");
                deadline_editor(ui, "t2", &mut self.t2);
                ui.label("Partidas (opcional, una por línea)");
                show_multiline(
                    ui,
                    &mut self.stage_descs,
                    "Radier\nMuros\nTechumbre",
                    self.prefs.dark,
                    420.0,
                    3,
                );
                ui.add_space(8.0);
                if !has_draft {
                    if primary_btn(ui, "Preparar partidas", self.prefs.dark).clicked() {
                        self.build_draft(slug, id, entry);
                    }
                } else if !has_offer {
                    if primary_btn(ui, "Firmar propuesta", self.prefs.dark).clicked() {
                        self.emit_offer(slug, id);
                    }
                } else {
                    ui.label(
                        RichText::new("Propuesta lista · envía en Trato")
                            .color(accent_green(self.prefs.dark)),
                    );
                }
            }
            Role::Contratista => {
                if has_offer || has_draft {
                    ui.label(RichText::new("Propuesta recibida.").strong());
                } else {
                    ui.label("Espera el envío. Archivo de respaldo:");
                    show_field(
                        ui,
                        &mut self.accept_path,
                        "archivo (opcional)",
                        self.prefs.dark,
                        320.0,
                    );
                    if ui.button("Abrir archivo").clicked() {
                        self.accept_from_file(slug, id);
                    }
                }
            }
        }

        if let Ok(Some(draft)) = self.store.load_draft(slug) {
            ui.separator();
            ui.heading("Tablero de partidas");
            let bond = bond_minor(draft.total_minor(), draft.bond_bps).unwrap_or(0);
            if let Some((t1, t2)) = draft.dispute.fee_burn_deadlines() {
                ui.label(format!("Primer plazo: {}", format_unix_local_es(t1)));
                ui.label(format!("Segundo plazo: {}", format_unix_local_es(t2)));
            }
            ui.label(format!(
                "Total {} {} · boleta {} (10%)",
                format_major_amount(draft.total_minor(), draft.unit),
                draft.unit,
                format_major_amount(bond, draft.unit)
            ));
            egui::Grid::new("stages").striped(true).show(ui, |ui| {
                ui.strong("#");
                ui.strong("qué se hace");
                ui.strong("monto");
                ui.strong("= boleta");
                ui.strong("plazo");
                ui.end_row();
                for p in &draft.partidas {
                    ui.label(p.id.to_string());
                    ui.label(&p.description);
                    ui.label(format_major_amount(p.amount_minor, draft.unit));
                    ui.label(if p.amount_minor == bond { "sí" } else { "NO" });
                    ui.label(format_unix_local_es(p.plazo_unix));
                    ui.end_row();
                }
            });
        }
    }

    fn show_fx_line(&mut self, ui: &mut egui::Ui) {
        if self.unit.is_bitcoin_denom() {
            if let Ok(total) = parse_major_amount(&self.total_major, self.unit) {
                if let Some(sats) = preview_sats(total, self.unit, None) {
                    ui.label(
                        RichText::new(format!(
                            "Eso son {sats} sats on-chain (sin tipo de cambio)."
                        ))
                        .small(),
                    );
                }
            }
            return;
        }
        ui.horizontal(|ui| {
            if !self.fx_line.is_empty() {
                ui.label(RichText::new(&self.fx_line).small().weak());
            }
            if ui
                .add_enabled(
                    self.busy.is_none(),
                    egui::Button::new("Ver equivalente en sats"),
                )
                .clicked()
            {
                self.spawn_fx();
            }
        });
        if let (Ok(total), Some(q)) = (
            parse_major_amount(&self.total_major, self.unit),
            self.fx.as_ref(),
        ) {
            if q.unit == self.unit {
                if let Some(sats) = preview_sats(total, self.unit, Some(q)) {
                    ui.label(
                        RichText::new(format!(
                            "Hoy serían unos {sats} sats ({}). Se fija de verdad al fondear, no ahora.",
                            q.source
                        ))
                        .small(),
                    );
                }
            }
        }
    }

    fn show_tab_pago(&mut self, ui: &mut egui::Ui) {
        let Some(slug) = self.selected.clone() else {
            ui.label(RichText::new("Elige una obra arriba.").weak());
            return;
        };
        let signed = match self.store.load_signed(&slug) {
            Ok(Some(s)) => s,
            Ok(None) => {
                panel_card(ui, self.prefs.dark, |ui| {
                    ui.label(RichText::new("Pago").strong());
                    ui.label(
                        RichText::new("Cierra el trato primero (Aceptar / confirmar).")
                            .color(accent_amber(self.prefs.dark)),
                    );
                });
                return;
            }
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let id = match self.store.load_identity(&slug) {
            Ok(id) => id,
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let project = match self.store.ensure_pay_project(&slug) {
            Ok(p) => p,
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let pending_q = self.store.load_pay_quote(&slug).ok().flatten();
        let stage = pay_stage(Some(&project), pending_q.as_ref());
        let now = spanish_now(Some(&project), pending_q.as_ref());
        let dark = self.prefs.dark;
        let status_color = match stage {
            PayStage::PartidaInCourse | PayStage::UnlockNext | PayStage::AllClosed => {
                accent_green(dark)
            }
            PayStage::FundBondP1 => accent_green(dark),
            _ => accent_amber(dark),
        };

        panel_card(ui, dark, |ui| {
            ui.label(RichText::new("Pago").strong().size(18.0));
            ui.label(RichText::new(now).color(status_color).strong());
            ui.label(
                RichText::new("Signet. Una partida a la vez. Sin árbitro. Sin acuerdo: fee-burn.")
                    .small()
                    .weak(),
            );
        });
        ui.add_space(8.0);

        self.show_pay_watch(ui, &slug);
        ui.add_space(8.0);
        self.show_pay_partidas(ui, &project);
        ui.add_space(8.0);
        self.show_pay_quote(ui, &slug, &id, &signed, &project, pending_q.as_ref());

        if matches!(
            stage,
            PayStage::FundBondP1 | PayStage::PartidaInCourse | PayStage::UnlockNext
        ) && project.quote.is_some()
        {
            ui.add_space(8.0);
            self.show_pay_fund(ui, &slug, &id, &project);
        }
        if matches!(stage, PayStage::PartidaInCourse) {
            ui.add_space(8.0);
            self.show_pay_coop(ui, &slug, &id, &project);
        }
        if matches!(stage, PayStage::UnlockNext) {
            ui.add_space(8.0);
            panel_card(ui, dark, |ui| {
                ui.label(RichText::new("Partida 2").strong());
                ui.label(
                    RichText::new(
                        "Desbloqueada. El fondeo de P2 (solo mandante, --partida-only) no está en esta pantalla. Esta ronda cubre boleta + partida 1.",
                    )
                    .weak(),
                );
            });
        }
    }

    fn show_pay_watch(&mut self, ui: &mut egui::Ui, slug: &str) {
        let watch = self.store.load_watch(slug, Some(self.passphrase.as_str()));
        panel_card(ui, self.prefs.dark, |ui| match watch {
            Ok(Some(acc)) => {
                if let Ok(a) = address_at(&acc.receive_descriptor, 0, PRODUCT_NETWORK) {
                    ui.label(RichText::new("Billetera watch-only").strong());
                    ui.label(RichText::new(format!("Dirección 0: {a}")).small().weak());
                    ui.label(
                        RichText::new(
                            "Local. Nadie más ve la xpub. En Pago: Buscar mis monedas (Esplora).",
                        )
                        .small()
                        .weak(),
                    );
                }
            }
            Ok(None) => {
                ui.label(RichText::new("Falta la xpub / vpub (watch-only, local)").strong());
                ui.label(
                    RichText::new(
                        "Guárdala aquí. La app lista tus UTXO en Signet. No se envía al otro.",
                    )
                    .small()
                    .weak(),
                );
                self.show_wallet(ui, slug);
            }
            Err(e) => {
                ui.label(RichText::new(e.to_string()).small().weak());
                self.show_wallet(ui, slug);
            }
        });
    }

    fn show_pay_partidas(&self, ui: &mut egui::Ui, project: &hbp_core::Project) {
        panel_card(ui, self.prefs.dark, |ui| {
            ui.label(RichText::new("Partidas").strong());
            for p in &project.partidas {
                let live = partida_ui_enabled(project, p.id);
                let state = match &p.state {
                    hbp_core::PartidaStatus::Scheduled => "sin cotizar",
                    hbp_core::PartidaStatus::AmountAgreed { sats } => {
                        // shown below
                        let _ = sats;
                        "cotizada"
                    }
                    hbp_core::PartidaStatus::Funding { .. }
                    | hbp_core::PartidaStatus::Locked { .. } => "en curso",
                    hbp_core::PartidaStatus::ReceptionProposed { .. } => "recepción propuesta",
                    hbp_core::PartidaStatus::Paid { .. } => "pagada",
                    hbp_core::PartidaStatus::Unwound { .. } => "deshecha",
                    hbp_core::PartidaStatus::FeeBurnT1 { .. } => "fee-burn t1",
                    hbp_core::PartidaStatus::FeeBurnT2 { .. } => "fee-burn t2",
                };
                let extra = match &p.state {
                    hbp_core::PartidaStatus::AmountAgreed { sats }
                    | hbp_core::PartidaStatus::Funding { sats, .. }
                    | hbp_core::PartidaStatus::Locked { sats, .. } => format!(" · {sats} sats"),
                    _ => String::new(),
                };
                let line = format!("Partida {} — {state}{extra}", p.id);
                if live {
                    ui.label(RichText::new(line).strong());
                } else if p.is_terminal() {
                    ui.label(RichText::new(format!("✓ {line}")).small().weak());
                } else {
                    ui.label(
                        RichText::new(format!("{line} (bloqueada hasta cerrar la anterior)"))
                            .weak(),
                    );
                }
            }
        });
    }

    fn show_pay_quote(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        id: &Identity,
        signed: &SignedContract,
        project: &hbp_core::Project,
        pending: Option<&Quote>,
    ) {
        let quote = project.quote.as_ref().or(pending);
        let locked = quote.map(quote_fully_signed).unwrap_or(false);
        let dark = self.prefs.dark;
        panel_card(ui, dark, |ui| {
            ui.label(RichText::new("Cotización — boleta + partida 1").strong());
            if locked {
                if let Some(q) = quote {
                    let p1 = q.partida_sats(1).unwrap_or(0);
                    ui.label(format!(
                        "Boleta {} sats · partida 1 {} sats",
                        q.bond_sats, p1
                    ));
                    if !q.fx_note.is_empty() {
                        ui.label(RichText::new(&q.fx_note).small().weak());
                    }
                    ui.label(
                        RichText::new("Las dos partes ya firmaron. Los sats están fijos.")
                            .small()
                            .color(accent_green(dark)),
                    );
                }
                return;
            }

            ui.label(
                RichText::new("Precio BTC: Yadio → CoinGecko → CoinMarketCap, o escríbelo tú.")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("Consultar precio").clicked() {
                    self.spawn_fx();
                }
                if !self.fx_line.is_empty() {
                    ui.label(RichText::new(&self.fx_line).small());
                }
            });
            ui.horizontal(|ui| {
                ui.label("BTC (manual)");
                show_field(ui, &mut self.pay.price_major, "ej. 80000", dark, 140.0);
            });

            if let Some(price) = self.pay_price_minor() {
                if let Ok((bond, p1)) = preview_quote_sats(signed, Some(price)) {
                    ui.label(
                        RichText::new(format!("Boleta ≈ {bond} sats · partida 1 ≈ {p1} sats"))
                            .strong(),
                    );
                }
            } else if signed.body.unit.is_bitcoin_denom() {
                if let Ok((bond, p1)) = preview_quote_sats(signed, None) {
                    ui.label(
                        RichText::new(format!("Boleta {bond} sats · partida 1 {p1} sats")).strong(),
                    );
                }
            }

            if let Some(q) = quote {
                let p1 = q.partida_sats(1).unwrap_or(0);
                ui.label(format!(
                    "Pendiente: boleta {} · P1 {} sats. Falta una firma.",
                    q.bond_sats, p1
                ));
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if primary_btn(ui, "Proponer cotización", dark).clicked() {
                    self.pay_propose_quote(slug, id, signed);
                }
                if ui.button("Firmar cotización").clicked() {
                    self.pay_sign_quote(slug, id, signed);
                }
            });
        });
    }

    fn show_pay_fund(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        id: &Identity,
        project: &hbp_core::Project,
    ) {
        let dark = self.prefs.dark;
        let role = match party_role(id, &project.contract.body) {
            Ok(r) => r,
            Err(e) => {
                ui.colored_label(theme_red(), e.to_string());
                return;
            }
        };
        let fee = parse_fee(&self.pay.fee_sats).unwrap_or(500);
        let need = our_funding_need(project, role, fee).ok();
        let has_watch = matches!(
            self.store.load_watch(slug, Some(self.passphrase.as_str())),
            Ok(Some(_))
        );
        let our_coin = self.our_saved_coin(slug, role);
        let psbt = self.pay.unsigned_psbt_hex.trim().to_string();
        let class = if psbt.is_empty() {
            None
        } else {
            classify_funding_psbt(&psbt).ok()
        };
        let onesig_class = if self.pay.onesig_hex.trim().is_empty() {
            None
        } else {
            classify_funding_psbt(&self.pay.onesig_hex).ok()
        };
        let we_own = our_coin
            .as_ref()
            .map(|c| parse_psbt_has_outpoint(&psbt, &c.outpoint))
            .unwrap_or(false);
        let sigs = class
            .map(|c| c.1)
            .unwrap_or(0)
            .max(onesig_class.map(|c| c.1).unwrap_or(0));
        let step = fund_handshake_step(
            has_watch,
            self.watch_scan.is_some(),
            our_coin.is_some(),
            class.map(|c| c.0),
            sigs,
            we_own,
            self.pay.awaiting_chain,
        );
        let show_coins = matches!(
            step,
            FundHandshakeStep::ScanCoins
                | FundHandshakeStep::PickCoin
                | FundHandshakeStep::SendPartial
                | FundHandshakeStep::CompleteIncoming
        );

        panel_card(ui, dark, |ui| {
            ui.label(RichText::new("Fondeo — boleta + partida 1").strong());
            ui.label(
                RichText::new(format!("Ahora: {}", step.title_es()))
                    .color(accent_amber(dark))
                    .strong(),
            );
            ui.label(
                RichText::new("Mis monedas → parcial → completar → firmar fuera → en cadena.")
                    .small()
                    .weak(),
            );
            if let Ok((bond, p1)) = escrow_addrs(project) {
                ui.label(RichText::new(format!("Boleta: {bond}")).small());
                ui.label(RichText::new(format!("Partida 1: {p1}")).small());
                if bond == p1 {
                    ui.label(
                        RichText::new("Error: las dos direcciones salieron iguales.")
                            .color(theme_red()),
                    );
                }
            }
            ui.horizontal(|ui| {
                ui.label("Comisión");
                show_field(ui, &mut self.pay.fee_sats, "500", dark, 80.0);
                ui.label(RichText::new("sats (sale del cambio)").small().weak());
                if let Some(n) = need {
                    ui.label(RichText::new(format!("Tu parte ≈ {n} sats")).small());
                }
            });

            if show_coins {
                self.show_pay_coin_picker(ui, slug, role, need.unwrap_or(0), our_coin.as_ref());
            } else if let Some(c) = &our_coin {
                ui.label(
                    RichText::new(format!("Usando {} · {} sats", c.outpoint, c.sats))
                        .small()
                        .weak(),
                );
            }

            ui.add_space(8.0);
            match step {
                FundHandshakeStep::NeedWatch => {
                    ui.label(
                        RichText::new("Primero guarda tu xpub / vpub arriba (watch-only).")
                            .color(accent_amber(dark)),
                    );
                }
                FundHandshakeStep::ScanCoins => {
                    ui.label("La app mira tu xpub en Signet (Esplora) y sugiere una moneda.");
                    if primary_btn(ui, "Buscar mis monedas", dark).clicked() {
                        self.pay_scan_coins(slug);
                    }
                }
                FundHandshakeStep::PickCoin => {
                    ui.label("Elige una moneda con suficientes sats (la app ya sugiere una).");
                    if ui.small_button("Volver a buscar").clicked() {
                        self.pay_scan_coins(slug);
                    }
                }
                FundHandshakeStep::SendPartial => {
                    ui.label("Arma un PSBT solo con tu input y mándalo al otro por Tor.");
                    if primary_btn(ui, "Armar y enviar parcial", dark).clicked() {
                        self.pay_start_partial(slug, role, project);
                    }
                }
                FundHandshakeStep::WaitPeerComplete => {
                    ui.label(
                        RichText::new("Parcial enviado. Esperando que el otro añada su moneda.")
                            .color(accent_amber(dark)),
                    );
                }
                FundHandshakeStep::CompleteIncoming => {
                    ui.label("Llegó su parcial. Añade tu moneda y devuélvelo.");
                    if primary_btn(ui, "Completar y devolver", dark).clicked() {
                        self.pay_complete_partial(slug, role, project);
                    }
                }
                FundHandshakeStep::ExportAndSign => {
                    ui.label(
                        "PSBT completo (2 inputs). Cópialo, fírmalo en Electrum / Sparrow, pega el de 1 firma.",
                    );
                    ui.label(RichText::new("Para exportar — no difundir todavía").small());
                    show_multiline(
                        ui,
                        &mut self.pay.unsigned_psbt_hex,
                        "PSBT…",
                        dark,
                        ui.available_width().min(720.0),
                        3,
                    );
                    ui.label(RichText::new("Pega el PSBT con TU firma").small());
                    show_multiline(
                        ui,
                        &mut self.pay.onesig_hex,
                        "PSBT firmado…",
                        dark,
                        ui.available_width().min(720.0),
                        2,
                    );
                    if primary_btn(ui, "Enviar PSBT de 1 firma", dark).clicked() {
                        self.pay_send_onesig(slug, role, fee);
                    }
                }
                FundHandshakeStep::WaitChain => {
                    ui.label(
                        RichText::new(
                            "Firma y difusión en Electrum / Sparrow. La app mira Signet sola.",
                        )
                        .color(accent_amber(dark)),
                    );
                    if !self.pay.onesig_hex.is_empty() {
                        ui.label(RichText::new("PSBT de 1 firma (para Electrum)").small());
                        show_multiline(
                            ui,
                            &mut self.pay.onesig_hex,
                            "PSBT…",
                            dark,
                            ui.available_width().min(720.0),
                            2,
                        );
                    }
                    if !self.pay_chain_line.is_empty() {
                        ui.label(RichText::new(&self.pay_chain_line).color(accent_amber(dark)));
                    }
                    if self.chain_busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Consultando Signet…");
                        });
                    }
                    if primary_btn(ui, "Buscar fondeo ahora", dark).clicked() {
                        self.pay_poll_chain(slug, true);
                    }
                    ui.collapsing("Avanzado: pegar tx hex (rescate)", |ui| {
                        show_multiline(
                            ui,
                            &mut self.pay.funding_tx_hex,
                            "tx hex…",
                            dark,
                            ui.available_width().min(720.0),
                            2,
                        );
                        if ui.small_button("Verificar hex (rescate)").clicked() {
                            self.pay_verify(slug);
                        }
                    });
                }
            }
        });
    }

    fn show_pay_coin_picker(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        role: Role,
        need: u64,
        our_coin: Option<&OfferedCoin>,
    ) {
        let dark = self.prefs.dark;
        if let Some(scan) = &self.watch_scan {
            if scan.utxos.is_empty() {
                ui.label(
                    RichText::new("No hay UTXO en esta xpub. Recibe Signet y vuelve a buscar.")
                        .weak(),
                );
            } else {
                ui.label(RichText::new("Elige una moneda (la app sugiere la justa).").small());
                if need > 0 {
                    if let Some(sug) = suggest_watched(scan, need) {
                        ui.label(
                            RichText::new(format!(
                                "Sugerida: {} · {} sats{}",
                                sug.outpoint,
                                sug.sats,
                                if sug.confirmed {
                                    ""
                                } else {
                                    " (sin confirmar)"
                                }
                            ))
                            .small()
                            .weak(),
                        );
                    }
                }
                let change = scan.change.clone();
                let utxos = scan.utxos.clone();
                for u in &utxos {
                    let selected = self.pay.selected_outpoint == u.outpoint
                        || our_coin
                            .as_ref()
                            .map(|c| c.outpoint == u.outpoint)
                            .unwrap_or(false);
                    let label = format!(
                        "{} · {} sats{}",
                        u.outpoint,
                        u.sats,
                        if u.confirmed { "" } else { " · mempool" }
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        self.pay_take_utxo(slug, role, u, &change);
                    }
                }
            }
        }
        if let Some(c) = our_coin {
            ui.label(
                RichText::new(format!("Usando {} · {} sats", c.outpoint, c.sats))
                    .small()
                    .color(accent_green(dark)),
            );
        }
    }

    fn our_saved_coin(&self, slug: &str, role: Role) -> Option<OfferedCoin> {
        let coins = self.store.load_pay_coins(slug).ok()?;
        match role {
            Role::Mandante => coins.mandante,
            Role::Contratista => coins.contratista,
        }
    }

    fn pay_take_utxo(
        &mut self,
        slug: &str,
        role: Role,
        utxo: &hbp_bitcoin::WatchedUtxo,
        change: &str,
    ) {
        match coin_from_watched(role, utxo, change) {
            Ok(mut coin) => {
                if let Some(base) = self.esplora_base_or_default() {
                    let socks = self.socks();
                    let txid = utxo.outpoint.split(':').next().unwrap_or("").to_string();
                    if let Ok(hex) = esplora_tx_hex(&base, socks, &txid) {
                        coin.prev_tx_hex = Some(hex);
                    }
                }
                if let Err(e) = self.store.save_offered_coin(slug, &coin) {
                    return self.fail(e);
                }
                self.pay.selected_outpoint = utxo.outpoint.clone();
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.note(format!("Elegí {} ({} sats).", utxo.outpoint, utxo.sats));
            }
            Err(e) => self.fail(e),
        }
    }

    fn esplora_base_or_default(&self) -> Option<String> {
        if !self.esplora_base.trim().is_empty() {
            return Some(self.esplora_base.trim().trim_end_matches('/').to_string());
        }
        default_esplora_urls(PRODUCT_NETWORK)
            .first()
            .map(|s| (*s).to_string())
    }

    fn pay_scan_coins(&mut self, slug: &str) {
        let acc = match self.store.load_watch(slug, Some(self.passphrase.as_str())) {
            Ok(Some(a)) => a,
            Ok(None) => return self.fail("Guarda primero la xpub / vpub."),
            Err(e) => return self.fail(e),
        };
        let socks = self.socks();
        let urls: Vec<String> = default_esplora_urls(PRODUCT_NETWORK)
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        self.start_job("buscando monedas en Signet", move |tx| {
            let refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
            let r = (|| {
                let base = esplora_try_bases(&refs, socks).map_err(|e| e.to_string())?;
                let b = base.clone();
                let scan = scan_watch(&acc, |addr| {
                    let list = esplora_address_utxos(&b, socks, &addr.to_string())
                        .map_err(|e| hbp_bitcoin::Error::msg(e.to_string()))?;
                    list.into_iter()
                        .map(|u| {
                            let op = format!("{}:{}", u.txid, u.vout)
                                .parse()
                                .map_err(|e| hbp_bitcoin::Error::msg(format!("{e}")))?;
                            Ok((op, u.sats, u.confirmed))
                        })
                        .collect()
                })
                .map_err(|e| e.to_string())?;
                Ok((scan, base))
            })();
            let _ = tx.send(JobEvent::ScanDone(r));
        });
    }

    fn pay_start_partial(&mut self, slug: &str, role: Role, project: &hbp_core::Project) {
        let Some(coin) = self.our_saved_coin(slug, role) else {
            return self.fail("Elige una moneda de la lista.");
        };
        let fee = match parse_fee(&self.pay.fee_sats) {
            Ok(f) => f,
            Err(e) => return self.fail(e),
        };
        match build_our_partial(project, role, &coin, fee) {
            Ok(psbt) => {
                self.pay.unsigned_psbt_hex = psbt_to_hex(&psbt);
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.note("Parcial armado. Lo mando; el otro añade su moneda.");
                self.spawn_deliver(
                    NetMessage::Artifact {
                        name: ART_PARTIAL.into(),
                        json: funding_wire(&self.pay.unsigned_psbt_hex, role, fee),
                    },
                    "PSBT parcial enviado.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_complete_partial(&mut self, slug: &str, role: Role, project: &hbp_core::Project) {
        let Some(coin) = self.our_saved_coin(slug, role) else {
            return self.fail("Elige una moneda de la lista.");
        };
        let fee = match parse_fee(&self.pay.fee_sats) {
            Ok(f) => f,
            Err(e) => return self.fail(e),
        };
        match complete_incoming_partial(project, role, &coin, fee, &self.pay.unsigned_psbt_hex) {
            Ok(psbt) => {
                self.pay.unsigned_psbt_hex = psbt_to_hex(&psbt);
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.note("PSBT completo (2 inputs). Lo devuelvo. Ahora se firma fuera.");
                self.spawn_deliver(
                    NetMessage::Artifact {
                        name: ART_PSBT.into(),
                        json: funding_wire(&self.pay.unsigned_psbt_hex, role, fee),
                    },
                    "PSBT completo enviado.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_send_onesig(&mut self, slug: &str, role: Role, fee: u64) {
        let hex = self.pay.onesig_hex.trim().to_string();
        if hex.is_empty() {
            return self.fail("Pega el PSBT que firmaste en Electrum / Sparrow.");
        }
        match classify_funding_psbt(&hex) {
            Ok((_, sigs)) if sigs >= 1 => {}
            Ok(_) => {
                return self.fail("Ese PSBT no tiene firma. Fírmalo fuera y pégalo de nuevo.");
            }
            Err(e) => return self.fail(e),
        }
        self.pay.onesig_hex = hex.clone();
        self.pay.awaiting_chain = true;
        let _ = self.store.save_pay_draft(slug, &self.pay);
        self.spawn_deliver(
            NetMessage::Artifact {
                name: ART_ONESIG.into(),
                json: funding_wire(&hex, role, fee),
            },
            "PSBT de 1 firma enviado. El otro firma y difunde en Electrum.",
        );
        self.pay_chain_line = "Esperando el fondeo en Signet…".into();
    }

    fn maybe_poll_funding_chain(&mut self) {
        if self.tab != MainTab::Pago || self.chain_busy {
            return;
        }
        let Some(slug) = self.selected.clone() else {
            return;
        };
        let Ok(project) = self.store.ensure_pay_project(&slug) else {
            return;
        };
        if !matches!(pay_stage(Some(&project), None), PayStage::FundBondP1) {
            return;
        }
        if project.quote.is_none() {
            return;
        }
        if self.last_chain_poll.elapsed() < std::time::Duration::from_secs(8) {
            return;
        }
        self.pay_poll_chain(&slug, false);
    }

    fn pay_poll_chain(&mut self, slug: &str, force: bool) {
        if self.chain_busy && !force {
            return;
        }
        let project = match self.store.ensure_pay_project(slug) {
            Ok(p) => p,
            Err(_) => return,
        };
        let quote = match &project.quote {
            Some(q) if quote_fully_signed(q) => q.clone(),
            _ => return,
        };
        let Ok((bond_addr, _)) = escrow_addrs(&project) else {
            return;
        };
        let socks = self.socks();
        let urls: Vec<String> = if !self.esplora_base.is_empty() {
            vec![self.esplora_base.clone()]
        } else {
            default_esplora_urls(PRODUCT_NETWORK)
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        };
        let bond_sats = quote.bond_sats;
        let p1_sats = quote.partida_sats(1).unwrap_or(0);
        self.chain_busy = true;
        self.last_chain_poll = std::time::Instant::now();
        self.spawn_quiet(move |tx| {
            let refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
            let r = (|| {
                let base = esplora_try_bases(&refs, socks).map_err(|e| e.to_string())?;
                let txs =
                    esplora_address_txs(&base, socks, &bond_addr).map_err(|e| e.to_string())?;
                let Some(txid) = find_dual_amount_txid(&txs, bond_sats, p1_sats) else {
                    return Ok(None);
                };
                let hex = esplora_tx_hex(&base, socks, txid).map_err(|e| e.to_string())?;
                Ok(Some(hex))
            })();
            let _ = tx.send(JobEvent::ChainDone(r));
        });
    }

    fn pay_note_chain_found(&mut self) {
        let Some(slug) = self.selected.clone() else {
            return;
        };
        self.pay_verify(&slug);
    }

    fn show_pay_coop(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        _id: &Identity,
        project: &hbp_core::Project,
    ) {
        let dark = self.prefs.dark;
        panel_card(ui, dark, |ui| {
            ui.label(RichText::new("Recepción / cierre cooperativo (P1)").strong());
            ui.label(
                RichText::new(
                    "Cuando ambos están de acuerdo: destino Signet + dos rondas MuSig2 (archivo o Tor).",
                )
                .small()
                .weak(),
            );
            if let Some((txid, vout, sats)) = project.partida(1).ok().and_then(|p| p.locked_utxo())
            {
                ui.label(
                    RichText::new(format!("UTXO P1 {txid}:{vout} · {sats} sats"))
                        .small()
                        .weak(),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Destino");
                show_field(
                    ui,
                    &mut self.pay.dest,
                    "dirección Signet de cobro",
                    dark,
                    360.0,
                );
            });
            if let Ok(Some(coop)) = self.store.load_pay_coop(slug) {
                ui.label(
                    RichText::new(format!(
                        "Ronda en curso · dest {} · fee {}",
                        coop.dest, coop.fee
                    ))
                    .small(),
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if primary_btn(ui, "Proponer cierre", dark).clicked() {
                    self.pay_coop_propose(slug);
                }
                if ui.button("Firmar ronda").clicked() {
                    self.pay_coop_sign(slug);
                }
                if ui.button("Terminar y marcar pagada").clicked() {
                    self.pay_coop_finish(slug);
                }
            });
            ui.label(RichText::new("Archivo de ronda (pega JSON si no llegó por red)").small());
            show_multiline(
                ui,
                &mut self.pay.coop_paste,
                "{ … }",
                dark,
                ui.available_width().min(720.0),
                3,
            );
            if ui.button("Guardar JSON pegado").clicked() {
                self.pay_import_coop_paste(slug);
            }
        });
    }

    fn pay_price_minor(&self) -> Option<u64> {
        if let Ok(p) = price_minor_from_major(&self.pay.price_major) {
            if p > 0 {
                return Some(p);
            }
        }
        self.fx
            .as_ref()
            .map(|q| (q.btc_price_major * 100.0).round() as u64)
    }

    fn pay_fx_note(&self) -> String {
        if !self.pay.price_major.trim().is_empty() {
            return format!("manual {} / BTC", self.pay.price_major.trim());
        }
        if let Some(q) = &self.fx {
            return format!("{} {:.2} {}/BTC", q.source, q.btc_price_major, q.unit);
        }
        String::new()
    }

    fn pay_propose_quote(&mut self, slug: &str, id: &Identity, signed: &SignedContract) {
        let price =
            if signed.body.unit.is_bitcoin_denom() {
                None
            } else {
                match self.pay_price_minor() {
                    Some(p) => Some(p),
                    None => return self.fail(
                        "Consulta el precio o escribe cuánto vale 1 BTC en la moneda de la obra.",
                    ),
                }
            };
        match draft_quote(signed, price, &self.pay_fx_note())
            .and_then(|q| sign_our_quote(id, signed, q))
        {
            Ok(q) => {
                if let Err(e) = self.store.save_pay_quote(slug, &q) {
                    return self.fail(e);
                }
                if let Ok(mut project) = self.store.ensure_pay_project(slug) {
                    if let Ok(true) = lock_quote_if_ready(&mut project, &q) {
                        let _ = self.store.save_pay_project(slug, &project);
                    }
                }
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.note("Cotización firmada por ti. La mando al otro.");
                self.spawn_deliver(NetMessage::Quote { quote: q }, "Cotización enviada.");
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_sign_quote(&mut self, slug: &str, id: &Identity, signed: &SignedContract) {
        let Some(q) = self.store.load_pay_quote(slug).ok().flatten() else {
            return self.fail("Todavía no hay cotización. Propón una o espera la del otro.");
        };
        match sign_our_quote(id, signed, q) {
            Ok(q) => {
                if let Err(e) = self.store.save_pay_quote(slug, &q) {
                    return self.fail(e);
                }
                if let Ok(mut project) = self.store.ensure_pay_project(slug) {
                    match lock_quote_if_ready(&mut project, &q) {
                        Ok(true) => {
                            let _ = self.store.save_pay_project(slug, &project);
                            self.note("Cotización cerrada. Ahora: fondear boleta + partida 1.");
                        }
                        Ok(false) => self.note("Firmaste. Falta la otra firma."),
                        Err(e) => return self.fail(e),
                    }
                }
                self.spawn_deliver(NetMessage::Quote { quote: q }, "Cotización enviada.");
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_verify(&mut self, slug: &str) {
        let mut project = match self.store.ensure_pay_project(slug) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        if self.pay.funding_tx_hex.trim().is_empty() {
            return self.fail("Aún no veo el tx. Espera Esplora o pega el hex.");
        }
        match apply_verified_p1_funding(&mut project, &self.pay.funding_tx_hex) {
            Ok(txid) => {
                if let Err(e) = self.store.save_pay_project(slug, &project) {
                    return self.fail(e);
                }
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.pay_chain_line.clear();
                self.note(format!("Fondeo visto. txid {txid}. Partida 1 en curso."));
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_import_coop_paste(&mut self, slug: &str) {
        let raw = self.pay.coop_paste.trim();
        if raw.is_empty() {
            return self.fail("Pega el JSON de la ronda.");
        }
        match serde_json::from_str::<CoopFile>(raw) {
            Ok(coop) => match self.store.save_pay_coop(slug, &coop) {
                Ok(()) => self.note("Ronda guardada."),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }

    fn pay_coop_propose(&mut self, slug: &str) {
        let id = match self.store.load_identity(slug) {
            Ok(id) => id,
            Err(e) => return self.fail(e),
        };
        let project = match self.store.ensure_pay_project(slug) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let fee = match parse_fee(&self.pay.fee_sats) {
            Ok(f) => f,
            Err(e) => return self.fail(e),
        };
        if self.pay.dest.trim().is_empty() {
            return self.fail("Escribe la dirección Signet de cobro.");
        }
        let mut journal = match self.store.load_pay_nonces(slug) {
            Ok(j) => j,
            Err(e) => return self.fail(e),
        };
        match coop_propose(&id, &project, &self.pay.dest, fee, &mut journal) {
            Ok(coop) => {
                if let Err(e) = self.store.save_pay_nonces(slug, &journal) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_pay_coop(slug, &coop) {
                    return self.fail(e);
                }
                self.pay.coop_paste = serde_json::to_string_pretty(&coop).unwrap_or_default();
                let json = match serde_json::to_value(&coop) {
                    Ok(v) => v,
                    Err(e) => return self.fail(e),
                };
                self.note("Ronda propuesta. El otro debe firmar.");
                self.spawn_deliver(
                    NetMessage::Artifact {
                        name: ART_COOP.into(),
                        json,
                    },
                    "Cierre enviado.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_coop_sign(&mut self, slug: &str) {
        let id = match self.store.load_identity(slug) {
            Ok(id) => id,
            Err(e) => return self.fail(e),
        };
        let project = match self.store.ensure_pay_project(slug) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let coop = match self.store.load_pay_coop(slug) {
            Ok(Some(c)) => c,
            Ok(None) => return self.fail("No hay ronda. Pégala o espera la del otro."),
            Err(e) => return self.fail(e),
        };
        let mut journal = match self.store.load_pay_nonces(slug) {
            Ok(j) => j,
            Err(e) => return self.fail(e),
        };
        match coop_sign(&id, &project, coop, &mut journal) {
            Ok(coop) => {
                if let Err(e) = self.store.save_pay_nonces(slug, &journal) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_pay_coop(slug, &coop) {
                    return self.fail(e);
                }
                self.pay.coop_paste = serde_json::to_string_pretty(&coop).unwrap_or_default();
                let json = match serde_json::to_value(&coop) {
                    Ok(v) => v,
                    Err(e) => return self.fail(e),
                };
                self.note("Ronda firmada. Devuélvela para terminar.");
                self.spawn_deliver(
                    NetMessage::Artifact {
                        name: ART_COOP.into(),
                        json,
                    },
                    "Firma de cierre enviada.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn pay_coop_finish(&mut self, slug: &str) {
        let id = match self.store.load_identity(slug) {
            Ok(id) => id,
            Err(e) => return self.fail(e),
        };
        let mut project = match self.store.ensure_pay_project(slug) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let coop = match self.store.load_pay_coop(slug) {
            Ok(Some(c)) => c,
            Ok(None) => return self.fail("No hay ronda para terminar."),
            Err(e) => return self.fail(e),
        };
        let mut journal = match self.store.load_pay_nonces(slug) {
            Ok(j) => j,
            Err(e) => return self.fail(e),
        };
        match coop_finish(&id, &mut project, &coop, &mut journal) {
            Ok(hex) => {
                if let Err(e) = self.store.save_pay_nonces(slug, &journal) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_pay_project(slug, &project) {
                    return self.fail(e);
                }
                self.pay.funding_tx_hex = hex.clone();
                let _ = self.store.save_pay_draft(slug, &self.pay);
                self.note(
                    "Partida 1 pagada. Difunde el tx de cierre en Signet. Partida 2 queda desbloqueada.",
                );
                self.spawn_deliver(
                    NetMessage::Artifact {
                        name: ART_TX.into(),
                        json: hex_artifact(&hex),
                    },
                    "Cierre enviado.",
                );
            }
            Err(e) => self.fail(e),
        }
    }

    fn show_wallet(&mut self, ui: &mut egui::Ui, slug: &str) {
        ui.label(RichText::new("Billetera local").strong());
        ui.label(RichText::new("xpub / vpub. No se envía.").small().weak());
        show_field(
            ui,
            &mut self.xpub_draft,
            "vpub… o tpub… (Signet)",
            self.prefs.dark,
            520.0,
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new("Frase de cifrado (opcional)").small());
            show_field(
                ui,
                &mut self.passphrase,
                "si la pones, se guarda cifrada",
                self.prefs.dark,
                220.0,
            );
        });
        if ui.button("Guardar llave pública").clicked() {
            match self.store.import_xpub_local(
                slug,
                &self.xpub_draft,
                Some(self.passphrase.as_str()),
            ) {
                Ok(acc) => match address_at(&acc.receive_descriptor, 0, PRODUCT_NETWORK) {
                    Ok(a) => {
                        self.note(format!(
                            "Billetera guardada aquí. Primera dirección: {a}. Nadie más ve la xpub."
                        ));
                        self.xpub_draft.clear();
                    }
                    Err(e) => self.fail(e),
                },
                Err(e) => self.fail(e),
            }
        }
        match self.store.load_watch(slug, Some(self.passphrase.as_str())) {
            Ok(Some(acc)) => {
                if let Ok(a) = address_at(&acc.receive_descriptor, 0, PRODUCT_NETWORK) {
                    ui.label(
                        RichText::new(format!("Ya hay una billetera guardada. Dirección 0: {a}"))
                            .small()
                            .weak(),
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                ui.label(RichText::new(e.to_string()).small().weak());
            }
        }
    }

    fn show_backup(&mut self, ui: &mut egui::Ui, slug: &str) {
        ui.collapsing("Respaldo", |ui| {
            ui.label("Copia de esta obra (llave en hexadecimal, no es una frase de 12 palabras).");
            show_field(
                ui,
                &mut self.backup_path,
                "ruta del archivo (opcional)",
                self.prefs.dark,
                420.0,
            );
            ui.horizontal(|ui| {
                if ui.button("Exportar respaldo").clicked() {
                    match export_backup(&self.store, slug) {
                        Ok(b) => {
                            let path = if self.backup_path.trim().is_empty() {
                                self.store.work_dir(slug).join("backup.json")
                            } else {
                                std::path::PathBuf::from(self.backup_path.trim())
                            };
                            match write_backup_file(&path, &b) {
                                Ok(()) => self.note(format!("Respaldo en {}", path.display())),
                                Err(e) => self.fail(e),
                            }
                        }
                        Err(e) => self.fail(e),
                    }
                }
                if ui.button("Importar respaldo").clicked() {
                    if self.backup_path.trim().is_empty() {
                        self.fail("Indica la ruta del respaldo");
                    } else {
                        let path = std::path::PathBuf::from(self.backup_path.trim());
                        match read_backup_file(&path)
                            .and_then(|b| import_backup(&mut self.store, &b))
                        {
                            Ok(e) => {
                                self.note(format!("Obra importada: {}", e.name));
                                self.selected = Some(e.slug);
                            }
                            Err(e) => self.fail(friendly_store_err(&e.to_string())),
                        }
                    }
                }
            });
        });
    }

    fn build_draft(&mut self, slug: &str, id: &Identity, entry: &WorkEntry) {
        let total = match parse_major_amount(&self.total_major, self.unit) {
            Ok(v) => v,
            Err(_) => {
                return self.fail(if self.unit == Unit::Sats {
                    "El total en SATS tiene que ser un entero (ej. 100000)"
                } else {
                    "El total tiene que ser un número (ej. 100 o 100.50)"
                })
            }
        };
        let t1 = match self.t1.to_unix() {
            Ok(v) => v,
            Err(e) => return self.fail(format!("Primer plazo: {e}")),
        };
        let t2 = match self.t2.to_unix() {
            Ok(v) => v,
            Err(e) => return self.fail(format!("Segundo plazo: {e}")),
        };
        if let Err(e) = validate_deadline_order(t1, t2) {
            return self.fail(e);
        }
        let descs: Vec<String> = self
            .stage_descs
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        match draft_equal_stages(id, &entry.name, self.unit, total, t1, t2, &descs) {
            Ok(body) => match self.store.save_draft(slug, &body) {
                Ok(()) => self.note(format!(
                    "Partidas listas: {} de {} {}",
                    body.partidas.len(),
                    body.partidas
                        .first()
                        .map(|p| format_major_amount(p.amount_minor, body.unit))
                        .unwrap_or_else(|| "0".into()),
                    body.unit
                )),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(friendly_store_err(&e.to_string())),
        }
    }

    fn emit_offer(&mut self, slug: &str, id: &Identity) {
        let Some(body) = (match self.store.load_draft(slug) {
            Ok(b) => b,
            Err(e) => return self.fail(e),
        }) else {
            return self.fail("Primero prepara las partidas");
        };
        if let Err(e) = body.validate() {
            return self.fail(e);
        }
        let sk = match id.secret() {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let sig = match sign_body(&sk, &body) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let offer = Offer {
            body,
            mandante_sig: sig,
        };
        match self.store.save_offer(slug, &offer) {
            Ok(_) => self.note("Propuesta firmada. Ya puedes enviarla."),
            Err(e) => self.fail(e),
        }
    }

    fn accept_from_store(&mut self, slug: &str, id: &Identity) {
        let offer = match self.store.load_offer(slug) {
            Ok(Some(o)) => o,
            Ok(None) => return self.fail("Todavía no llega la propuesta"),
            Err(e) => return self.fail(e),
        };
        self.finish_accept(slug, id, offer);
    }

    fn accept_from_file(&mut self, slug: &str, id: &Identity) {
        let path = self.accept_path.trim();
        if path.is_empty() {
            return self.fail("Indica la ruta del archivo");
        }
        let offer: Offer = match std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(o) => o,
            None => return self.fail("No pude leer esa propuesta"),
        };
        if let Err(e) = self.store.save_offer(slug, &offer) {
            return self.fail(e);
        }
        self.finish_accept(slug, id, offer);
    }

    fn finish_accept(&mut self, slug: &str, id: &Identity, offer: Offer) {
        match contratista_accept(offer, id) {
            Ok(pending) => {
                if let Err(e) = self.store.save_draft(slug, &pending.body) {
                    return self.fail(e);
                }
                if let Err(e) = self.store.save_pending(slug, &pending) {
                    return self.fail(e);
                }
                self.note("Aceptaste. Se lo mando al mandante para que confirme.");
                self.spawn_deliver(NetMessage::Accept { pending }, "Aceptación enviada.");
            }
            Err(e) => self.fail(e),
        }
    }

    fn apply_overlay_hints(&self) {
        let Some(o) = &self.overlay else {
            return;
        };
        if let Some(found) = hbp_net::discover_socks() {
            o.set_socks(Some(found.addr));
        } else if let Ok(addr) = TorConfig::from_env()
            .socks()
            .parse::<std::net::SocketAddr>()
        {
            o.set_socks(Some(addr));
        }
        let onion = self.own_onion.trim();
        if onion.is_empty() {
            return;
        }
        if let Ok(p) = PeerAddr::parse_flexible(onion) {
            o.set_advertised(p);
        }
    }

    fn start_overlay(&mut self) -> bool {
        if self.overlay.is_some() {
            self.apply_overlay_hints();
            return true;
        }
        let mut cfg = OverlayConfig::default();
        cfg.listen = "127.0.0.1:3848".parse().expect("static");
        let handle = match OverlayHandle::bind(cfg.clone()) {
            Ok(h) => h,
            Err(_) => {
                cfg.listen = "127.0.0.1:0".parse().expect("static");
                match OverlayHandle::bind(cfg) {
                    Ok(h) => h,
                    Err(e) => {
                        self.fail(format!("No pude abrir la red local: {e}"));
                        return false;
                    }
                }
            }
        };
        self.overlay = Some(handle);
        self.apply_overlay_hints();
        true
    }

    fn connect_network(&mut self) {
        if self.net_light == NetLight::Connecting || self.busy.is_some() {
            return;
        }
        if !self.start_overlay() {
            self.net_light = NetLight::Err;
            self.net_line = "No pude abrir el descubrimiento local.".into();
            return;
        }
        let port = match &self.overlay {
            Some(o) => o.local_addr().port(),
            None => return,
        };
        let root = self.store.root.clone();
        self.net_light = NetLight::Connecting;
        self.net_line = "Preparando la red… la primera vez puede tardar un minuto.".into();
        self.note("Conectando…");
        self.start_job("conectando la red", move |tx| {
            let result = bring_up_tor_with_hint(&root, port, |s| {
                let _ = tx.send(JobEvent::Progress(s.to_string()));
            })
            .map_err(|e| format!("No pude conectar. Revisa internet y vuelve a pulsar. ({e})"));
            let _ = tx.send(JobEvent::ConnectDone(result));
        });
    }

    fn bootstrap_known_peers(&mut self) {
        let mut addrs = env_bootstrap_peers();
        for o in self.store.load_peer_book().onions {
            if let Ok(p) = PeerAddr::parse_flexible(&o) {
                addrs.push(p);
            }
        }
        if !self.peer_onion.trim().is_empty() {
            if let Ok(p) = PeerAddr::parse_flexible(&self.peer_onion) {
                addrs.push(p);
            }
        }
        if addrs.is_empty() {
            return;
        }
        let Some(overlay) = self.overlay.clone() else {
            return;
        };
        thread::Builder::new()
            .name("hbp-boot".into())
            .spawn(move || {
                let _ = overlay.bootstrap(&addrs);
            })
            .ok();
    }

    fn spawn_announce(&mut self, entry: &WorkEntry) {
        let Some(o) = self.overlay.clone() else {
            return self.fail("Primero pulsa Conectarme");
        };
        let onion = if self.own_onion.trim().is_empty() {
            o.advertised().display()
        } else {
            self.own_onion.trim().to_string()
        };
        let person = {
            let from_prefs = self.prefs.display_name().to_string();
            let from_entry = entry.publisher_name.trim().to_string();
            if !from_prefs.is_empty() {
                from_prefs
            } else {
                from_entry
            }
        };
        if person.is_empty() {
            return self.fail("Escribe tu nombre (así te busca el maestro), no el de la obra.");
        }
        let _ = self.store.set_publisher_name(&entry.slug, &person);
        let slug = entry.slug.clone();
        let work = entry.name.clone();
        let persona_topic = literal_topic(&person);
        let ann = WorkAnnounce {
            work_name: work.clone(),
            onion,
            offer_id: None,
            role: "mandante".into(),
            person_name: person.clone(),
        };
        let _ = announce_topics(&ann);
        self.start_job("publicando la obra", move |tx| {
            let dht = o.announce_work(&ann).map_err(|e| e.to_string());
            let board = o.publish_rendezvous(&ann);
            let msg = match (dht, board) {
                (Ok(_), Ok(_)) => format!(
                    "Publicado: te buscan como {person} (tema {persona_topic}), obra {work}."
                ),
                (Ok(_), Err(e)) => format!(
                    "Publicada en la red local. El tablero no respondió ({e}). Si no aparece, usen Avanzado."
                ),
                (Err(e), _) => e.to_string(),
            };
            let ok = msg.starts_with("Publicad") || msg.starts_with("Obra");
            if ok {
                let _ = tx.send(JobEvent::AnnounceDone(Ok((slug, msg))));
            } else {
                let _ = tx.send(JobEvent::AnnounceDone(Err(msg)));
            }
        });
    }

    fn spawn_lookup(&mut self) {
        let Some(o) = self.overlay.clone() else {
            return self.fail("Primero pulsa Conectarme");
        };
        let name = self.lookup_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.last_error.clear();
        self.start_job("buscando al mandante", move |tx| {
            let r = o.discover_catalog(&name).map_err(|e| e.to_string());
            let _ = tx.send(JobEvent::LookupDone(r));
        });
    }

    fn spawn_bootstrap(&mut self, raw: &str) {
        let Some(o) = self.overlay.clone() else {
            return self.fail("Primero pulsa Conectarme");
        };
        let raw = raw.trim();
        if raw.is_empty() {
            return self.fail("Falta el código de la otra persona");
        }
        let normalized = if raw.contains(':') || raw.ends_with(".onion") {
            raw.to_string()
        } else {
            format!("{raw}:80")
        };
        let peers = match parse_bootstrap_list(&normalized)
            .or_else(|_| PeerAddr::parse_flexible(&normalized).map(|p| vec![p]))
        {
            Ok(p) => p,
            Err(_) => return self.fail("Ese código no se entiende (xxxx.onion)"),
        };
        if let Some(p) = peers.first() {
            self.peer_onion = p.host.clone();
            if let Some(slug) = self.selected.clone() {
                let _ = self.store.save_peer_onion(&slug, &p.display());
            }
        }
        self.start_job("llamando a la otra persona", move |tx| {
            let r = o.bootstrap(&peers).map_err(|e| e.to_string());
            let _ = tx.send(JobEvent::BootstrapDone(r));
        });
    }

    fn spawn_send_offer(&mut self, slug: &str) {
        let offer = match self.store.load_offer(slug) {
            Ok(Some(off)) => off,
            Ok(None) => return self.fail("Primero firma la propuesta"),
            Err(e) => return self.fail(e),
        };
        if offer.body.network != PRODUCT_NETWORK {
            return self.fail("Esta propuesta no es de Signet");
        }
        self.spawn_deliver(NetMessage::Offer { offer }, "Propuesta enviada.");
    }

    fn spawn_deliver(&mut self, msg: NetMessage, ok_note: &str) {
        let Some(o) = self.overlay.clone() else {
            return self.fail("Primero pulsa Conectarme");
        };
        let dest_raw = if !self.peer_onion.trim().is_empty() {
            self.peer_onion.clone()
        } else {
            return self
                .fail("Todavía no encuentro a la otra persona. Busca la obra o pega el código.");
        };
        let dest = match PeerAddr::parse_flexible(&dest_raw) {
            Ok(p) => p,
            Err(_) => return self.fail("El código de la otra persona no se entiende"),
        };
        let note = ok_note.to_string();
        self.start_job("enviando", move |tx| {
            let r = o
                .deliver(&dest, &msg)
                .map(|_| note)
                .map_err(|e| format!("No pude enviar ({e})"));
            let _ = tx.send(JobEvent::DeliverDone(r));
        });
    }

    fn spawn_fx(&mut self) {
        let unit = self.unit;
        if unit.is_bitcoin_denom() {
            self.fx_line = "Esta moneda ya es bitcoin: no hace falta un precio.".into();
            return;
        }
        let socks = self.socks();
        self.start_job("consultando el precio", move |tx| {
            let r = quote_btc(unit, socks).map_err(|e| e.to_string());
            let _ = tx.send(JobEvent::FxDone(r));
        });
    }
}

fn friendly_store_err(e: &str) -> String {
    if e.contains("mainnet") {
        "Esta app no usa mainnet. Solo Signet.".into()
    } else if e.contains("already exists") {
        "Ya existe una obra con ese nombre.".into()
    } else if e.contains("work name") {
        "Escribe el nombre de la obra primero".into()
    } else {
        e.to_string()
    }
}

fn big_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(label).min_size(Vec2::new(220.0, 32.0)))
}

fn primary_btn(ui: &mut egui::Ui, label: &str, dark: bool) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
            .fill(accent_green(dark))
            .min_size(Vec2::new(220.0, 34.0)),
    )
}

fn panel_card(ui: &mut egui::Ui, dark: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(panel_fill(dark))
        .stroke(egui::Stroke::new(1.0, panel_stroke(dark)))
        .inner_margin(12.0)
        .rounding(6.0)
        .show(ui, add);
}

fn accent_blue(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(91, 141, 239)
    } else {
        Color32::from_rgb(43, 108, 176)
    }
}

fn accent_green(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(61, 155, 95)
    } else {
        Color32::from_rgb(47, 133, 90)
    }
}

fn accent_amber(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(212, 160, 23)
    } else {
        Color32::from_rgb(192, 86, 33)
    }
}

fn theme_red() -> Color32 {
    Color32::from_rgb(200, 64, 64)
}

fn panel_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(36, 40, 50)
    } else {
        Color32::from_rgb(255, 255, 255)
    }
}

fn panel_stroke(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(70, 78, 96)
    } else {
        Color32::from_rgb(198, 206, 220)
    }
}

fn deadline_editor(ui: &mut egui::Ui, id: &str, fields: &mut DeadlineFields) {
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut fields.day)
                .range(1..=31)
                .prefix("día "),
        );
        let month_label = MONTHS_ES
            .get(fields.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("mes");
        egui::ComboBox::from_id_salt(format!("{id}-month"))
            .selected_text(month_label)
            .show_ui(ui, |ui| {
                for (i, name) in MONTHS_ES.iter().enumerate() {
                    ui.selectable_value(&mut fields.month, i as u32 + 1, *name);
                }
            });
        ui.add(
            egui::DragValue::new(&mut fields.year)
                .range(2024..=2100)
                .prefix("año "),
        );
        ui.label("  ");
        ui.add(
            egui::DragValue::new(&mut fields.hour)
                .range(0..=23)
                .suffix(" h"),
        );
        ui.add(
            egui::DragValue::new(&mut fields.minute)
                .range(0..=59)
                .suffix(" min"),
        );
    });
    ui.label(RichText::new(fields.preview_es()).italics().small());
}

fn net_badge(ui: &mut egui::Ui, light: NetLight, _line: &str) {
    let (color, text) = match light {
        NetLight::Off => (Color32::from_rgb(140, 140, 140), "Apagado"),
        NetLight::Connecting => (Color32::from_rgb(220, 170, 40), "Conectando"),
        NetLight::Ok => (Color32::from_rgb(70, 180, 90), "En la red"),
        NetLight::Partial => (Color32::from_rgb(70, 180, 90), "En la red"),
        NetLight::Err => (Color32::from_rgb(210, 70, 70), "Error"),
    };
    ui.colored_label(color, format!("● {text}"));
}

fn edit_fg(dark: bool) -> Color32 {
    if dark {
        Color32::WHITE
    } else {
        Color32::from_rgb(12, 14, 18)
    }
}

fn edit_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 78, 94)
    } else {
        Color32::WHITE
    }
}

fn field_single<'a>(value: &'a mut String, hint: &str, dark: bool) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .hint_text(hint.to_owned())
        .text_color(edit_fg(dark))
        .frame(false)
}

fn unit_helper(unit: Unit) -> &'static str {
    match unit {
        Unit::Sats => {
            "El monto ya está en SATS. No se convierte con un precio. El pago en cadena viene después."
        }
        Unit::Btc => {
            "El monto ya está en BTC. Los sats salen de ese monto. El pago en cadena viene después."
        }
        _ => {
            "Moneda del contrato. Los sats se fijan después, al cotizar/fondear con un precio. Elegir moneda no arma el pago."
        }
    }
}

fn show_field(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    dark: bool,
    width: f32,
) -> egui::Response {
    let stroke = if dark {
        Color32::from_rgb(170, 176, 190)
    } else {
        Color32::from_rgb(90, 96, 108)
    };
    egui::Frame::none()
        .fill(edit_bg(dark))
        .stroke(egui::Stroke::new(1.0, stroke))
        .inner_margin(4.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.add(field_single(value, hint, dark).desired_width(width))
        })
        .inner
}

fn show_multiline(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    dark: bool,
    width: f32,
    rows: usize,
) -> egui::Response {
    let stroke = if dark {
        Color32::from_rgb(170, 176, 190)
    } else {
        Color32::from_rgb(90, 96, 108)
    };
    egui::Frame::none()
        .fill(edit_bg(dark))
        .stroke(egui::Stroke::new(1.0, stroke))
        .inner_margin(4.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(value)
                    .hint_text(hint.to_owned())
                    .text_color(edit_fg(dark))
                    .frame(false)
                    .desired_width(width)
                    .desired_rows(rows),
            )
        })
        .inner
}

fn paint_widgets(v: &mut egui::Visuals, fg: Color32) {
    v.widgets.noninteractive.fg_stroke.color = fg;
    v.widgets.inactive.fg_stroke.color = fg;
    v.widgets.hovered.fg_stroke.color = fg;
    v.widgets.active.fg_stroke.color = fg;
    v.widgets.open.fg_stroke.color = fg;
}

fn parse_psbt_has_outpoint(raw: &str, outpoint: &str) -> bool {
    let Ok(psbt) = parse_psbt(raw) else {
        return false;
    };
    let want = outpoint.trim();
    if want.is_empty() {
        return false;
    }
    psbt.unsigned_tx
        .input
        .iter()
        .any(|i| i.previous_output.to_string() == want)
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let fg = edit_fg(dark);
    v.override_text_color = Some(fg);
    paint_widgets(&mut v, fg);
    if dark {
        v.extreme_bg_color = Color32::from_rgb(28, 32, 42);
        v.faint_bg_color = Color32::from_rgb(32, 36, 48);
        v.widgets.inactive.bg_fill = Color32::from_rgb(44, 50, 64);
        v.widgets.hovered.bg_fill = Color32::from_rgb(58, 72, 104);
        v.widgets.active.bg_fill = Color32::from_rgb(61, 155, 95);
        v.widgets.open.bg_fill = Color32::from_rgb(44, 50, 64);
        v.window_fill = Color32::from_rgb(22, 24, 30);
        v.panel_fill = Color32::from_rgb(26, 28, 36);
        v.selection.bg_fill = Color32::from_rgb(43, 90, 176);
        v.selection.stroke.color = fg;
        v.warn_fg_color = Color32::from_rgb(212, 160, 23);
        v.error_fg_color = Color32::from_rgb(220, 80, 80);
    } else {
        v.extreme_bg_color = Color32::from_rgb(255, 255, 255);
        v.faint_bg_color = Color32::from_rgb(232, 236, 244);
        v.widgets.inactive.bg_fill = Color32::from_rgb(236, 240, 248);
        v.widgets.hovered.bg_fill = Color32::from_rgb(214, 226, 246);
        v.widgets.active.bg_fill = Color32::from_rgb(47, 133, 90);
        v.widgets.open.bg_fill = Color32::from_rgb(236, 240, 248);
        v.window_fill = Color32::from_rgb(236, 238, 244);
        v.panel_fill = Color32::from_rgb(244, 245, 247);
        v.selection.bg_fill = Color32::from_rgb(190, 214, 245);
        v.selection.stroke.color = Color32::from_rgb(20, 32, 48);
        v.warn_fg_color = Color32::from_rgb(180, 90, 20);
        v.error_fg_color = Color32::from_rgb(180, 40, 40);
    }
    ctx.set_visuals(v);
}
