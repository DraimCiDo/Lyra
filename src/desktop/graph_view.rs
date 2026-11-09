//! Interactive code graph screen: force layout, pan/zoom, search, GitHub pull-request overlay.
use super::theme::{Colors, Language};
use crate::codegraph::{self, Graph, NodeKind};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use std::{
    collections::BTreeMap,
    io,
    path::PathBuf,
    sync::mpsc::{Receiver, TryRecvError, channel},
};

type Built = io::Result<(Graph, BTreeMap<String, u32>, String)>;

#[derive(Default)]
pub(super) struct GraphView {
    graph: Option<Graph>,
    highlight: BTreeMap<String, u32>,
    title: String,
    rx: Option<Receiver<Built>>,
    status: String,
    query: String,
    #[cfg_attr(not(feature = "gateway-http"), allow(dead_code))]
    pr_number: String,
    show_symbols: bool,
    show_isolated: bool,
    selected: Option<usize>,
    pos: Vec<Vec2>,
    vel: Vec<Vec2>,
    linked: Vec<bool>,
    steps: u32,
    pan: Vec2,
    zoom: f32,
}

fn community_color(community: u32) -> Color32 {
    // HSL(h, 60%, 60%) → RGB
    let h = (community as f32 * 67.0) % 360.0 / 60.0;
    let (s, l) = (0.6f32, 0.6f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0) as u8;
    Color32::from_rgb(to(r), to(g), to(b))
}

