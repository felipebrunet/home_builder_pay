//! Native product window (egui/eframe). Not the throwaway `hbp-ui` localhost.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, RichText, Vec2};
use hbp_app::{
    contratista_accept, default_works_root, draft_equal_stages, export_backup,
    format_unix_local_es, import_backup, import_signed, mandante_commit, next_step,
    read_backup_file, validate_deadline_order, write_backup_file, DeadlineFields, NextKind,
    UiPrefs, WorkEntry, WorkProgress, WorkStore, MONTHS_ES,
};
use hbp_bitcoin::{address_at, sign_body, Identity};
use hbp_core::{
    bond_minor, format_major_amount, parse_major_amount, Offer, Role, SignedContract, Unit,
    DEFAULT_BOND_BPS, PRODUCT_NETWORK,
};
use hbp_net::{
    bring_up_tor_with_hint, env_bootstrap_peers, parse_bootstrap_list, preview_sats, quote_btc,
    FxQuote, NetMessage, OverlayConfig, OverlayHandle, PeerAddr, TorConfig, TorRuntime,
    WorkAnnounce,
};

enum JobEvent {
    Progress(String),
    ConnectDone(Result<TorRuntime, String>),
    AnnounceDone(Result<String, String>),
    LookupDone(Result<Option<WorkAnnounce>, String>),
    BootstrapDone(Result<usize, String>),
    DeliverDone(Result<String, String>),
    FxDone(Result<FxQuote, String>),
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
    job_rx: Option<mpsc::Receiver<JobEvent>>,
    fx: Option<FxQuote>,
    fx_line: String,
    name_draft: String,
    last_log: String,
    log_hits: u32,
    change_profile: bool,
    picking_role: bool,
}

