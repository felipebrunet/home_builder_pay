//! Native product window (egui/eframe). Not the throwaway `hbp-ui` localhost.

use std::str::FromStr;
use std::sync::mpsc;
use std::thread;

use eframe::egui::{self, Color32, RichText};
use hbp_app::{
    default_works_root, draft_equal_stages, export_backup, format_unix_local_es, import_backup,
    read_backup_file, validate_deadline_order, write_backup_file, DeadlineFields, UiPrefs,
    WorkEntry, WorkStore, MONTHS_ES,
};
use hbp_bitcoin::{sign_body, verify_body};
use hbp_core::{
    bond_minor, minor_from_major, Offer, Role, SignedContract, Unit, DEFAULT_BOND_BPS,
    PRODUCT_NETWORK,
};
use hbp_net::{
    bring_up_tor, parse_bootstrap_list, OverlayConfig, OverlayHandle, PeerAddr, TorConfig,
    TorRuntime, WorkAnnounce,
};

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
    new_role: Role,
    total_major: String,
    unit: String,
    t1: DeadlineFields,
    t2: DeadlineFields,
    stage_descs: String,
    accept_path: String,
    backup_path: String,
    log: String,
    overlay: Option<OverlayHandle>,
    tor_rt: Option<TorRuntime>,
    onion: String,
    bootstrap: String,
    lookup_name: String,
    last_error: String,
    net_light: NetLight,
    net_line: String,
    connect_rx: Option<mpsc::Receiver<Result<TorRuntime, String>>>,
}

impl App {
    fn new() -> Self {
        let store = WorkStore::open(default_works_root()).unwrap_or_else(|_| WorkStore {
            root: default_works_root(),
            index: Default::default(),
        });
        let prefs = store.load_prefs();
        Self {
            store,
            prefs,
            selected: None,
            last_slug: None,
            new_name: String::new(),
            new_role: Role::Mandante,
            total_major: "100".into(),
            unit: "USD".into(),
            t1: DeadlineFields::days_from_now(7),
            t2: DeadlineFields::days_from_now(14),
            stage_descs: String::new(),
            accept_path: String::new(),
            backup_path: String::new(),
            log: "Signet. Árbitro apagado. Si no hay acuerdo, se quema el dinero en dos plazos.\n"
                .into(),
            overlay: None,
            tor_rt: None,
            onion: String::new(),
            bootstrap: String::new(),
            lookup_name: String::new(),
            last_error: String::new(),
            net_light: NetLight::Off,
            net_line: "Red apagada. Pulsa Conectar red cuando quieras hablar con la otra persona."
                .into(),
            connect_rx: None,
        }
    }

    fn note(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.log.push_str(&s);
        if !s.ends_with('\n') {
            self.log.push('\n');
        }
        self.last_error.clear();
    }

    fn fail(&mut self, e: impl std::fmt::Display) {
        self.last_error = e.to_string();
        self.log.push_str(&format!("error: {e}\n"));
    }

    fn selected_entry(&self) -> Option<&WorkEntry> {
        let slug = self.selected.as_deref()?;
        self.store.index.works.iter().find(|w| w.slug == slug)
    }

    fn has_draft(&self, slug: &str) -> bool {
        matches!(self.store.load_draft(slug), Ok(Some(_)))
    }

    fn has_offer(&self, slug: &str) -> bool {
        matches!(self.store.load_offer(slug), Ok(Some(_)))
    }