impl GraphView {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    fn load(&mut self, graph: Graph, highlight: BTreeMap<String, u32>, title: String) {
        let n = graph.nodes.len();
        let communities = graph.communities.max(1) as f32;
        self.pos = (0..n)
            .map(|i| {
                let c = graph.nodes[i].community as f32 / communities * std::f32::consts::TAU;
                let r = 60.0 + communities.sqrt() * 30.0;
                // deterministic jitter, no RNG dependency
                let j = ((i * 2654435761) % 1000) as f32 / 1000.0 - 0.5;
                Vec2::new(
                    c.cos() * r + j * 60.0,
                    c.sin() * r + (1.0 - j) * 30.0 - 15.0,
                )
            })
            .collect();
        self.vel = vec![Vec2::ZERO; n];
        let mut linked = vec![false; n];
        for e in graph
            .edges
            .iter()
            .filter(|e| e.kind != codegraph::EdgeKind::Defines)
        {
            linked[e.from] = true;
            if graph.nodes[e.to].kind == NodeKind::File {
                linked[e.to] = true;
            }
        }
        self.linked = linked;
        self.graph = Some(graph);
        self.highlight = highlight;
        self.title = title;
        self.selected = None;
        self.steps = 0;
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    pub fn build_local(&mut self, repo: PathBuf) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.status = "…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let (files, truncated) = codegraph::load_dir(&repo.canonicalize()?)?;
                let graph = codegraph::build(&files, truncated);
                Ok((graph, BTreeMap::new(), repo.display().to_string()))
            })();
            let _ = tx.send(result);
        });
    }

    #[cfg(feature = "gateway-http")]
    pub fn build_pr(&mut self, repo: PathBuf, github: String, number: u64, token_env: String) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.status = "…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let client =
                    crate::github::GithubClient::new(crate::github::DEFAULT_API, &token_env)?;
                let changed = client.pull_filenames(&github, number)?;
                let (files, truncated) = codegraph::load_dir(&repo.canonicalize()?)?;
                let graph = codegraph::build(&files, truncated);
                let radius = graph.impact(&changed, 2);
                Ok((graph, radius, format!("{github}#{number}")))
            })();
            let _ = tx.send(result);
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(Ok((graph, highlight, title))) => {
                self.status = format!(
                    "{} / {} / {}",
                    graph.nodes.len(),
                    graph.edges.len(),
                    highlight.len()
                );
                self.load(graph, highlight, title);
            }
            Ok(Err(e)) => self.status = e.to_string(),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => {}
        }
        self.rx = None;
    }

    fn visible(&self, graph: &Graph, i: usize) -> bool {
        match graph.nodes[i].kind {
            NodeKind::File => self.show_isolated || self.linked[i],
            NodeKind::Symbol => self.show_symbols,
        }
    }

    fn step(&mut self) {
        let Some(graph) = &self.graph else { return };
        let ids: Vec<usize> = (0..graph.nodes.len())
            .filter(|&i| self.visible(graph, i))
            .collect();
        let stride = (ids.len() / 400).max(1);
        let mut force = vec![Vec2::ZERO; graph.nodes.len()];
        for (a, &i) in ids.iter().enumerate() {
            let mut b = a + 1;
            while b < ids.len() {
                let j = ids[b];
                let d = self.pos[i] - self.pos[j];
                let dist2 = d.length_sq() + 1.0;
                let f = d * (300.0 * stride as f32 / dist2).min(40.0);
                force[i] += f;
                force[j] -= f;
                b += if b - a > 40 { stride } else { 1 };
            }
        }
        for e in &graph.edges {
            if !self.visible(graph, e.from) || !self.visible(graph, e.to) {
                continue;
            }
            let d = self.pos[e.to] - self.pos[e.from];
            let len = d.length() + 0.1;
            let rest = if e.kind == codegraph::EdgeKind::Defines {
                30.0
            } else {
                60.0
            };
            let f = d / len * ((len - rest) * 0.02);
            force[e.from] += f;
            force[e.to] -= f;
        }
        for &i in &ids {
            let v = (self.vel[i] + force[i] - self.pos[i] * 0.004)
                .clamp(Vec2::splat(-20.0), Vec2::splat(20.0));
            self.vel[i] = v * 0.7;
            self.pos[i] += self.vel[i];
        }
        self.steps += 1;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        c: Colors,
        l: Language,
        repo: Option<&PathBuf>,
        github: &str,
        token_env: &str,
    ) {
        self.poll(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.heading(l.text("Граф кода"));
            if ui
                .add_enabled(
                    repo.is_some() && !self.busy(),
                    egui::Button::new(l.text("Построить граф проекта")),
                )
                .clicked()
                && let Some(repo) = repo
            {
                self.build_local(repo.clone());
            }
            #[cfg(feature = "gateway-http")]
            {
                ui.add(
                    egui::TextEdit::singleline(&mut self.pr_number)
                        .hint_text("PR #")
                        .desired_width(60.0),
                );
                let number = self.pr_number.trim().parse::<u64>().ok();
                if ui
                    .add_enabled(
                        repo.is_some()
                            && number.is_some()
                            && !github.trim().is_empty()
                            && !self.busy(),
                        egui::Button::new(l.text("Влияние PR из GitHub")),
                    )
                    .clicked()
                    && let (Some(repo), Some(number)) = (repo, number)
                {
                    self.build_pr(
                        repo.clone(),
                        github.trim().to_owned(),
                        number,
                        token_env.to_owned(),
                    );
                }
            }
            #[cfg(not(feature = "gateway-http"))]
            let _ = (github, token_env);
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(l.text("Поиск"))
                    .desired_width(140.0),
            );
            ui.checkbox(&mut self.show_symbols, l.text("Символы"));
            ui.checkbox(&mut self.show_isolated, l.text("Без связей"));
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).small().weak());
            }
        });
        if self.graph.is_none() {
            ui.add_space(12.0);
            ui.label(l.text("Выберите папку проекта и постройте граф: файлы, символы, импорты и сообщества. Содержимое файлов не сохраняется."));
            return;
        }
        if self.steps < 500 {
            self.step();
            ui.ctx().request_repaint();
        }
        let size = ui.available_size_before_wrap();
        let (response, painter) = ui.allocate_painter(
            Vec2::new(size.x, (size.y - 30.0).max(200.0)),
            Sense::click_and_drag(),
        );
        painter.rect_filled(response.rect, 8.0, c.card);
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = (1.0 + scroll * 0.002).clamp(0.8, 1.25);
                if let Some(p) = response.hover_pos() {
                    let origin = response.rect.center().to_vec2() + self.pan;
                    self.pan = (self.pan + (p.to_vec2() - origin) * (1.0 - factor))
                        .clamp(Vec2::splat(-1e5), Vec2::splat(1e5));
                }
                self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
            }
        }
        let Some(graph) = &self.graph else { return };
        let origin = response.rect.center() + self.pan;
        let to_screen = |p: Vec2| Pos2::new(origin.x + p.x * self.zoom, origin.y + p.y * self.zoom);
        let query = self.query.to_lowercase();
        let matches = |i: usize| {
            !query.is_empty()
                && (graph.nodes[i].label.to_lowercase().contains(&query)
                    || graph.nodes[i].file.to_lowercase().contains(&query))
        };
        let painter = painter.with_clip_rect(response.rect);
        for e in &graph.edges {
            if !self.visible(graph, e.from) || !self.visible(graph, e.to) {
                continue;
            }
            let color = match e.kind {
                codegraph::EdgeKind::Imports => Color32::from_rgb(108, 123, 208),
                codegraph::EdgeKind::References => c.border,
                codegraph::EdgeKind::Defines => c.border.gamma_multiply(0.5),
            };
            painter.line_segment(
                [to_screen(self.pos[e.from]), to_screen(self.pos[e.to])],
                Stroke::new(0.8, color),
            );
        }
        let mut nearest: Option<(usize, f32)> = None;
        let pointer = response.hover_pos();
        for (i, node) in graph.nodes.iter().enumerate() {
            if !self.visible(graph, i) {
                continue;
            }
            let p = to_screen(self.pos[i]);
            let radius = if node.kind == NodeKind::File {
                3.0 + (node.degree as f32).sqrt().min(8.0)
            } else {
                2.0
            };
            let hl = self.highlight.get(&node.file).copied();
            let mut color = match hl {
                Some(0) => c.error,
                Some(_) => Color32::from_rgb(255, 183, 77),
                None => community_color(node.community),
            };
            if !query.is_empty() && !matches(i) {
                color = color.gamma_multiply(0.25);
            }
            painter.circle_filled(p, radius, color);
            if Some(i) == self.selected || (!query.is_empty() && matches(i)) {
                painter.circle_stroke(p, radius + 2.0, Stroke::new(1.5, c.text));
            }
            if node.kind == NodeKind::File
                && (node.degree > 8 || Some(i) == self.selected || hl.is_some())
            {
                painter.text(
                    p + Vec2::new(radius + 3.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &node.label,
                    egui::FontId::proportional(11.0),
                    c.text,
                );
            }
            if let Some(m) = pointer {
                let d = m.distance(p);
                if d < radius + 6.0 && nearest.is_none_or(|(_, best)| d < best) {
                    nearest = Some((i, d));
                }
            }
        }
        if response.clicked() {
            self.selected = nearest.map(|(i, _)| i);
        }
        let info = self.selected.map(|i| {
            let n = &graph.nodes[i];
            let state = match self.highlight.get(&n.file) {
                Some(0) => format!(" · {}", l.text("изменён в PR")),
                Some(d) => format!(" · {} {d}", l.text("затронут, глубина")),
                None => String::new(),
            };
            format!(
                "{} {} — {}{} · {} {}{state}",
                if n.kind == NodeKind::File {
                    l.text("файл")
                } else {
                    l.text("символ")
                },
                n.label,
                n.file,
                if n.line > 0 {
                    format!(":{}", n.line)
                } else {
                    String::new()
                },
                l.text("связей"),
                n.degree
            )
        });
        ui.label(
            RichText::new(info.unwrap_or_else(|| {
                format!(
                    "{} · {}",
                    self.title,
                    l.text("колесо — масштаб, перетаскивание — сдвиг, клик — узел")
                )
            }))
            .small(),
        );
    }
}
