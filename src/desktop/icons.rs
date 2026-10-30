//! Lucide SVGs, pinned and embedded. SVG/texture cache is reused across frames and themes.
use eframe::egui::{self, Color32};
#[derive(Clone, Copy)]
pub enum Icon {
    ArrowUp,
    NewChat,
    FolderPlus,
    BookOpen,
    Download,
    Settings,
    Back,
    Folder,
    Model,
    Cpu,
    Workers,
    Palette,
    Monitor,
    Play,
    Stop,
    Check,
    Save,
    Log,
    Sparkles,
    Help,
    Close,
    Sun,
    Moon,
}
impl Icon {
    pub const ALL: [Self; 23] = [
        Self::ArrowUp,
        Self::NewChat,
        Self::FolderPlus,
        Self::BookOpen,
        Self::Download,
        Self::Settings,
        Self::Back,
        Self::Folder,
        Self::Model,
        Self::Cpu,
        Self::Workers,
        Self::Palette,
        Self::Monitor,
        Self::Play,
        Self::Stop,
        Self::Check,
        Self::Save,
        Self::Log,
        Self::Sparkles,
        Self::Help,
        Self::Close,
        Self::Sun,
        Self::Moon,
    ];
    pub fn asset(self) -> (&'static str, &'static [u8]) {
        macro_rules! svg {
            ($file:literal) => {
                (
                    concat!("bytes://lucide/", $file, ".svg"),
                    include_bytes!(concat!("../../assets/lucide/", $file, ".svg")) as &[u8],
                )
            };
        }
        match self {
            Self::ArrowUp => svg!("arrow-up"),
            Self::NewChat => svg!("message-square-plus"),
            Self::FolderPlus => svg!("folder-plus"),
            Self::BookOpen => svg!("book-open"),
            Self::Download => svg!("download"),
            Self::Settings => svg!("settings"),
            Self::Back => svg!("undo-2"),
            Self::Folder => svg!("folder"),
            Self::Model => svg!("file-code"),
            Self::Cpu => svg!("cpu"),
            Self::Workers => svg!("users"),
            Self::Palette => svg!("palette"),
            Self::Monitor => svg!("monitor"),
            Self::Play => svg!("play"),
            Self::Stop => svg!("square"),
            Self::Check => svg!("check"),
            Self::Save => svg!("save"),
            Self::Log => svg!("scroll-text"),
            Self::Sparkles => svg!("sparkles"),
            Self::Help => svg!("circle-question-mark"),
            Self::Close => svg!("x"),
            Self::Sun => svg!("sun"),
            Self::Moon => svg!("moon"),
        }
    }
    pub fn image(self, color: Color32) -> egui::Image<'static> {
        let (uri, _) = self.asset();
        // CurrentColor is resolved to white once; shader tint handles all palette changes.
        egui::Image::from_bytes(uri, white_svg(self))
            .fit_to_exact_size(egui::vec2(18.0, 18.0))
            .tint(color)
    }
}
fn white_svg(icon: Icon) -> &'static [u8] {
    use std::{collections::HashMap, sync::OnceLock};
    static ASSETS: OnceLock<HashMap<&'static str, Vec<u8>>> = OnceLock::new();
    let assets = ASSETS.get_or_init(|| {
        Icon::ALL
            .into_iter()
            .map(|i| {
                let (k, b) = i.asset();
                (
                    k,
                    String::from_utf8_lossy(b)
                        .replace("currentColor", "#ffffff")
                        .into_bytes(),
                )
            })
            .collect()
    });
    assets
        .get(icon.asset().0)
        .map(Vec::as_slice)
        .unwrap_or(icon.asset().1)
}
pub fn button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    color: Color32,
) -> egui::Response {
    let response = ui
        .add(
            egui::Button::image(icon.image(color))
                .selected(selected)
                .min_size(egui::vec2(34.0, 30.0)),
        )
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response
}
pub fn action(ui: &mut egui::Ui, icon: Icon, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Button::image_and_text(icon.image(color), text))
}
pub fn toggle(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    animations: bool,
    accent: Color32,
    on_accent: Color32,
) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(38.0, 22.0), egui::Sense::click());
        if response.clicked() {
            response.request_focus();
        }
        let key = response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        if response.clicked() || key {
            *value = !*value;
            response.mark_changed();
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *value, label)
        });
        let t = ui.ctx().animate_bool_with_time(
            response.id,
            *value,
            if animations { 0.14 } else { 0.0 },
        );
        let off = ui.visuals().widgets.inactive.bg_fill;
        let color = egui::lerp(egui::Rgba::from(off)..=egui::Rgba::from(accent), t);
        ui.painter().rect_filled(rect, 11.0, Color32::from(color));
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                12.0,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Outside,
            );
        }
        let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
        ui.painter().circle_filled(
            egui::pos2(x, rect.center().y),
            7.0,
            Color32::from(egui::lerp(
                egui::Rgba::from(ui.visuals().text_color())..=egui::Rgba::from(on_accent),
                t,
            )),
        );
        ui.label(label);
        response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toggle_can_be_clicked_then_activated_with_keyboard() {
        let ctx = egui::Context::default();
        super::super::theme::configure(&ctx, super::super::theme::Palette::Violet, false);
        let mut value = false;
        let mut rect = egui::Rect::NOTHING;
        let mut frame = |events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 460.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    rect = toggle(
                        ui,
                        &mut value,
                        "Workers",
                        false,
                        Color32::LIGHT_GREEN,
                        Color32::BLACK,
                    )
                    .rect;
                },
            );
            out.textures_delta.clear();
            (value, rect)
        };
        let (_, rect) = frame(Vec::new());
        let pos = rect.center();
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        assert!(
            frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default()
            }])
            .0
        );
        assert!(
            !frame(vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default()
            }])
            .0
        );
        assert!(
            !frame(vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Default::default()
            }])
            .0
        );
    }
    #[test]
    fn embedded_svg_and_application_icon_decode_offline() {
        for icon in Icon::ALL {
            let image =
                egui_extras::image::load_svg_bytes(white_svg(icon), &Default::default()).unwrap();
            assert_eq!(image.size, [24, 24]);
            assert!(image.pixels.iter().any(|p| p.a() > 0));
        }
        let app =
            eframe::icon_data::from_png_bytes(include_bytes!("../../assets/app.png")).unwrap();
        assert_eq!((app.width, app.height), (256, 256));
    }
}
