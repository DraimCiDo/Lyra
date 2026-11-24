use eframe::egui::{self, Color32, RichText, Stroke};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

pub(super) const LOGO: &[u8] = include_bytes!("../../../assets/app.png");
const LOGO_VECTOR: &[u8] = include_bytes!("../../../assets/app.svg");
pub(super) const TEXT: Color32 = Color32::from_rgb(240, 240, 251);
pub(super) const MUTED: Color32 = Color32::from_rgb(174, 183, 207);
pub(in crate::v2) const ACCENT: Color32 = Color32::from_rgb(182, 171, 255);
pub(in crate::v2) const SUCCESS: Color32 = Color32::from_rgb(111, 224, 177);
pub(in crate::v2) const WARNING: Color32 = Color32::from_rgb(255, 204, 133);
pub(in crate::v2) const ERROR: Color32 = Color32::from_rgb(255, 155, 167);
const BORDER: Color32 = Color32::from_rgb(57, 65, 92);

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Appearance {
    pub(super) reduce_motion: bool,
    pub(super) transparent_window: bool,
}

impl Appearance {
    pub(super) fn load(folder: &Path) -> Self {
        let path = folder.join("appearance.json");
        if fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= 4096)
            && let Ok(bytes) = fs::read(&path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        Self::default()
    }

    pub(super) fn save(self, folder: &Path) -> io::Result<()> {
        fs::create_dir_all(folder)?;
        let file = tempfile::NamedTempFile::new_in(folder)?;
        serde_json::to_writer(file.as_file(), &self)?;
        file.as_file().sync_all()?;
        file.persist(folder.join("appearance.json"))
            .map_err(|error| error.error)?;
        Ok(())
    }
}

pub(super) fn configure(ctx: &egui::Context, reduce_motion: bool) {
    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        animation_time: if reduce_motion { 0.0 } else { 0.16 },
        ..Default::default()
    };
    let v = &mut style.visuals;
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::from_rgb(28, 33, 53);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = egui::CornerRadius::same(14);
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.extreme_bg_color = Color32::from_rgb(19, 23, 40);
    v.faint_bg_color = Color32::from_rgb(34, 39, 61);
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(MUTED);
    v.selection.bg_fill = Color32::from_rgb(55, 48, 103);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    for (widget, fill, border) in [
        (&mut v.widgets.noninteractive, (30, 35, 56), BORDER),
        (&mut v.widgets.inactive, (35, 40, 63), BORDER),
        (
            &mut v.widgets.hovered,
            (49, 53, 83),
            Color32::from_rgb(125, 115, 177),
        ),
        (&mut v.widgets.active, (59, 49, 106), ACCENT),
        (&mut v.widgets.open, (47, 49, 77), BORDER),
    ] {
        let fill = Color32::from_rgb(fill.0, fill.1, fill.2);
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0, border);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
        widget.corner_radius = egui::CornerRadius::same(8);
    }
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style.spacing.interact_size.y = 29.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.combo_width = 125.0;
    for (text_style, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 12.5),
        (egui::TextStyle::Heading, 25.0),
    ] {
        style
            .text_styles
            .insert(text_style, egui::FontId::proportional(size));
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub(super) fn background(ctx: &egui::Context, transparent_window: bool) {
    let rect = ctx.content_rect();
    let alpha = if transparent_window { 224 } else { 255 };
    let mut mesh = egui::Mesh::default();
    for (point, (r, g, b)) in [
        (rect.left_top(), (24, 24, 48)),
        (rect.right_top(), (34, 35, 65)),
        (rect.right_bottom(), (14, 23, 40)),
        (rect.left_bottom(), (18, 20, 37)),
        (rect.center(), (24, 28, 49)),
    ] {
        mesh.colored_vertex(point, Color32::from_rgba_unmultiplied(r, g, b, alpha));
    }
    for index in 0..4 {
        mesh.add_triangle(index, (index + 1) % 4, 4);
    }
    ctx.layer_painter(egui::LayerId::background()).add(mesh);
}

pub(super) fn logo(ui: &mut egui::Ui, size: f32) {
    ui.add(
        egui::Image::from_bytes("bytes://lyra-v2-logo.svg", LOGO_VECTOR)
            .fit_to_exact_size(egui::Vec2::splat(size))
            .alt_text("Lyra"),
    );
}

pub(in crate::v2) fn surface() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(38, 44, 70, 190))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(14)
        .inner_margin(14)
}

pub(in crate::v2) fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

pub(in crate::v2) fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        RichText::new(text)
            .color(Color32::from_rgb(22, 18, 53))
            .strong(),
    )
    .fill(ACCENT)
    .min_size(egui::vec2(104.0, 32.0))
}

pub(in crate::v2) fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.4)))
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().strong().color(color))
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_survives_restart_and_invalid_preferences_use_defaults() {
        let folder = tempfile::tempdir().unwrap();
        let appearance = Appearance {
            reduce_motion: true,
            transparent_window: true,
        };
        appearance.save(folder.path()).unwrap();
        let saved = Appearance::load(folder.path());
        assert!(saved.reduce_motion && saved.transparent_window);
        Appearance::default().save(folder.path()).unwrap();
        assert!(!Appearance::load(folder.path()).reduce_motion);
        fs::write(folder.path().join("appearance.json"), b"{bad json").unwrap();
        let recovered = Appearance::load(folder.path());
        assert!(!recovered.reduce_motion && !recovered.transparent_window);
    }
}
