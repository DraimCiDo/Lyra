//! Conversation presentation and bounded, factual activity history.
use super::{appearance, code_blocks};
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

pub(super) struct Activity {
    started: Instant,
    finished: Option<Duration>,
    steps: Vec<String>,
    failed: bool,
}

impl Activity {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            finished: None,
            steps: Vec::new(),
            failed: false,
        }
    }

    pub fn record(&mut self, step: &str) {
        let step: String = step.chars().take(400).collect();
        if !step.is_empty() && self.steps.last() != Some(&step) {
            if self.steps.len() == 64 {
                self.steps.remove(0);
            }
            self.steps.push(step);
        }
    }

    pub fn fail(&mut self, reason: &str) {
        self.failed = true;
        self.record(reason);
    }

    pub fn finish(&mut self) {
        if self.finished.is_none() {
            self.finished = Some(self.started.elapsed());
        }
    }

    pub fn show(&self, ui: &mut egui::Ui, id: impl egui::AsIdSalt) {
        let time = duration(self.finished.unwrap_or_else(|| self.started.elapsed()));
        let title = if self.finished.is_none() {
            format!("Working · {time}")
        } else if self.failed {
            format!("Stopped after {time}")
        } else {
            format!("Worked for {time}")
        };
        egui::CollapsingHeader::new(RichText::new(title).small().color(appearance::MUTED))
            .id_salt(id)
            .default_open(false)
            .show(ui, |ui| {
                for step in &self.steps {
                    ui.label(RichText::new(step).small().color(if self.failed {
                        appearance::WARNING
                    } else {
                        appearance::MUTED
                    }));
                }
                if self.steps.is_empty() {
                    appearance::muted(
                        ui,
                        if self.finished.is_none() {
                            "Starting…"
                        } else {
                            "No progress events were recorded."
                        },
                    );
                }
            });
        if self.finished.is_none() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }
}

fn duration(value: Duration) -> String {
    let seconds = value.as_secs();
    if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

#[derive(Debug, PartialEq)]
enum Span<'a> {
    Text(&'a str),
    Bold(&'a str),
    Code(&'a str),
    Link(&'a str, &'a str),
}

fn safe_link(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn inline(mut text: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    while !text.is_empty() {
        if let Some(rest) = text.strip_prefix('`')
            && let Some(end) = rest.find('`')
        {
            spans.push(Span::Code(&rest[..end]));
            text = &rest[end + 1..];
            continue;
        }
        if let Some(rest) = text.strip_prefix("**")
            && let Some(end) = rest.find("**")
        {
            spans.push(Span::Bold(&rest[..end]));
            text = &rest[end + 2..];
            continue;
        }
        if let Some(rest) = text.strip_prefix('[')
            && let Some(end) = rest.find("](")
            && let Some(close) = rest[end + 2..].find(')')
        {
            let url = &rest[end + 2..end + 2 + close];
            if safe_link(url) {
                spans.push(Span::Link(&rest[..end], url));
                text = &rest[end + 3 + close..];
                continue;
            }
        }
        // Keep unfinished streaming syntax and unsupported markup visible, without loss.
        let next = text
            .char_indices()
            .skip(1)
            .find(|(_, c)| matches!(c, '`' | '*' | '['))
            .map_or(text.len(), |(index, _)| index);
        spans.push(Span::Text(&text[..next]));
        text = &text[next..];
    }
    spans
}

fn rich_line(ui: &mut egui::Ui, text: &str, heading: bool) {
    ui.scope(|ui| {
        // Text rows should follow font height, rather than the toolbar button height.
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for span in inline(text) {
                match span {
                    Span::Link(label, url) => {
                        ui.hyperlink_to(label, url);
                    }
                    Span::Text(value) | Span::Bold(value) | Span::Code(value) => {
                        let mut rich = RichText::new(value);
                        if heading || matches!(span, Span::Bold(_)) {
                            rich = rich.strong();
                        }
                        if heading {
                            rich = rich.size(18.0);
                        }
                        if matches!(span, Span::Code(_)) {
                            rich = rich
                                .monospace()
                                .background_color(ui.visuals().extreme_bg_color);
                        }
                        ui.add(egui::Label::new(rich).selectable(true));
                    }
                }
            }
        });
    });
}

fn prose(ui: &mut egui::Ui, text: &str) {
    for paragraph in text.split("\n\n") {
        let mut plain = String::new();
        let flush = |ui: &mut egui::Ui, plain: &mut String| {
            if !plain.is_empty() {
                rich_line(ui, plain, false);
                plain.clear();
            }
        };
        for line in paragraph.lines() {
            let line = line.trim();
            let bullet = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "));
            let numbered = line
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            let heading = line
                .strip_prefix("### ")
                .or_else(|| line.strip_prefix("## "))
                .or_else(|| line.strip_prefix("# "));
            if let Some(content) = bullet {
                flush(ui, &mut plain);
                rich_line(ui, &format!("• {content}"), false);
            } else if let Some((number, content)) = numbered {
                flush(ui, &mut plain);
                rich_line(ui, &format!("{number}. {content}"), false);
            } else if let Some(content) = heading {
                flush(ui, &mut plain);
                rich_line(ui, content, true);
            } else {
                if !plain.is_empty() {
                    plain.push(' ');
                }
                plain.push_str(line);
            }
        }
        flush(ui, &mut plain);
        ui.add_space(5.0);
    }
}

pub(super) fn markdown(ui: &mut egui::Ui, text: &str) {
    for (index, (language, block)) in code_blocks(text).into_iter().enumerate() {
        if let Some(language) = language {
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(8)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        appearance::muted(ui, language);
                        if ui.small_button("Copy code").clicked() {
                            ui.ctx().copy_text(block.to_owned());
                        }
                    });
                    egui::ScrollArea::horizontal()
                        .id_salt(ui.scope_id().with(index))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(block).monospace())
                                    .wrap_mode(egui::TextWrapMode::Extend)
                                    .selectable(true),
                            );
                        });
                });
        } else {
            prose(ui, block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_answers_preserve_partial_markup_and_validate_link_schemes() {
        assert_eq!(
            inline("Use **Auto** and `2 + 3` [source](https://example.org)."),
            vec![
                Span::Text("Use "),
                Span::Bold("Auto"),
                Span::Text(" and "),
                Span::Code("2 + 3"),
                Span::Text(" "),
                Span::Link("source", "https://example.org"),
                Span::Text(".")
            ]
        );
        for text in [
            "**partial",
            "`partial",
            "[unfinished",
            "[bad](javascript:alert)",
        ] {
            let visible: String = inline(text)
                .into_iter()
                .map(|span| match span {
                    Span::Text(value) => value,
                    _ => panic!("Unexpected parsed markup"),
                })
                .collect();
            assert_eq!(visible, text);
        }
        assert!(!safe_link("file:///C:/secret"));
        assert!(!safe_link("https://user:password@example.org"));
    }
    #[test]
    fn activity_is_bounded_and_only_contains_received_events() {
        let mut activity = Activity::new();
        assert!(activity.steps.is_empty());
        activity.record("Searching sources");
        activity.record("Searching sources");
        assert_eq!(activity.steps.len(), 1);
        for i in 0..100 {
            activity.record(&format!("Step {i}"));
        }
        assert_eq!(activity.steps.len(), 64);
        activity.fail("Check failed");
        activity.finish();
        let elapsed = activity.finished;
        activity.finish();
        assert_eq!(activity.finished, elapsed);
        assert!(activity.failed);
        assert_eq!(duration(Duration::from_secs(254)), "4m 14s");
    }
}
