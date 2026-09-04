//! Native product window (egui/eframe). Not the throwaway `hbp-ui` localhost.

use std::str::FromStr;

use eframe::egui::{self, Color32, RichText};
use hbp_app::{
    default_works_root, draft_equal_stages, export_backup, import_backup, read_backup_file,
    write_backup_file, WorkEntry, WorkStore,
};
use hbp_bitcoin::{sign_body, verify_body};
use hbp_core::{
    bond_minor, minor_from_major, Offer, Role, SignedContract, Unit, ARBITER_ENABLED,
    DEFAULT_BOND_BPS,
};
use hbp_net::{tor_status, DhtNode, TorConfig, WorkAnnounce, FILE_FALLBACK};

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("home_builder_pay"),
        ..Default::default()
    };
    eframe::run_native(
        "home_builder_pay",
        opts,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

struct App {
    store: WorkStore,
    selected: Option<String>,
    new_name: String,
    new_role: Role,
    new_network: String,
    total_major: String,
    unit: String,
    t1: String,
    t2: String,
    stage_descs: String,
    accept_path: String,
    backup_path: String,
    log: String,
    dht: DhtNode,
    onion: String,
    last_error: String,
}

impl App {
    fn new() -> Self {
        let store = WorkStore::open(default_works_root()).unwrap_or_else(|_| WorkStore {
            root: default_works_root(),
            index: Default::default(),
        });
        Self {
            store,
            selected: None,
            new_name: String::new(),
            new_role: Role::Mandante,
            new_network: "signet".into(),
            total_major: "100".into(),
            unit: "USD".into(),
            t1: String::new(),
            t2: String::new(),
            stage_descs: String::new(),
            accept_path: String::new(),
            backup_path: String::new(),
            log: "Producto nativo (Windows). Árbitro: off. Disputa: fee-burn t1/t2.\n".into(),
            dht: DhtNode::new([0x68, 0x62, 0x70, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
            onion: String::new(),
            last_error: String::new(),
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("home_builder_pay");
                ui.separator();
                ui.label(RichText::new("obra + boleta 10%  ·  fee-burn t1/t2  ·  Tor + DHT").weak());
                if !ARBITER_ENABLED {
                    ui.label(RichText::new("árbitro off").color(Color32::from_rgb(180, 140, 80)));
                }
            });
        });

        egui::SidePanel::left("works")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.heading("Obras");
                ui.label(
                    RichText::new("Una clave secp256k1 por obra")
                        .small()
                        .weak(),
                );
                ui.separator();
                let slugs: Vec<(String, String)> = self
                    .store
                    .index
                    .works
                    .iter()
                    .map(|w| (w.slug.clone(), format!("{} ({:?})", w.name, w.role)))
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
                ui.label("Nueva obra");
                ui.text_edit_singleline(&mut self.new_name);
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.new_role, Role::Mandante, "mandante");
                    ui.radio_value(&mut self.new_role, Role::Contratista, "contratista");
                });
                ui.horizontal(|ui| {
                    ui.label("red");
                    ui.text_edit_singleline(&mut self.new_network);
                });
                if ui.button("Crear obra + identidad").clicked() {
                    match hbp_core::Network::from_str(&self.new_network) {
                        Ok(net) => match self.store.create_work(
                            &self.new_name,
                            self.new_role,
                            net,
                            None,
                        ) {
                            Ok(e) => {
                                self.note(format!(
                                    "obra '{}' ({}) — identidad nueva",
                                    e.name, e.slug
                                ));
                                self.selected = Some(e.slug);
                                self.new_name.clear();
                            }
                            Err(e) => self.fail(e),
                        },
                        Err(e) => self.fail(e),
                    }
                }
            });

        egui::TopBottomPanel::bottom("log")
            .resizable(true)
            .default_height(140.0)
            .show(ctx, |ui| {
                ui.label("Registro");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.log)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
                if !self.last_error.is_empty() {
                    ui.colored_label(Color32::from_rgb(220, 80, 80), &self.last_error);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(slug) = self.selected.clone() else {
                ui.label("Elige o crea una obra. Cada obra tiene su propia identidad (no un xpub).");
                ui.label(
                    "hbp-ui (localhost:3847) no es este producto — es un wizard de prueba.",
                );
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
        ui.heading(&entry.name);
        ui.horizontal(|ui| {
            ui.label(format!("rol {:?}", entry.role));
            ui.separator();
            ui.label(format!("red {:?}", entry.network));
            ui.separator();
            ui.monospace(&id.public_key);
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.collapsing("Oferta (mandante) — partidas = boleta", |ui| {
                ui.label(format!(
                    "Boleta fija {} bps (10%). El total se parte en {} partidas iguales a la boleta.",
                    DEFAULT_BOND_BPS,
                    hbp_core::equal_stage_count(DEFAULT_BOND_BPS).unwrap_or(0)
                ));
                ui.horizontal(|ui| {
                    ui.label("total");
                    ui.text_edit_singleline(&mut self.total_major);
                    ui.label("unidad");
                    ui.text_edit_singleline(&mut self.unit);
                });
                ui.horizontal(|ui| {
                    ui.label("t1 (unix)");
                    ui.text_edit_singleline(&mut self.t1);
                    ui.label("t2 (unix)");
                    ui.text_edit_singleline(&mut self.t2);
                });
                ui.label("descripciones (una por línea, opcional; si vacío: Partida 1…N)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.stage_descs)
                        .desired_rows(4)
                        .desired_width(480.0),
                );
                if ui.button("Armar draft fee-burn (stage = bond)").clicked() {
                    self.build_draft(slug, &id, &entry);
                }
                if ui.button("Firmar y escribir 00-offer.json").clicked() {
                    self.emit_offer(slug, &id);
                }
            });

            ui.collapsing("Aceptar oferta (contratista) / importar archivo", |ui| {
                ui.label("Ruta a 00-offer.json (fallback de archivos; Tor cuando el onion esté listo)");
                ui.text_edit_singleline(&mut self.accept_path);
                if ui.button("Aceptar oferta").clicked() {
                    self.accept_offer(slug, &id);
                }
            });

            if let Ok(Some(draft)) = self.store.load_draft(slug) {
                ui.separator();
                ui.heading("Tablero de partidas");
                let bond = bond_minor(draft.total_minor(), draft.bond_bps).unwrap_or(0);
                ui.label(format!(
                    "total_minor={}  boleta={}  policy={:?}",
                    draft.total_minor(),
                    bond,
                    draft.dispute
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
                        ui.label(p.amount_minor.to_string());
                        ui.label(if p.amount_minor == bond { "sí" } else { "NO" });
                        ui.label(p.plazo_unix.to_string());
                        ui.end_row();
                    }
                });
            }

            ui.separator();
            ui.heading("Red: Tor + DHT");
            ui.label(format!(
                "Canal de contrato: mismos JSON que el CLI. Fallback: {FILE_FALLBACK}."
            ));
            ui.horizontal(|ui| {
                ui.label("onion propio (conocido, va en el anuncio)");
                ui.text_edit_singleline(&mut self.onion);
            });
            if ui.button("Probar SOCKS Tor").clicked() {
                let st = tor_status(&TorConfig::from_env());
                self.note(format!(
                    "tor socks={} reachable={} — {}",
                    st.socks, st.reachable, st.detail
                ));
                if let Some(p) = st.suggested_tor_binary {
                    self.note(format!("tor.exe encontrado: {p}"));
                } else {
                    self.note(
                        "no hay tor.exe al lado del exe; ver docs/WINDOWS.md (Tor Expert Bundle)",
                    );
                }
            }
            if ui.button("Anunciar obra en DHT local").clicked() {
                let ann = WorkAnnounce {
                    work_name: entry.name.clone(),
                    onion: self.onion.clone(),
                    offer_id: None,
                    role: format!("{:?}", entry.role).to_lowercase(),
                };
                match self.dht.announce_work(&ann) {
                    Ok(k) => self.note(format!("dht put {} (local; WAN aún no)", hex::encode(k))),
                    Err(e) => self.fail(e),
                }
            }
            ui.label(format!("registros DHT en este proceso: {}", self.dht.len()));

            ui.separator();
            ui.heading("Respaldo");
            ui.label("Exporta identidad (hex 256 bit) + draft/offer. No es BIP39.");
            ui.text_edit_singleline(&mut self.backup_path);
            ui.horizontal(|ui| {
                if ui.button("Exportar backup").clicked() {
                    match export_backup(&self.store, slug) {
                        Ok(b) => {
                            let path = if self.backup_path.trim().is_empty() {
                                self.store.work_dir(slug).join("backup.json")
                            } else {
                                std::path::PathBuf::from(self.backup_path.trim())
                            };
                            match write_backup_file(&path, &b) {
                                Ok(()) => self.note(format!("backup {}", path.display())),
                                Err(e) => self.fail(e),
                            }
                        }
                        Err(e) => self.fail(e),
                    }
                }
                if ui.button("Importar backup").clicked() {
                    let path = std::path::PathBuf::from(self.backup_path.trim());
                    match read_backup_file(&path).and_then(|b| import_backup(&mut self.store, &b)) {
                        Ok(e) => {
                            self.note(format!("importada obra '{}'", e.name));
                            self.selected = Some(e.slug);
                        }
                        Err(e) => self.fail(e),
                    }
                }
            });
        });
    }

    fn build_draft(&mut self, slug: &str, id: &hbp_bitcoin::Identity, entry: &WorkEntry) {
        let total = match minor_from_major(&self.total_major) {
            Ok(v) => v,
            Err(e) => return self.fail(e),
        };
        let t1: u32 = match self.t1.trim().parse() {
            Ok(v) => v,
            Err(_) => return self.fail("t1 debe ser unix time"),
        };
        let t2: u32 = match self.t2.trim().parse() {
            Ok(v) => v,
            Err(_) => return self.fail("t2 debe ser unix time"),
        };
        let unit = match Unit::from_str(&self.unit) {
            Ok(u) => u,
            Err(e) => return self.fail(e),
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
                    "draft {} partidas × {} (boleta = partida)",
                    body.partidas.len(),
                    body.partidas.first().map(|p| p.amount_minor).unwrap_or(0)
                )),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }

    fn emit_offer(&mut self, slug: &str, id: &hbp_bitcoin::Identity) {
        let Some(body) = (match self.store.load_draft(slug) {
            Ok(b) => b,
            Err(e) => return self.fail(e),
        }) else {
            return self.fail("no hay draft");
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
            Ok(p) => self.note(format!("offer {}", p.display())),
            Err(e) => self.fail(e),
        }
    }

    fn accept_offer(&mut self, slug: &str, id: &hbp_bitcoin::Identity) {
        let path = self.accept_path.trim();
        if path.is_empty() {
            return self.fail("indica la ruta del offer");
        }
        let offer: Offer = match std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(o) => o,
            None => return self.fail("no pude leer el offer"),
        };
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
                Ok(()) => self.note(format!(
                    "pending {} — pásalo al mandante (archivo o Tor)",
                    out.display()
                )),
                Err(e) => self.fail(e),
            },
            Err(e) => self.fail(e),
        }
    }
}