    fn poll_connect(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.connect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(rt)) => {
                let hint = rt.hint_es.clone();
                let onion = rt.onion.clone();
                let socks = rt.socks;
                if let Some(o) = &self.overlay {
                    o.set_socks(Some(socks));
                    if let Some(ref onion) = onion {
                        o.set_advertised(PeerAddr::new(onion.clone(), 80));
                    }
                }
                self.net_light = if onion.is_some() {
                    NetLight::Ok
                } else {
                    NetLight::Partial
                };
                self.net_line = hint.clone();
                self.note(hint);
                if let Some(onion) = onion {
                    self.onion = onion;
                }
                self.tor_rt = Some(rt);
            }
            Ok(Err(e)) => {
                self.net_light = NetLight::Err;
                self.net_line = e.clone();
                self.fail(e);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.connect_rx = Some(rx);
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.net_light = NetLight::Err;
                self.net_line =
                    "Se cortó la conexión al arrancar Tor. Vuelve a pulsar Conectar red.".into();
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.prefs.dark);
        self.poll_connect(ctx);
        if self.selected != self.last_slug {
            if let Some(slug) = self.selected.clone() {
                if let Ok(Some(draft)) = self.store.load_draft(&slug) {
                    if let Some((t1, t2)) = draft.dispute.fee_burn_deadlines() {
                        self.t1 = DeadlineFields::from_unix(t1);
                        self.t2 = DeadlineFields::from_unix(t2);
                    }
                    self.total_major = format!("{:.2}", draft.total_minor() as f64 / 100.0);
                    self.unit = format!("{:?}", draft.unit).to_uppercase();
                    if self.unit == "USD" {
                        self.unit = "USD".into();
                    }
                }
            }
            self.last_slug = self.selected.clone();
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("home_builder_pay");
                ui.separator();
                ui.label(RichText::new("Signet · boleta 10% · sin árbitro").weak());
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
            ui.add_space(4.0);
        });

        egui::SidePanel::left("works")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Tus obras");
                ui.label(
                    RichText::new("Cada obra es un contrato aparte, con su propia llave.")
                        .small()
                        .weak(),
                );
                ui.separator();
                let slugs: Vec<(String, String)> = self
                    .store
                    .index
                    .works
                    .iter()
                    .map(|w| (w.slug.clone(), format!("{} — {}", w.name, role_es(w.role))))
                    .collect();
                for (slug, label) in slugs {
                    if ui
                        .selectable_label(self.selected.as_deref() == Some(slug.as_str()), label)
                        .clicked()
                    {
                        self.selected = Some(slug);
                    }
                }
                ui.separator();
                ui.label(RichText::new("Nueva obra").strong());
                ui.label("Nombre de la obra (como lo dirías en la faena)");
                show_field(
                    ui,
                    &mut self.new_name,
                    "ej. Casa Norte, radier y muro",
                    self.prefs.dark,
                    220.0,
                );
                ui.label("Tu rol");
                ui.radio_value(
                    &mut self.new_role,
                    Role::Mandante,
                    "Mandante (quien paga)",
                );
                ui.radio_value(
                    &mut self.new_role,
                    Role::Contratista,
                    "Contratista (quien construye)",
                );
                ui.label(
                    RichText::new("Red: Signet (única). No hay mainnet.")
                        .small()
                        .weak(),
                );
                if ui.button("Crear obra").clicked() {
                    if self.new_name.trim().is_empty() {
                        self.fail("Escribe el nombre de la obra primero");
                    } else {
                        match self
                            .store
                            .create_product_work(&self.new_name, self.new_role, None)
                        {
                            Ok(e) => {
                                self.note(format!("Obra creada: {}", e.name));
                                self.selected = Some(e.slug);
                                self.new_name.clear();
                            }
                            Err(e) => self.fail(friendly_store_err(&e.to_string())),
                        }
                    }
                }
            });

        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(120.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Notas").strong());
                egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                    show_multiline(
                        ui,
                        &mut self.log,
                        "",
                        self.prefs.dark,
                        ui.available_width(),
                        4,
                    );
                });
                if !self.last_error.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 80, 80), &self.last_error);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(slug) = self.selected.clone() else {
                ui.add_space(8.0);
                ui.heading("Empieza por una obra");
                ui.label("Crea una obra a la izquierda. Ahí van el nombre, las partidas, la boleta y los plazos.");
                ui.label("La red (Tor) se conecta con un solo botón cuando quieras hablar con la otra persona.");
                return;
            };
            self.show_work(ui, &slug);
        });
    }
}

