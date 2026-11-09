//! Small widget kit shared by every screen: cards, setting rows, chips, segmented control.
//! No logic lives here; widgets only draw and report interaction.
use super::{
    icons::Icon,
    theme::{Colors, RADIUS},
};
use eframe::egui::{self, Color32, Response, RichText, Sense, Stroke, Ui, Vec2};

pub fn card(c: Colors) -> egui::Frame {
    egui::Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(RADIUS + 2)
        .inner_margin(16)
}

/// A titled group of controls. `body` receives a ui that is as wide as the card.
pub fn section<R>(
    ui: &mut Ui,
    c: Colors,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.add_space(4.0);
    if !title.is_empty() {
        ui.label(RichText::new(title).strong().size(15.0));
    }
    if !description.is_empty() {
        ui.label(RichText::new(description).small().color(c.muted));
    }
    ui.add_space(2.0);
    card(c)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 2.0).max(0.0));
            body(ui)
        })
        .inner
}

/// Title and description on the left, control on the right. Wraps on narrow windows.
pub fn setting_row(
    ui: &mut Ui,
    c: Colors,
    title: &str,
    description: &str,
    control: impl FnOnce(&mut Ui),
) {
    let narrow = ui.available_width() < 520.0;
    let label = |ui: &mut Ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).strong());
            if !description.is_empty() {
                ui.label(RichText::new(description).small().color(c.muted));
            }
        });
    };
    if narrow {
        ui.vertical(|ui| {
            label(ui);
            control(ui);
        });
    } else {
        ui.horizontal_top(|ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                Vec2::new(width * 0.55, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                label,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), control);
        });
    }
}

pub fn divider(ui: &mut Ui, c: Colors) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, c.border);
    ui.add_space(4.0);
}

/// Rounded status pill. Returns a response so it can double as a button.
pub fn chip(
    ui: &mut Ui,
    c: Colors,
    icon: Option<Icon>,
    text: &str,
    active: bool,
    tint: Option<Color32>,
) -> Response {
    let color = tint.unwrap_or(if active { c.accent } else { c.muted });
    let fill = if active { c.accent_soft() } else { c.input };
    let mut button = match icon {
        Some(icon) => egui::Button::image_and_text(
            icon.image(color).fit_to_exact_size(Vec2::splat(14.0)),
            RichText::new(text).small().color(c.text),
        ),
        None => egui::Button::new(RichText::new(text).small().color(c.text)),
    };
    button = button
        .fill(fill)
        .stroke(Stroke::new(1.0, if active { c.accent } else { c.border }))
        .corner_radius(99)
        .min_size(Vec2::new(0.0, 24.0));
    ui.add(button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Mutually exclusive choice rendered as a joined pill.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    c: Colors,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(c.input)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(RADIUS)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (option, label) in options {
                    let selected = *value == *option;
                    let button = egui::Button::new(RichText::new(*label).color(if selected {
                        c.on_accent
                    } else {
                        c.text
                    }))
                    .fill(if selected {
                        c.accent
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(RADIUS - 3);
                    if ui.add(button).clicked() && !selected {
                        *value = *option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

pub fn primary_button(
    ui: &mut Ui,
    c: Colors,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
) -> Response {
    let label = RichText::new(text).color(c.on_accent).strong();
    let button = match icon {
        Some(i) => egui::Button::image_and_text(i.image(c.on_accent), label),
        None => egui::Button::new(label),
    }
    .fill(c.accent)
    .stroke(Stroke::NONE)
    .min_size(Vec2::new(0.0, 32.0));
    ui.add_enabled(enabled, button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn ghost_button(
    ui: &mut Ui,
    c: Colors,
    icon: Option<Icon>,
    text: &str,
    enabled: bool,
) -> Response {
    let label = RichText::new(text).color(c.text);
    let button = match icon {
        Some(i) => egui::Button::image_and_text(i.image(c.muted), label),
        None => egui::Button::new(label),
    }
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::new(1.0, c.border));
    ui.add_enabled(enabled, button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .corner_radius(99)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });
}

pub fn kbd(ui: &mut Ui, c: Colors, text: &str) {
    egui::Frame::new()
        .fill(c.input)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().monospace().color(c.muted));
        });
}

/// Large clickable tile used for suggestions and theme previews.
pub fn tile(
    ui: &mut Ui,
    c: Colors,
    selected: bool,
    size: Vec2,
    content: impl FnOnce(&mut Ui),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let fill = if selected {
        c.accent_soft()
    } else if hovered {
        c.hover
    } else {
        c.card
    };
    ui.painter().rect(
        rect,
        RADIUS,
        fill,
        Stroke::new(1.0, if selected { c.accent } else { c.border }),
        egui::StrokeKind::Inside,
    );
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(12.0))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    content(&mut inner);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square icon button for the navigation rail; the selected item gets a soft pill and an accent edge.
pub fn rail_button(ui: &mut Ui, c: Colors, icon: Icon, tip: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, 12, if selected { c.accent_soft() } else { c.hover });
    }
    if selected {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() - 8.0, rect.center().y - 10.0),
            Vec2::new(3.0, 20.0),
        );
        ui.painter().rect_filled(bar, 2, c.accent);
    }
    let color = if selected { c.accent } else { c.muted };
    icon.image(color).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), Vec2::splat(21.0)),
    );
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Flat list row used by the conversation list and the command palette.
pub fn list_row(ui: &mut Ui, c: Colors, selected: bool, text: &str, secondary: &str) -> Response {
    let height = if secondary.is_empty() { 32.0 } else { 46.0 };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, 8, if selected { c.accent_soft() } else { c.hover });
    }
    let clip = ui
        .painter()
        .with_clip_rect(rect.shrink2(Vec2::new(8.0, 0.0)));
    let font = egui::FontId::proportional(14.0);
    if secondary.is_empty() {
        clip.text(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            c.text,
        );
    } else {
        clip.text(
            egui::pos2(rect.left() + 10.0, rect.top() + 14.0),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            c.text,
        );
        clip.text(
            egui::pos2(rect.left() + 10.0, rect.top() + 32.0),
            egui::Align2::LEFT_CENTER,
            secondary,
            egui::FontId::proportional(12.0),
            c.muted,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
