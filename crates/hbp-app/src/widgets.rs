//! Shared egui chrome: cards, primary CTA, fields, obra stepper.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use hbp_core::Unit;

use crate::datetime::{DeadlineFields, MONTHS_ES};
use crate::pay::{format_money_edit, obra_lane_labels, parse_money_input, ObraLane};
use crate::theme::{
    accent_amber, accent_green, accent_green_hover, accent_green_pressed, edit_bg, edit_fg, muted,
    panel_fill, panel_stroke,
};

pub fn big_btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).strong().size(16.0))
            .rounding(10.0)
            .min_size(Vec2::new(220.0, 40.0)),
    )
}

pub fn primary_btn(ui: &mut egui::Ui, label: &str, dark: bool) -> egui::Response {
    primary_btn_enabled(ui, label, dark, true)
}

pub fn primary_btn_enabled(
    ui: &mut egui::Ui,
    label: &str,
    dark: bool,
    enabled: bool,
) -> egui::Response {
    let w = ui.available_width().min(440.0).max(260.0);
    let size = Vec2::new(w, 40.0);
    let resp = ui.allocate_response(size, Sense::click());
    let rect = resp.rect;
    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    let fill = if !enabled {
        Color32::from_rgb(110, 118, 128)
    } else if pressed {
        accent_green_pressed(dark)
    } else if hovered {
        accent_green_hover(dark)
    } else {
        accent_green(dark)
    };
    let grow = if pressed {
        -1.0
    } else if hovered {
        1.0
    } else {
        0.0
    };
    let r = rect.expand(grow);
    if enabled && !pressed {
        let shadow = Rect::from_min_max(
            Pos2::new(r.min.x + 1.0, r.min.y + 2.0),
            Pos2::new(r.max.x + 1.0, r.max.y + 3.0),
        );
        ui.painter().rect_filled(
            shadow,
            10.0,
            Color32::from_black_alpha(if dark { 80 } else { 28 }),
        );
    }
    ui.painter().rect_filled(r, 10.0, fill);
    if hovered && !pressed {
        ui.painter().rect_stroke(
            r,
            10.0,
            Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 50)),
        );
    }
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(16.0),
        if enabled {
            Color32::WHITE
        } else {
            Color32::from_rgb(210, 214, 220)
        },
    );
    if enabled {
        resp
    } else {
        resp.on_disabled_hover_text("No está disponible ahora")
    }
}

pub fn ghost_btn(ui: &mut egui::Ui, label: &str, dark: bool) -> egui::Response {
    let size = Vec2::new(ui.available_width().min(280.0).max(140.0), 36.0);
    let resp = ui.allocate_response(size, Sense::click());
    let rect = resp.rect;
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    let fill = if pressed {
        if dark {
            Color32::from_rgb(50, 58, 70)
        } else {
            Color32::from_rgb(226, 232, 240)
        }
    } else if hovered {
        if dark {
            Color32::from_rgb(44, 50, 62)
        } else {
            Color32::from_rgb(240, 244, 248)
        }
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(
            1.2,
            if pressed {
                accent_green(dark)
            } else {
                panel_stroke(dark)
            },
        ),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        edit_fg(dark),
    );
    resp
}

pub fn panel_card(ui: &mut egui::Ui, dark: bool, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    egui::Frame::none()
        .fill(panel_fill(dark))
        .stroke(egui::Stroke::new(1.0, panel_stroke(dark)))
        .inner_margin(18.0)
        .rounding(12.0)
        .show(ui, |ui| {
            ui.set_min_width((w - 8.0).max(200.0));
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add);
        });
}

pub fn show_obra_stepper(ui: &mut egui::Ui, lane: ObraLane, dark: bool) {
    let labels = obra_lane_labels();
    let current = lane as usize;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, label) in labels.iter().enumerate() {
            let done = i < current;
            let here = i == current;
            let (color, text, size) = if here {
                (accent_amber(dark), format!("● {label}"), 16.0)
            } else if done {
                (accent_green(dark), format!("✓ {label}"), 14.0)
            } else {
                (muted(dark), (*label).to_string(), 14.0)
            };
            ui.label(RichText::new(text).color(color).strong().size(size));
            if i + 1 < labels.len() {
                ui.label(RichText::new("→").color(muted(dark)));
            }
        }
    });
}

pub fn show_pot_badges(ui: &mut egui::Ui, boleta: &str, p1: &str, dark: bool) {
    ui.horizontal_wrapped(|ui| {
        badge(ui, "Boleta", boleta, dark);
        badge(ui, "Partida 1", p1, dark);
    });
}

fn badge(ui: &mut egui::Ui, name: &str, state: &str, dark: bool) {
    let (fill, fg) = badge_colors(state, dark);
    egui::Frame::none()
        .fill(fill)
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{name}: {state}"))
                    .color(fg)
                    .size(13.0)
                    .strong(),
            );
        });
}

fn badge_colors(state: &str, dark: bool) -> (Color32, Color32) {
    match state {
        "fondeada" | "cobrada" | "devuelta" | "deshecha" => (
            if dark {
                Color32::from_rgb(28, 56, 38)
            } else {
                Color32::from_rgb(220, 242, 226)
            },
            accent_green(dark),
        ),
        "en curso" | "fondeando" | "pendiente" => (
            if dark {
                Color32::from_rgb(56, 46, 22)
            } else {
                Color32::from_rgb(255, 240, 214)
            },
            accent_amber(dark),
        ),
        s if s.starts_with("quema") => (
            if dark {
                Color32::from_rgb(64, 32, 32)
            } else {
                Color32::from_rgb(255, 228, 228)
            },
            Color32::from_rgb(200, 64, 64),
        ),
        _ => (
            if dark {
                Color32::from_rgb(44, 48, 58)
            } else {
                Color32::from_rgb(236, 238, 244)
            },
            muted(dark),
        ),
    }
}

pub fn deadline_editor(ui: &mut egui::Ui, id: &str, fields: &mut DeadlineFields) {
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

pub fn unit_helper(unit: Unit) -> &'static str {
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

fn field_single<'a>(value: &'a mut String, hint: &str, dark: bool) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .hint_text(hint.to_owned())
        .text_color(edit_fg(dark))
        .frame(false)
}

pub fn show_field(
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
        .inner_margin(6.0)
        .rounding(6.0)
        .show(ui, |ui| {
            ui.add(field_single(value, hint, dark).desired_width(width))
        })
        .inner
}

pub fn show_money_field(
    ui: &mut egui::Ui,
    value: &mut String,
    unit: Unit,
    hint: &str,
    dark: bool,
    width: f32,
) -> egui::Response {
    let resp = show_field(ui, value, hint, dark, width);
    if resp.lost_focus() {
        if let Ok(minor) = parse_money_input(value, unit) {
            *value = format_money_edit(minor, unit);
        }
    }
    resp
}

pub fn show_multiline(
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
        .inner_margin(6.0)
        .rounding(6.0)
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

pub fn paint_widgets(v: &mut egui::Visuals, fg: Color32) {
    v.widgets.noninteractive.fg_stroke.color = fg;
    v.widgets.inactive.fg_stroke.color = fg;
    v.widgets.hovered.fg_stroke.color = fg;
    v.widgets.active.fg_stroke.color = fg;
    v.widgets.open.fg_stroke.color = fg;
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
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