impl App {
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
        let has_draft = self.has_draft(slug);
        let has_offer = self.has_offer(slug);

        ui.heading(&entry.name);
        ui.label(format!("{} · Signet", role_es(entry.role)));
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            self.show_construction(ui, slug, &id, &entry, has_draft, has_offer);
            ui.add_space(12.0);
            self.show_network(ui, slug, &entry, has_offer);
            ui.add_space(12.0);
            self.show_backup(ui, slug);
        });
    }

    fn show_construction(
        &mut self,
        ui: &mut egui::Ui,
        slug: &str,
        id: &hbp_bitcoin::Identity,
        entry: &WorkEntry,
        has_draft: bool,
        has_offer: bool,
    ) {
        ui.heading("Construcción / obra");
        ui.label(
            "El total se parte en partidas. El 10% queda como boleta de garantía (la misma plata en cada partida: si el total es 100, la boleta es 10 y hay 10 partidas de 10).",
        );
        ui.label(
            "Si ambos están de acuerdo, se paga normal. Si no hay acuerdo, en el primer plazo se quema la mitad (va a mineros; nadie se la queda) y en el segundo plazo se quema el resto. No hay árbitro.",
        );
        ui.add_space(6.0);

        match entry.role {
            Role::Mandante => {
                ui.label(RichText::new("Paso 1 — Monto y plazos").strong());
                ui.horizontal(|ui| {
                    ui.label("Total de la obra");
                    show_field(ui, &mut self.total_major, "100", self.prefs.dark, 80.0);
                    ui.label("unidad");
                    show_field(ui, &mut self.unit, "USD", self.prefs.dark, 60.0);
                });
                if let Ok(total) = minor_from_major(&self.total_major) {
                    if let Ok(bond) = bond_minor(total, DEFAULT_BOND_BPS) {
                        let n = hbp_core::equal_stage_count(DEFAULT_BOND_BPS).unwrap_or(0);
                        ui.label(
                            RichText::new(format!(
                                "Boleta 10% = {:.2} {}. Serán {n} partidas de {:.2} (cada una = la boleta).",
                                bond as f64 / 100.0,
                                self.unit,
                                bond as f64 / 100.0
                            ))
                            .weak(),
                        );
                    }
                }

                ui.add_space(4.0);
                ui.label("Primer plazo (si no hay acuerdo, se quema la mitad)");
                deadline_editor(ui, "t1", &mut self.t1);
                ui.label("Segundo plazo (se quema el resto)");
                deadline_editor(ui, "t2", &mut self.t2);

                ui.add_space(4.0);
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
                ui.label(RichText::new("Paso 2 — Preparar partidas").strong());
                if ui.button("Preparar partidas").clicked() {
                    self.build_draft(slug, id, entry);
                }

                ui.add_space(4.0);
                ui.label(RichText::new("Paso 3 — Firmar la oferta").strong());
                if has_draft {
                    if ui.button("Firmar y guardar oferta").clicked() {
                        self.emit_offer(slug, id);
                    }
                    if has_offer {
                        ui.label(
                            RichText::new("Oferta lista. Puedes enviarla por red o por archivo.")
                                .color(Color32::from_rgb(80, 160, 100)),
                        );
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new("Firmar y guardar oferta"));
                    ui.label(
                        RichText::new("Primero prepara las partidas (paso 2).")
                            .small()
                            .weak(),
                    );
                }
            }
            Role::Contratista => {
                ui.label(RichText::new("Paso 1 — Recibir la oferta del mandante").strong());
                ui.label(
                    "Pide el archivo de oferta o conéctate a la red y lee el buzón. Luego acéptala aquí.",
                );
                show_field(
                    ui,
                    &mut self.accept_path,
                    "ruta al archivo de oferta",
                    self.prefs.dark,
                    420.0,
                );
                if ui.button("Aceptar oferta").clicked() {
                    self.accept_offer(slug, id);
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
                "Total {:.2} · boleta {:.2} (10%)",
                draft.total_minor() as f64 / 100.0,
                bond as f64 / 100.0
            ));
            egui::Grid::new("stages").striped(true).show(ui, |ui| {
                ui.strong("#");
                ui.strong("descripción");
                ui.strong("monto");
                ui.strong("= boleta");
                ui.strong("plazo");
                ui.end_row();
                for p in &draft.partidas {
                    ui.label(p.id.to_string());
                    ui.label(&p.description);
                    ui.label(format!("{:.2}", p.amount_minor as f64 / 100.0));
                    ui.label(if p.amount_minor == bond { "sí" } else { "NO" });
                    ui.label(format_unix_local_es(p.plazo_unix));
                    ui.end_row();
                }
            });
        }
    }

    fn show_network(&mut self, ui: &mut egui::Ui, slug: &str, entry: &WorkEntry, has_offer: bool) {
        ui.heading("Red");
        ui.label("Un botón. Busca Tor (Expert Bundle o Tor Browser) y enciende el descubrimiento.");
        ui.horizontal(|ui| {
            let busy = self.net_light == NetLight::Connecting;
            let label = if busy {
                "Conectando…"
            } else {
                "Conectar red (Tor + DHT)"
            };
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                self.connect_network();
            }
            net_badge(ui, self.net_light, &self.net_line);
        });
        ui.label(RichText::new(&self.net_line).italics());

        ui.add_space(4.0);
        ui.collapsing("Avanzado", |ui| {
            ui.label("Código / onion de la otra persona (para encontrarse)");
            ui.horizontal(|ui| {
                show_field(ui, &mut self.bootstrap, "xxxx.onion", self.prefs.dark, 360.0);
                let net_up = self.overlay.is_some();
                if ui
                    .add_enabled(net_up, egui::Button::new("Usar este código"))
                    .clicked()
                {
                    self.do_bootstrap();
                }
            });
            if self.overlay.is_none() {
                ui.label(
                    RichText::new("Primero pulsa Conectar red.")
                        .small()
                        .weak(),
                );
            }
            ui.horizontal(|ui| {
                ui.label("Tu código");
                show_field(
                    ui,
                    &mut self.onion,
                    "aparece al conectar",
                    self.prefs.dark,
                    360.0,
                );
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.overlay.is_some(), egui::Button::new("Anunciar esta obra"))
                    .clicked()
                {
                    self.announce_work(entry);
                }
                show_field(ui, &mut self.lookup_name, "nombre de obra", self.prefs.dark, 160.0);
                if ui
                    .add_enabled(self.overlay.is_some(), egui::Button::new("Buscar"))
                    .clicked()
                {
                    self.lookup_work();
                }
            });
            if has_offer {
                if ui
                    .add_enabled(
                        self.overlay.is_some() && !self.onion.trim().is_empty(),
                        egui::Button::new("Enviar oferta por red"),
                    )
                    .clicked()
                {
                    self.send_offer_over_net(slug);
                }
            } else if entry.role == Role::Mandante {
                ui.label(
                    RichText::new("Para enviar por red, primero firma la oferta (paso 3).")
                        .small()
                        .weak(),
                );
            }
            if ui
                .add_enabled(self.overlay.is_some(), egui::Button::new("Leer buzón"))
                .clicked()
            {
                if let Some(o) = &self.overlay {
                    let inbox = o.take_inbox();
                    if inbox.is_empty() {
                        self.note("Buzón vacío");
                    } else {
                        for m in inbox {
                            self.note(format!("Llegó: {}", m.kind()));
                        }
                    }
                }
            }
            if let Some(o) = &self.overlay {
                ui.label(
                    RichText::new(format!(
                        "escucha {} · contactos {}",
                        o.local_addr(),
                        o.peer_count()
                    ))
                    .small()
                    .weak(),
                );
            }
        });
    }

    fn show_backup(&mut self, ui: &mut egui::Ui, slug: &str) {
        ui.collapsing("Respaldo", |ui| {
            ui.label("Copia de seguridad de esta obra (llave en hexadecimal, no es una frase BIP39).");
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
                                Ok(()) => self.note(format!("Respaldo guardado en {}", path.display())),
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
                        match read_backup_file(&path).and_then(|b| import_backup(&mut self.store, &b))
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

    fn build_draft(&mut self, slug: &str, id: &hbp_bitcoin::Identity, entry: &WorkEntry) {
        let total = match minor_from_major(&self.total_major) {
            Ok(v) => v,
            Err(_) => return self.fail("El total tiene que ser un número (ej. 100 o 100.50)"),
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
        let unit = match Unit::from_str(&self.unit) {
            Ok(u) => u,
            Err(_) => return self.fail("Unidad no reconocida. Prueba USD, CLP o UF."),
        };
        let descs: Vec<String> = self
            .stage_descs
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        match draft_equal_stages(id, &entry.name, unit, total, t1, t2, &descs) {
            Ok(body) => match self.store.save_draft(slug, &body) {
                Ok(()) => self.note(format!(
                    "Partidas listas: {} de {:.2} (cada una = la boleta)",
                    body.partidas.len(),
                    body.partidas.first().map(|p| p.amount_minor as f64 / 100.0).unwrap_or(0.0)
                )),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(friendly_store_err(&e.to_string())),
        }
    }

    fn emit_offer(&mut self, slug: &str, id: &hbp_bitcoin::Identity) {
        let Some(body) = (match self.store.load_draft(slug) {
            Ok(b) => b,
            Err(e) => return self.fail(e),
        }) else {
            return self.fail("Primero prepara las partidas (paso 2)");
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
            Ok(_) => self.note("Oferta firmada y guardada. Ya puedes enviarla."),
            Err(e) => self.fail(e),
        }
    }

    fn apply_overlay_hints(&self) {
        let Some(o) = &self.overlay else {
            return;
        };
        if let Some(found) = hbp_net::discover_socks() {
            o.set_socks(Some(found.addr));
        } else if let Ok(addr) = TorConfig::from_env().socks().parse::<std::net::SocketAddr>() {
            o.set_socks(Some(addr));
        }
        let onion = self.onion.trim();
        if onion.is_empty() {
            return;
        }
        let parsed = PeerAddr::parse(onion).or_else(|_| PeerAddr::parse(&format!("{onion}:80")));
        if let Ok(p) = parsed {
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
        if self.net_light == NetLight::Connecting {
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
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("hbp-tor".into())
            .spawn(move || {
                let _ = tx.send(bring_up_tor(&root, port).map_err(|e| {
                    format!("No pude conectar Tor. Abre Tor Browser o pon tor.exe junto a la app. ({e})")
                }));
            })
            .ok();
        self.connect_rx = Some(rx);
        self.net_light = NetLight::Connecting;
        self.net_line = "Buscando Tor (Expert Bundle en 9050 o Tor Browser en 9150)…".into();
        self.note("Conectando red…");
    }

    fn do_bootstrap(&mut self) {
        let Some(o) = &self.overlay else {
            return self.fail("Primero pulsa Conectar red");
        };
        let raw = self.bootstrap.trim();
        if raw.is_empty() {
            return self.fail("Pega el código de la otra persona (termina en .onion)");
        }
        let normalized = if raw.contains(':') {
            raw.to_string()
        } else {
            format!("{raw}:80")
        };
        let peers = match parse_bootstrap_list(&normalized) {
            Ok(p) => p,
            Err(_) => {
                return self.fail("Ese código no se entiende. Debe verse como xxxx.onion");
            }
        };
        if peers.is_empty() {
            return self.fail("Pega el código de la otra persona (termina en .onion)");
        }
        match o.bootstrap(&peers) {
            Ok(n) if n > 0 => self.note("Encontré a la otra persona"),
            Ok(_) => self.fail("No respondió. ¿Está conectada y el código es el de ella?"),
            Err(e) => self.fail(format!("No pude llegar a la otra persona ({e})")),
        }
    }

    fn announce_work(&mut self, entry: &WorkEntry) {
        let Some(o) = &self.overlay else {
            return self.fail("Primero pulsa Conectar red");
        };
        let onion = if self.onion.trim().is_empty() {
            o.advertised().display()
        } else {
            self.onion.trim().to_string()
        };
        let ann = WorkAnnounce {
            work_name: entry.name.clone(),
            onion,
            offer_id: None,
            role: format!("{:?}", entry.role).to_lowercase(),
        };
        match o.announce_work(&ann) {
            Ok(_) => self.note("Obra anunciada. La otra persona puede buscarla por nombre."),
            Err(e) => self.fail(e),
        }
    }

    fn lookup_work(&mut self) {
        let Some(o) = &self.overlay else {
            return self.fail("Primero pulsa Conectar red");
        };
        let name = if self.lookup_name.trim().is_empty() {
            return self.fail("Escribe el nombre de la obra a buscar");
        } else {
            self.lookup_name.trim().to_string()
        };
        match o.lookup_work(&name) {
            Ok(Some(ann)) => {
                self.note(format!("Encontré «{}»", ann.work_name));
                if ann.onion.contains(".onion") || ann.onion.contains(':') {
                    self.onion = ann.onion;
                }
            }
            Ok(None) => self.fail("No aparece. Comparte el código (Avanzado) con la otra persona."),
            Err(e) => self.fail(e),
        }
    }

    fn send_offer_over_net(&mut self, slug: &str) {
        let Some(o) = &self.overlay else {
            return self.fail("Primero pulsa Conectar red");
        };
        let offer = match self.store.load_offer(slug) {
            Ok(Some(off)) => off,
            Ok(None) => return self.fail("Primero firma la oferta (paso 3)"),
            Err(e) => return self.fail(e),
        };
        if offer.body.network != PRODUCT_NETWORK {
            return self.fail("Esta oferta no es de Signet");
        }
        let dest = match PeerAddr::parse(&self.onion) {
            Ok(p) => p,
            Err(_) => match PeerAddr::parse(&format!("{}:80", self.onion.trim())) {
                Ok(p) => p,
                Err(_) => {
                    return self.fail("Falta el código de la otra persona (Avanzado)");
                }
            },
        };
        match o.deliver(&dest, &hbp_net::NetMessage::Offer { offer }) {
            Ok(()) => self.note("Oferta enviada"),
            Err(e) => self.fail(format!("No pude enviar la oferta ({e})")),
        }
    }

    fn accept_offer(&mut self, slug: &str, id: &hbp_bitcoin::Identity) {
        let path = self.accept_path.trim();
        if path.is_empty() {
            return self.fail("Indica la ruta del archivo de oferta");
        }
        let offer: Offer = match std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(o) => o,
            None => return self.fail("No pude leer esa oferta. ¿La ruta es correcta?"),
        };
        if offer.body.network != PRODUCT_NETWORK {
            return self.fail("Esta oferta no es de Signet");
        }
        if let Err(e) = verify_body(
            &offer.body.mandante_pubkey,
            &offer.mandante_sig,
            &offer.body,
        ) {
            return self.fail(e);
        }
        let mut body = offer.body;
        body.contratista_pubkey = Some(id.public_key.clone());
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
        let signed = SignedContract {
            body,
            mandante_sig: offer.mandante_sig,
            contratista_sig: sig,
        };
        let out = self.store.work_dir(slug).join("01-accepted.pending.json");
        match serde_json::to_string_pretty(&signed) {
            Ok(j) => match std::fs::write(&out, j) {
                Ok(()) => self.note("Oferta aceptada. Pásasela al mandante (archivo o red)."),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }
}

fn role_es(role: Role) -> &'static str {
    match role {
        Role::Mandante => "Mandante (quien paga)",
        Role::Contratista => "Contratista (quien construye)",
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
        NetLight::Ok => (Color32::from_rgb(70, 180, 90), "Conectado"),
        NetLight::Partial => (Color32::from_rgb(70, 180, 90), "Conectado"),
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
