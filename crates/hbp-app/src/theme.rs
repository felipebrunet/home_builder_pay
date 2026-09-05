//! Light/dark colors shared by the window.

use eframe::egui::Color32;

pub fn accent_blue(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(91, 141, 239)
    } else {
        Color32::from_rgb(43, 108, 176)
    }
}

pub fn accent_green(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(61, 155, 95)
    } else {
        Color32::from_rgb(47, 133, 90)
    }
}

pub fn accent_amber(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(212, 160, 23)
    } else {
        Color32::from_rgb(192, 86, 33)
    }
}

pub fn theme_red() -> Color32 {
    Color32::from_rgb(200, 64, 64)
}

pub fn panel_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(36, 40, 50)
    } else {
        Color32::from_rgb(255, 255, 255)
    }
}

pub fn panel_stroke(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(70, 78, 96)
    } else {
        Color32::from_rgb(198, 206, 220)
    }
}

pub fn edit_fg(dark: bool) -> Color32 {
    if dark {
        Color32::WHITE
    } else {
        Color32::from_rgb(12, 14, 18)
    }
}

pub fn edit_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 78, 94)
    } else {
        Color32::WHITE
    }
}

pub fn muted(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(150, 156, 170)
    } else {
        Color32::from_rgb(110, 116, 128)
    }
}