impl App {
    fn new() -> Self {
        let store = WorkStore::open(default_works_root()).unwrap_or_else(|_| WorkStore {
            root: default_works_root(),
            index: Default::default(),
        });
        let prefs = store.load_prefs();
        let picking_role = prefs.first_run();
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
            job_rx: None,
            fx: None,
            fx_line: String::new(),
            name_draft: String::new(),
            last_log: String::new(),
            log_hits: 0,
            change_profile: false,
            picking_role,
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
        let (tx, rx) = mpsc::channel();
        self.job_rx = Some(rx);
        self.busy = Some(thinking.into());
        thread::Builder::new()
            .name("hbp-job".into())
            .spawn(move || f(tx))
            .ok();
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.job_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(JobEvent::Progress(s)) => {
                self.net_line = s;
                self.job_rx = Some(rx);
                ctx.request_repaint();
            }
            Ok(JobEvent::ConnectDone(Ok(rt))) => {
                self.busy = None;
                self.apply_tor(rt);
            }
            Ok(JobEvent::ConnectDone(Err(e))) => {
                self.busy = None;
                self.net_light = NetLight::Err;
                self.net_line = e.clone();
                self.fail(e);
            }
            Ok(JobEvent::AnnounceDone(Ok(msg))) => {
                self.busy = None;
                self.note(msg);
            }
            Ok(JobEvent::AnnounceDone(Err(e))) => {
                self.busy = None;
                self.fail(e);
            }
            Ok(JobEvent::LookupDone(Ok(Some(ann)))) => {
                self.busy = None;
                self.on_found_work(ann);
            }
            Ok(JobEvent::LookupDone(Ok(None))) => {
                self.busy = None;
                self.fail(
                    "No aparece. ¿El mandante se conectó y publicó? Prueba su nombre (Don José) o el de la obra. Si no, usen Avanzado.",
                );
            }
            Ok(JobEvent::LookupDone(Err(e))) => {
                self.busy = None;
                self.fail(e);
            }
            Ok(JobEvent::BootstrapDone(Ok(n))) => {
                self.busy = None;
                if n > 0 {
                    self.note("Ya estamos en contacto.");
                } else {
                    self.fail("No respondió. ¿Está conectada?");
                }
            }
            Ok(JobEvent::BootstrapDone(Err(e))) => {
                self.busy = None;
                self.fail(e);
            }
            Ok(JobEvent::DeliverDone(Ok(msg))) => {
                self.busy = None;
                self.note(msg);
            }
            Ok(JobEvent::DeliverDone(Err(e))) => {
                self.busy = None;
                self.fail(e);
            }
            Ok(JobEvent::FxDone(Ok(q))) => {
                self.busy = None;
                self.fx_line =
                    format!("1 BTC ≈ {:.2} {} ({})", q.btc_price_major, q.unit, q.source);
                self.fx = Some(q);
            }
            Ok(JobEvent::FxDone(Err(e))) => {
                self.busy = None;
                self.fx_line = format!(
                    "Sin precio ahora ({e}). Sigue igual: el trato es en la moneda de la obra."
                );
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.job_rx = Some(rx);
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = None;
            }
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
                }
            }
        }
    }

    fn on_found_work(&mut self, ann: WorkAnnounce) {
        let who = if ann.person_name.trim().is_empty() {
            ann.work_name.clone()
        } else {
            format!("{} · {}", ann.person_name, ann.work_name)
        };
        self.note(format!("Encontré a {who}"));
        let onion = ann.onion.trim().to_string();
        if !onion.is_empty() {
            self.peer_onion = onion.clone();
            let peer = if ann.person_name.trim().is_empty() {
                None
            } else {
                Some(ann.person_name.as_str())
            };
            if let Ok(entry) = self.store.ensure_contratista_work(&ann.work_name, peer) {
                let _ = self.store.save_peer_onion(&entry.slug, &onion);
                self.selected = Some(entry.slug);
                self.prefs.role = Role::Contratista;
                let _ = self.store.save_prefs(&self.prefs);
            }
            self.spawn_bootstrap(&onion);
        }
    }

    fn drain_inbox(&mut self) {
        let Some(o) = self.overlay.clone() else {
            return;
        };
        let inbox = o.take_inbox();
        for msg in inbox {
            match msg {
                NetMessage::Offer { offer } => self.on_inbox_offer(offer),
                NetMessage::Accept { pending } => self.on_inbox_accept(pending),
                NetMessage::Commit { signed } => self.on_inbox_commit(signed),
                other => self.note(format!("Llegó un recado ({})", other.kind())),
            }
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
                self.note("Llegó la propuesta del mandante. Revísala y pulsa Aceptar trato.");
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
                self.note("Trato cerrado. Las dos partes ya firmaron.");
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
                self.note("El mandante confirmó. El trato está cerrado.");
            }
            Err(e) => self.fail(e),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.prefs.dark);
        self.poll_jobs(ctx);
        self.drain_inbox();
        if self.selected != self.last_slug {
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
                }
            }
            self.last_slug = self.selected.clone();
        }

        if self.picking_role || self.prefs.needs_name() || self.change_profile {
            egui::CentralPanel::default().show(ctx, |ui| {
                self.show_onboarding(ui);
            });
            return;
        }

        self.show_header(ctx);

        match self.prefs.role {
            Role::Mandante => {
                egui::SidePanel::left("mandante-obras")
                    .resizable(true)
                    .default_width(260.0)
                    .show(ctx, |ui| {
                        self.show_mandante_sidebar(ui);
                    });
            }
            Role::Contratista => {
                egui::SidePanel::left("contratista-tratos")
                    .resizable(true)
                    .default_width(260.0)
                    .show(ctx, |ui| {
                        self.show_contratista_sidebar(ui);
                    });
            }
        }

        self.show_log_panel(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(slug) = self.selected.clone() else {
                self.show_role_home(ui);
                return;
            };
            self.show_work(ui, &slug);
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
                self.name_draft = self.prefs.mandante_name.clone();
                let _ = self.store.save_prefs(&self.prefs);
            }
            ui.add_space(6.0);
            if big_btn(ui, "Yo construyo — Contratista").clicked() {
                self.prefs.role = Role::Contratista;
                self.picking_role = false;
                self.change_profile = false;
                self.selected = None;
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

    fn show_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let name = self.prefs.display_name().to_string();
                ui.heading(&name);
                ui.label(
                    RichText::new(match self.prefs.role {
                        Role::Mandante => "Mandante · quien paga",
                        Role::Contratista => "Contratista · quien construye",
                    })
                    .weak(),
                );
                if ui.small_button("Cambiar perfil").clicked() {
                    self.change_profile = true;
                    self.picking_role = true;
                    self.selected = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.prefs.dark {
                        "Tema: oscuro"
                    } else {
                        "Tema: claro"
                    };
                    if ui.button(label).clicked() {
                        self.prefs.dark = !self.prefs.dark;
                        let _ = self.store.save_prefs(&self.prefs);
                    }
                    net_badge(ui, self.net_light, &self.net_line);
                });
            });
            if let Some(b) = &self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(format!("Pensando… {b}")).italics());
                });
            }
            ui.add_space(2.0);
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

    fn show_mandante_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mis obras");
        ui.label(
            RichText::new("Las casas o faenas que tú publicas. Tú no eres una carpeta.")
                .small()
                .weak(),
        );
        ui.separator();
        self.work_list(ui, false);
        ui.separator();
        ui.label(RichText::new("Nueva obra").strong());
        ui.label("Nombre de la faena, no el tuyo");
        show_field(
            ui,
            &mut self.new_name,
            "ej. Casa Norte",
            self.prefs.dark,
            220.0,
        );
        if big_btn(ui, "Crear obra").clicked() {
            self.create_mandante_work();
        }
    }

    fn show_contratista_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Buscar mandante");
        ui.label(
            RichText::new("Escribe cómo le dicen (Don José). Verás su obra, no un código raro.")
                .small()
                .weak(),
        );
        let busy = self.busy.is_some() || self.net_light == NetLight::Connecting;
        if self.overlay.is_none() {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(if busy { "Conectando…" } else { "Conectarme" })
                        .min_size(Vec2::new(200.0, 32.0)),
                )
                .clicked()
            {
                self.connect_network();
            }
        }
        show_field(
            ui,
            &mut self.lookup_name,
            "nombre del mandante",
            self.prefs.dark,
            220.0,
        );
        if ui
            .add_enabled(
                self.overlay.is_some() && self.busy.is_none(),
                egui::Button::new("Buscar").min_size(Vec2::new(200.0, 32.0)),
            )
            .clicked()
        {
            self.spawn_lookup();
        }
        ui.separator();
        ui.label(RichText::new("Mis tratos").strong());
        ui.label(
            RichText::new("Obras que ya encontraste o aceptaste.")
                .small()
                .weak(),
        );
        self.work_list(ui, true);
    }

    fn show_role_home(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        match self.prefs.role {
            Role::Mandante => {
                ui.heading(format!("Hola, {}", self.prefs.display_name()));
                ui.label(
                    "Crea una obra a la izquierda. Ahí van el total, las partidas y los plazos.",
                );
                ui.label("Cuando esté lista, la propuesta se la mandas al maestro por la red.");
            }
            Role::Contratista => {
                ui.heading(format!("Hola, {}", self.prefs.display_name()));
                ui.label(
                    "Tú no creas la obra. Buscas al mandante por su nombre y esperas su propuesta.",
                );
                ui.label("Cuando llegue, aparece Aceptar.");
            }
        }
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
                self.note(format!("Obra creada: {}", e.name));
                self.selected = Some(e.slug);
                self.new_name.clear();
            }
            Err(e) => self.fail(friendly_store_err(&e.to_string())),
        }
    }

    fn show_work(&mut self, ui: &mut egui::Ui, slug: &str) {
        let entry = match self.selected_entry().cloned() {
            Some(e) => e,
            None => return,
        };
        let id = match self.store.load_identity(slug) {
            Ok(id) => id,
            Err(e) => {
                ui.colored_label(Color32::RED, e.to_string());
                return;
            }
        };
        let has_draft = matches!(self.store.load_draft(slug), Ok(Some(_)));
        let has_offer = matches!(self.store.load_offer(slug), Ok(Some(_)));
        let has_pending = matches!(self.store.load_pending(slug), Ok(Some(_)));
        let has_signed = matches!(self.store.load_signed(slug), Ok(Some(_)));

        ui.heading(&entry.name);
        if !entry.peer_name.is_empty() {
            ui.label(format!("Trato con {}", entry.peer_name));
        } else {
            ui.label(match entry.role {
                Role::Mandante => "Tu obra",
                Role::Contratista => "Trato (aún sin nombre del mandante)",
            });
        }
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            self.show_next_card(
                ui,
                slug,
                &id,
                &entry,
                has_draft,
                has_offer,
                has_pending,
                has_signed,
            );
            ui.add_space(10.0);
            self.show_network_simple(ui, &entry);
            ui.add_space(10.0);
            self.show_construction(ui, slug, &id, &entry, has_draft, has_offer, has_signed);
            ui.add_space(10.0);
            self.show_wallet(ui, slug);
            ui.add_space(10.0);
            self.show_backup(ui, slug);
        });
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
        };
        let step = next_step(entry.role, progress);
        let fill = if self.prefs.dark {
            Color32::from_rgb(36, 48, 40)
        } else {
            Color32::from_rgb(232, 240, 232)
        };
        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(70, 140, 80)))
            .inner_margin(10.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Qué hacer ahora").strong());
                ui.label(&step.sentence);
                if let Some(label) = step.button {
                    if big_btn(ui, label).clicked() {
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
                self.spawn_lookup();
            }
            NextKind::None => {}
        }
    }

    fn show_network_simple(&mut self, ui: &mut egui::Ui, entry: &WorkEntry) {
        ui.heading("La red");
        ui.label("Un botón te pone en contacto. El maestro te busca por tu nombre.");
        ui.horizontal(|ui| {
            let busy = self.busy.is_some() || self.net_light == NetLight::Connecting;
            let label = if busy { "Conectando…" } else { "Conectarme" };
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(label).min_size(Vec2::new(160.0, 32.0)),
                )
                .clicked()
            {
                self.connect_network();
            }
            net_badge(ui, self.net_light, &self.net_line);
        });
        ui.label(RichText::new(&self.net_line).italics());

        match entry.role {
            Role::Mandante => {
                if ui
                    .add_enabled(
                        self.overlay.is_some() && self.busy.is_none(),
                        egui::Button::new("Publicar esta obra"),
                    )
                    .clicked()
                {
                    self.spawn_announce(entry);
                }
                ui.label(
                    RichText::new("El maestro busca este mismo nombre. Si no aparece, usen el código de respaldo.")
                        .small()
                        .weak(),
                );
            }
            Role::Contratista => {
                ui.horizontal(|ui| {
                    show_field(
                        ui,
                        &mut self.lookup_name,
                        "nombre de la obra",
                        self.prefs.dark,
                        220.0,
                    );
                    if ui
                        .add_enabled(
                            self.overlay.is_some() && self.busy.is_none(),
                            egui::Button::new("Buscar"),
                        )
                        .clicked()
                    {
                        if self.lookup_name.trim().is_empty() {
                            self.lookup_name = entry.name.clone();
                        }
                        self.spawn_lookup();
                    }
                });
            }
        }

        ui.collapsing("Avanzado — código de respaldo", |ui| {
            ui.label("Solo si el nombre no aparece. Pega el código de la otra persona.");
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
                    320.0,
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
        ui.heading("La obra");
        ui.label(
            "El total se parte en partidas. El 10% es la boleta (la misma plata en cada partida: total 100 → boleta 10 → 10 partidas de 10).",
        );
        ui.label(
            "Si ambos están de acuerdo, se paga normal. Si no, en el primer plazo se quema la mitad y en el segundo el resto. Nadie se queda esa plata.",
        );

        if has_signed {
            ui.label(
                RichText::new("Trato cerrado: las dos partes firmaron.")
                    .color(Color32::from_rgb(80, 160, 100)),
            );
        }

        match entry.role {
            Role::Mandante => {
                ui.add_space(6.0);
                ui.label(RichText::new("1. Monto y plazos").strong());
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
                ui.label(RichText::new(unit_helper(self.unit)).small().weak());
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
                ui.label("Primer plazo (si no hay acuerdo, se quema la mitad)");
                deadline_editor(ui, "t1", &mut self.t1);
                ui.label("Segundo plazo (se quema el resto)");
                deadline_editor(ui, "t2", &mut self.t2);
                ui.label("Nombres de las partidas (una por línea, opcional)");
                show_multiline(
                    ui,
                    &mut self.stage_descs,
                    "Radier\nMuros\nTechumbre",
                    self.prefs.dark,
                    520.0,
                    3,
                );
                ui.add_space(6.0);
                if big_btn(ui, "Preparar partidas").clicked() {
                    self.build_draft(slug, id, entry);
                }
                ui.add_space(4.0);
                if has_draft {
                    if big_btn(ui, "Firmar propuesta").clicked() {
                        self.emit_offer(slug, id);
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new("Firmar propuesta"));
                }
                if has_offer {
                    ui.label(
                        RichText::new("Propuesta lista.").color(Color32::from_rgb(80, 160, 100)),
                    );
                    if big_btn(ui, "Enviar propuesta").clicked() {
                        self.spawn_send_offer(slug);
                    }
                    ui.label(
                        RichText::new("Se manda por la red. El archivo queda de respaldo.")
                            .small()
                            .weak(),
                    );
                }
            }
            Role::Contratista => {
                ui.add_space(6.0);
                if has_offer || has_draft {
                    ui.label(
                        RichText::new(
                            "Llegó una propuesta. Mírala abajo y acepta si estás de acuerdo.",
                        )
                        .strong(),
                    );
                    if big_btn(ui, "Aceptar trato").clicked() {
                        self.accept_from_store(slug, id);
                    }
                } else {
                    ui.label("Cuando el mandante envíe, aparece aquí. También puedes abrir un archivo de respaldo.");
                    show_field(
                        ui,
                        &mut self.accept_path,
                        "archivo de propuesta (opcional)",
                        self.prefs.dark,
                        420.0,
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

    fn show_wallet(&mut self, ui: &mut egui::Ui, slug: &str) {
        ui.heading("Tu billetera (solo en este aparato)");
        ui.label(
            "Pega la clave pública de tu billetera (xpub / vpub). Sirve para ver tus monedas y direcciones después. No se la enviamos a nadie.",
        );
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
        let ann = WorkAnnounce {
            work_name: entry.name.clone(),
            onion,
            offer_id: None,
            role: "mandante".into(),
            person_name: self.prefs.display_name().to_string(),
        };
        self.start_job("publicando la obra", move |tx| {
            let dht = o.announce_work(&ann).map_err(|e| e.to_string());
            let board = o.publish_rendezvous(&ann);
            let msg = match (dht, board) {
                (Ok(_), Ok(_)) => {
                    "Obra publicada. El maestro te busca por tu nombre o por el de la obra."
                        .into()
                }
                (Ok(_), Err(e)) => format!(
                    "Publicada en la red local. El tablero público no respondió ({e}). Si no aparece, usen el código de respaldo."
                ),
                (Err(e), _) => e.to_string(),
            };
            let ok = msg.starts_with("Obra") || msg.starts_with("Publicada");
            if ok {
                let _ = tx.send(JobEvent::AnnounceDone(Ok(msg)));
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
            return self.fail("Escribe el nombre del mandante (o de la obra)");
        }
        self.start_job("buscando al mandante", move |tx| {
            let r = o.discover_work(&name).map_err(|e| e.to_string());
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
        v.extreme_bg_color = Color32::from_rgb(72, 78, 94);
        v.faint_bg_color = Color32::from_rgb(28, 32, 40);
        v.widgets.inactive.bg_fill = Color32::from_rgb(36, 40, 50);
        v.widgets.hovered.bg_fill = Color32::from_rgb(50, 56, 70);
        v.widgets.active.bg_fill = Color32::from_rgb(50, 56, 70);
        v.widgets.open.bg_fill = Color32::from_rgb(50, 56, 70);
        v.window_fill = Color32::from_rgb(20, 22, 28);
        v.panel_fill = Color32::from_rgb(20, 22, 28);
        v.selection.bg_fill = Color32::from_rgb(50, 90, 160);
        v.selection.stroke.color = fg;
    } else {
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(236, 238, 242);
        v.widgets.inactive.bg_fill = Color32::from_rgb(242, 244, 247);
        v.widgets.hovered.bg_fill = Color32::from_rgb(226, 230, 238);
        v.widgets.active.bg_fill = Color32::from_rgb(226, 230, 238);
        v.widgets.open.bg_fill = Color32::from_rgb(226, 230, 238);
        v.window_fill = Color32::from_rgb(248, 249, 251);
        v.panel_fill = Color32::from_rgb(248, 249, 251);
        v.selection.bg_fill = Color32::from_rgb(180, 205, 245);
        v.selection.stroke.color = fg;
    }
    ctx.set_visuals(v);
}
