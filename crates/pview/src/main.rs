//! Atenea — native EDA workbench: viewer, netlists, simulation.
//!
//! Opens one file (`.pdsprj`, `.MDF`/`.SDF`, `.DSN`/`.LYT`); with no
//! argument it shows a native file dialog. GPU-accelerated via
//! `eframe`/`wgpu`; parsing stays on the main thread (files are small).
//!
//! Usage: `atenea [file] [--tab TAB] [--gl] | atenea --help`

use std::collections::HashMap;
use std::path::Path;

use eframe::egui;

const HELP: &str = "Atenea — native Linux EDA workbench (GPL-3.0-or-later)

Usage:
  atenea [file] [--tab TAB] [--gl]
    open .pdsprj | .MDF | .SDF | .DSN | .LYT
  atenea --help   show this help

Options:
  --tab TAB   initial tab (Tables, Graph, Esquema, Simulate)
  --gl        force the OpenGL renderer (VMs, Xvfb, no-Vulkan hosts)

With no file argument a native open dialog is shown.";

fn main() -> eframe::Result<()> {
    // The window always opens, even with no file: the welcome view offers
    // an "Abrir" button and drag&drop. rfd dialogs only run from inside
    // the UI, where a missing backend shows an error instead of killing
    // the app before any window appears.
    let mut initial: Option<String> = None;
    let mut initial_tab: Option<String> = None;
    let mut use_gl = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" | "help" => {
                println!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("atenea {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--tab" => {
                initial_tab = args.next();
            }
            "--gl" => {
                use_gl = true;
            }
            p if p.starts_with('-') => {
                eprintln!("atenea: unknown flag {p}\n{HELP}");
                std::process::exit(2);
            }
            p => {
                initial = Some(p.to_owned());
            }
        }
    }
    let mut app = App::new_empty();
    if let Some(path) = initial {
        app.open_path(Path::new(&path));
    }
    if let Some(tab) = initial_tab.as_deref() {
        app.tab = match tab {
            "Graph" => Tab::Graph,
            "Esquema" => Tab::Wires,
            "Simulate" => Tab::Simulate,
            _ => Tab::Tables,
        };
    }
    let mut native = eframe::NativeOptions::default();
    if use_gl {
        native.renderer = eframe::Renderer::Glow;
    }
    eframe::run_native("Atenea", native, Box::new(|_| Ok(Box::new(app))))
}

/// One loaded artifact.
struct Loaded {
    /// File name for display.
    name: String,
    /// Summary lines.
    summary: Vec<(String, String)>,
    /// `*PARTLIST` rows.
    parts: Vec<(String, String, String)>,
    /// `*NETLIST` nets + pin counts.
    nets: Vec<(String, usize)>,
    /// Full net pin lists for the graph view.
    nets_full: Vec<(String, Vec<(String, String)>)>,
    /// `.DSN` `{...}` heads.
    blocks: Vec<(String, String)>,
    /// Real wire geometry in mm: `(x1, y1, x2, y2)` from `plib::dsn`.
    wires: Vec<(f64, f64, f64, f64)>,
    /// Placed labels in mm: `(text, x, y)`.
    placements: Vec<(String, f64, f64)>,
    /// Source text for `.MDF`/`.SDF` (enables the Simulate tab).
    sdf_text: Option<String>,
    /// Extra `*PARTLIST` fields per ref (EID, PACKAGE, PINSWAP, ...).
    part_rest: HashMap<String, Vec<String>>,
}

/// Convert raw DSN wires (internal units) to mm segments.
#[must_use]
fn wires_to_mm(raw: &[plib::dsn::Wire]) -> Vec<(f64, f64, f64, f64)> {
    raw.iter()
        .map(|w| {
            (
                plib::dsn::to_mm(w.x1),
                plib::dsn::to_mm(w.y1),
                plib::dsn::to_mm(w.x2),
                plib::dsn::to_mm(w.y2),
            )
        })
        .collect()
}

/// Placed text labels in mm: `(text, x, y)`.
#[must_use]
fn placements_mm(raw: &[plib::dsn::Placed]) -> Vec<(String, f64, f64)> {
    raw.iter()
        .map(|p| (p.text.clone(), plib::dsn::to_mm(p.x), plib::dsn::to_mm(p.y)))
        .collect()
}

/// Bounding box of mm wires as `(min_x, min_y, max_x, max_y)`.
#[must_use]
fn wires_bbox(wires: &[(f64, f64, f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let mut iter = wires.iter();
    let first = iter.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (
        first.0.min(first.2),
        first.1.min(first.3),
        first.0.max(first.2),
        first.1.max(first.3),
    );
    for &(ax, ay, bx, by) in iter {
        x0 = x0.min(ax.min(bx));
        y0 = y0.min(ay.min(by));
        x1 = x1.max(ax.max(bx));
        y1 = y1.max(ay.max(by));
    }
    Some((x0, y0, x1, y1))
}

/// DSN mm coordinates to egui world units (lossy `f64` -> `f32`).
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[must_use]
fn mm_to_world(x_mm: f64, y_mm: f64) -> egui::Pos2 {
    egui::pos2(x_mm as f32, y_mm as f32)
}

/// Map a world point to screen: `origin + (world - pan) * zoom`.
#[must_use]
fn world_to_screen(
    world: egui::Pos2,
    pan: egui::Vec2,
    zoom: f32,
    origin: egui::Pos2,
) -> egui::Pos2 {
    origin + ((world.to_vec2() - pan) * zoom)
}

/// Inverse of [`world_to_screen`].
#[must_use]
fn screen_to_world(
    screen: egui::Pos2,
    pan: egui::Vec2,
    zoom: f32,
    origin: egui::Pos2,
) -> egui::Pos2 {
    let v = (screen - origin) / zoom + pan;
    egui::pos2(v.x, v.y)
}

/// Distinct hues cycled by plot series index.
const TRACE_HUES: [(u8, u8, u8); 6] = [
    (90, 200, 250),
    (250, 180, 90),
    (140, 230, 140),
    (230, 140, 200),
    (200, 200, 120),
    (170, 160, 250),
];

/// Fit `bbox` (mm) into `size` px with a 5% margin; returns `(pan, zoom)`.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#[must_use]
fn fit_view(bbox: (f64, f64, f64, f64), size: egui::Vec2) -> (egui::Vec2, f32) {
    let (x0, y0, x1, y1) = bbox;
    let bw = (x1 - x0).max(1e-9);
    let bh = (y1 - y0).max(1e-9);
    let zoom_f = (f64::from(size.x) / bw).min(f64::from(size.y) / bh) * 0.95;
    let zoom_f = zoom_f.clamp(0.01, 1000.0);
    let cx = f64::midpoint(x0, x1);
    let cy = f64::midpoint(y0, y1);
    let pan = egui::vec2(
        (cx - f64::from(size.x) / (2.0 * zoom_f)) as f32,
        (cy - f64::from(size.y) / (2.0 * zoom_f)) as f32,
    );
    (pan, zoom_f as f32)
}

impl Loaded {
    fn open(path: &Path) -> Result<Self, String> {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_owned();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !path.is_file() {
            return Err(format!("no such file: {}", path.display()));
        }
        match ext.as_str() {
            "pdsprj" => Self::open_pdsprj(path, &name),
            "mdf" | "sdf" => Self::open_lisa(path, &name),
            "dsn" | "lyt" => Self::open_dsn(path, &name),
            _ => Err(format!("unsupported extension: .{ext}")),
        }
    }

    /// Empty placeholder shown in the welcome view.
    fn empty() -> Self {
        Self {
            name: "Atenea".to_owned(),
            summary: Vec::new(),
            parts: Vec::new(),
            nets: Vec::new(),
            nets_full: Vec::new(),
            blocks: Vec::new(),
            wires: Vec::new(),
            placements: Vec::new(),
            sdf_text: None,
            part_rest: HashMap::new(),
        }
    }

    /// `true` when no file is loaded yet.
    fn is_empty(&self) -> bool {
        self.parts.is_empty()
            && self.nets.is_empty()
            && self.blocks.is_empty()
            && self.wires.is_empty()
            && self.sdf_text.is_none()
    }

    fn open_pdsprj(path: &Path, name: &str) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let proj = pproj::open(file).map_err(|e| e.to_string())?;
        let dsn_text = proj.dsn.as_deref().unwrap_or_default();
        // Embedded DSN is binary; the LISA text lives in extracted samples,
        // so a schematic pdsprj shows project metadata here. Netlist-grade
        // text comes from `.MDF`/`.SDF` files.
        let doc = pmodel::parse(
            &String::from_utf8_lossy(dsn_text)
                .into_owned()
                .replace('\0', ""),
        )
        .unwrap_or_else(|_| pmodel::Document {
            format: "Proteus project".to_owned(),
            version: String::new(),
            preamble: HashMap::new(),
            sections: Vec::new(),
        });
        let mut loaded = Self::from_lisa(name, &doc);
        loaded.summary.insert(
            0,
            (
                "project".to_owned(),
                proj.title
                    .clone()
                    .unwrap_or_else(|| "(workspace)".to_owned()),
            ),
        );
        loaded
            .summary
            .insert(1, ("dsn_bytes".to_owned(), dsn_text.len().to_string()));
        if let Some(ref dsn) = proj.dsn {
            loaded.wires = wires_to_mm(&plib::dsn::wires(dsn));
            loaded.placements = placements_mm(&plib::dsn::placements(dsn));
            loaded
                .summary
                .push(("wires".to_owned(), loaded.wires.len().to_string()));
            loaded
                .summary
                .push(("labels".to_owned(), loaded.placements.len().to_string()));
            if let Some((x0, y0, x1, y1)) = wires_bbox(&loaded.wires) {
                loaded.summary.push((
                    "bbox_mm".to_owned(),
                    format!("{:.2}x{:.2}", x1 - x0, y1 - y0),
                ));
            }
        }
        Ok(loaded)
    }

    fn open_lisa(path: &Path, name: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let doc = pmodel::parse(&text).map_err(|e| e.to_string())?;
        let mut loaded = Self::from_lisa(name, &doc);
        loaded.sdf_text = Some(text);
        Ok(loaded)
    }

    fn from_lisa(name: &str, doc: &pmodel::Document) -> Self {
        let plist = doc.partlist().unwrap_or_default();
        let part_rest = plist
            .iter()
            .map(|p| (p.reference.clone(), p.rest.clone()))
            .collect();
        let parts = plist
            .into_iter()
            .map(|p| (p.reference, p.kind, p.value))
            .collect();
        let full = doc.netlist().unwrap_or_default();
        let nets = full.iter().map(|n| (n.id.clone(), n.pins.len())).collect();
        let nets_full = full
            .into_iter()
            .map(|n| (n.id, n.pins.into_iter().map(|(r, p, _)| (r, p)).collect()))
            .collect();
        let summary = vec![
            ("format".to_owned(), doc.format.clone()),
            ("version".to_owned(), doc.version.clone()),
        ];
        Self {
            name: name.to_owned(),
            summary,
            parts,
            nets,
            nets_full,
            blocks: Vec::new(),
            wires: Vec::new(),
            placements: Vec::new(),
            sdf_text: None,
            part_rest,
        }
    }

    fn open_dsn(path: &Path, name: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        plib::dsn::check_magic(&data).map_err(|e| e.to_string())?;
        let blocks = plib::dsn::blocks(&data)
            .into_iter()
            .map(|b| {
                let body: String = b.body.chars().take(80).collect();
                (b.head, body)
            })
            .collect();
        let wires = wires_to_mm(&plib::dsn::wires(&data));
        let placements = placements_mm(&plib::dsn::placements(&data));
        let mut summary = vec![("size".to_owned(), data.len().to_string())];
        summary.push(("wires".to_owned(), wires.len().to_string()));
        if let Some((x0, y0, x1, y1)) = wires_bbox(&wires) {
            summary.push((
                "bbox_mm".to_owned(),
                format!("{:.2}x{:.2}", x1 - x0, y1 - y0),
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            summary,
            parts: Vec::new(),
            nets: Vec::new(),
            nets_full: Vec::new(),
            blocks,
            wires,
            placements,
            sdf_text: None,
            part_rest: HashMap::new(),
        })
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
enum Tab {
    Tables,
    Graph,
    Wires,
    Simulate,
}

/// Precomputed canvas model: grid layout, net edges, search matches.
struct GraphData {
    names: Vec<(String, String)>,
    cols: u16,
    index: HashMap<String, usize>,
    edges: Vec<(usize, usize, u8)>,
    part_nets: HashMap<usize, Vec<String>>,
    matching: Vec<bool>,
}

/// Wires-view layer toggles (Proteus-sheet look).
#[derive(Clone, Copy)]
struct Layers {
    wires: bool,
    labels: bool,
    grid: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            wires: true,
            labels: true,
            grid: true,
        }
    }
}

/// Visibility flags for chrome panels (keeps `App` bool count low).
#[derive(Debug, Clone)]
struct Panels {
    explorer: bool,
    inspector: bool,
    help: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            explorer: true,
            inspector: true,
            help: false,
        }
    }
}

struct App {
    model: Loaded,
    /// Last file-open error (shown instead of panicking).
    file_error: Option<String>,
    tab: Tab,
    search: String,
    selected: Option<String>,
    zoom: f32,
    pan: egui::Vec2,
    /// Wires-view zoom (px per mm), independent from the Graph grid zoom.
    wires_zoom: f32,
    /// Wires-view pan in mm.
    wires_pan: egui::Vec2,
    /// Auto-fit done once per file (reset via the "fit" button).
    wires_fitted: bool,
    /// Sheet layer toggles (wires/labels/grid).
    layers: Layers,
    /// Cursor position in sheet mm for the status bar.
    wires_cursor: Option<(f64, f64)>,
    /// Last simulation trace (Simulate tab).
    sim_trace: Option<asim::Trace>,
    /// Last simulation error, if any.
    sim_error: Option<String>,
    /// Variables to print, space separated (e.g. `v(OUT)`).
    sim_vars: String,
    /// Transient stop time (e.g. `5m`).
    sim_tstop: String,
    /// Visibility flags for explorer / inspector / help.
    panels: Panels,
}

impl App {
    fn new_empty() -> Self {
        Self {
            model: Loaded::empty(),
            file_error: None,
            tab: Tab::Tables,
            search: String::new(),
            selected: None,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            wires_zoom: 1.0,
            wires_pan: egui::Vec2::ZERO,
            wires_fitted: false,
            layers: Layers::default(),
            wires_cursor: None,
            sim_trace: None,
            sim_error: None,
            sim_vars: "v(OUT)".to_owned(),
            sim_tstop: "5m".to_owned(),
            panels: Panels::default(),
        }
    }

    /// Reset per-file view state after opening a new file.
    fn reset_view(&mut self) {
        self.tab = Tab::Tables;
        self.search.clear();
        self.selected = None;
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        self.wires_zoom = 1.0;
        self.wires_pan = egui::Vec2::ZERO;
        self.wires_fitted = false;
        self.sim_trace = None;
        self.sim_error = None;
    }

    /// Open a file, replacing the model; errors are stored, not panics.
    fn open_path(&mut self, path: &std::path::Path) {
        match Loaded::open(path) {
            Ok(model) => {
                self.model = model;
                self.file_error = None;
                self.reset_view();
            }
            Err(e) => {
                self.file_error = Some(format!("{}: {e}", path.display()));
            }
        }
    }

    /// Native open dialog; a missing backend reports instead of hanging.
    fn open_dialog(&mut self) {
        match rfd::FileDialog::new()
            .add_filter("Proteus", &["pdsprj", "MDF", "SDF", "DSN", "LYT"])
            .add_filter("all", &["*"])
            .pick_file()
        {
            Some(path) => self.open_path(&path),
            None => {
                self.file_error = Some(
                    "diálogo cancelado o no disponible: arrastrá un archivo a la ventana"
                        .to_owned(),
                );
            }
        }
    }

    /// First-run screen: open button + supported formats.
    fn show_welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("Atenea — workbench nativa de electrónica");
            ui.label("Abrí un proyecto o esquema para empezar.");
            ui.add_space(12.0);
            if ui.button("Abrir archivo…").clicked() {
                self.open_dialog();
            }
            ui.add_space(8.0);
            ui.label("o arrastrá un archivo a esta ventana");
            ui.monospace(".pdsprj  .MDF  .SDF  .DSN  .LYT");
            ui.add_space(12.0);
        });
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.collapsing("Primeros pasos", |ui| {
                    ui.label("1. Abrí un .pdsprj o un .DSN.");
                    ui.label("2. Mirá la pestaña Esquema (wires + etiquetas reales).");
                    ui.label("3. Probá Simulate con un .SDF + ngspice.");
                });
            });
            ui.vertical(|ui| {
                ui.collapsing("Ayuda", |ui| {
                    ui.label("Docs: 01-empezar, 02-formatos, 03-simular-y-plugins.");
                    ui.label("Log de arranque: /tmp/atenea.log");
                });
                ui.collapsing("Acerca de", |ui| {
                    ui.label("Atenea GPL-3.0-or-later. RE interoperable.");
                });
            });
        });
    }

    /// Grid position of a cell in world units.
    fn grid_pos(col: u16, row: u16) -> egui::Pos2 {
        egui::pos2(f32::from(col) * 170.0, f32::from(row) * 54.0)
    }

    /// Layout model for the canvas: positions, edges, search matches.
    fn build_graph(&self) -> Option<GraphData> {
        if self.model.parts.is_empty() {
            return None;
        }
        let query = self.search.to_lowercase();
        let names: Vec<(String, String)> = self
            .model
            .parts
            .iter()
            .map(|(r, k, _)| (r.clone(), k.clone()))
            .collect();
        let cols = u16::try_from(names.len().isqrt())
            .unwrap_or(u16::MAX)
            .saturating_add(1);
        let index: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .map(|(i, (r, _))| (r.clone(), i))
            .collect();
        let mut edges: Vec<(usize, usize, u8)> = Vec::new();
        let mut part_nets: HashMap<usize, Vec<String>> = HashMap::new();
        for (id, pins) in &self.model.nets_full {
            let mut idx: Vec<usize> = pins
                .iter()
                .filter_map(|(r, _)| index.get(r).copied())
                .collect();
            idx.sort_unstable();
            idx.dedup();
            for &pi in &idx {
                part_nets.entry(pi).or_default().push(id.clone());
            }
            if idx.len() >= 2 && idx.len() <= 10 {
                let color = u8::try_from(id.len() % 120).unwrap_or(0).wrapping_mul(47);
                for w in idx.windows(2) {
                    edges.push((w[0], w[1], color));
                }
            }
        }
        let matching: Vec<bool> = names
            .iter()
            .map(|(r, k)| {
                query.is_empty()
                    || r.to_lowercase().contains(&query)
                    || k.to_lowercase().contains(&query)
            })
            .collect();
        Some(GraphData {
            names,
            cols,
            index,
            edges,
            part_nets,
            matching,
        })
    }

    fn draw_graph(&mut self, ui: &mut egui::Ui) {
        let Some(data) = self.build_graph() else {
            ui.label("No netlist in this file — see Tables.");
            return;
        };
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.interact(ui, rect, &response, &data);
        Self::paint(
            ui,
            rect,
            &data,
            self.zoom,
            self.pan,
            self.selected.as_deref(),
        );
        // Inspector footer.
        ui.separator();
        if let Some(sel) = &self.selected {
            let nets = data
                .index
                .get(sel)
                .and_then(|i| data.part_nets.get(i))
                .cloned()
                .unwrap_or_default();
            let clear = ui
                .horizontal(|ui| {
                    ui.strong(sel);
                    ui.label(format!("nets: {}", nets.join(", ")));
                    ui.button("clear").clicked()
                })
                .inner;
            if clear {
                self.selected = None;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} parts, {} nets — drag to pan, wheel to zoom, click a part",
                    data.names.len(),
                    self.model.nets.len()
                ));
                if ui.button("fit").clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                    self.selected = None;
                }
            });
        }
    }

    /// Pan (drag), zoom (wheel) and click-to-select.
    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        data: &GraphData,
    ) {
        let zoom_delta = ui.input(egui::InputState::zoom_delta);
        let mut zoom = self.zoom;
        let mut pan = self.pan;
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            if let Some(ptr) = response.hover_pos() {
                let to_world =
                    |p: egui::Pos2, zoom: f32, pan: egui::Vec2| (p - rect.min) / zoom + pan;
                let before = to_world(ptr, zoom, pan);
                zoom = (zoom * zoom_delta).clamp(0.2, 4.0);
                let after = to_world(ptr, zoom, pan);
                pan += before - after;
            }
        }
        if response.dragged() {
            pan -= response.drag_delta() / zoom;
        }
        if response.clicked() {
            if let Some(ptr) = response.interact_pointer_pos() {
                // Same transform as zoom (`to_world` above) and `paint`:
                // screen = rect.min + (world - pan) * zoom.
                let world = rect.min + ((ptr - rect.min) / zoom + pan);
                let mut best: Option<(usize, f32)> = None;
                for (i, _) in data.names.iter().enumerate() {
                    let iu = u16::try_from(i).unwrap_or(u16::MAX);
                    let d = (Self::grid_pos(iu % data.cols, iu / data.cols) - world).length();
                    if d < 60.0 && best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((i, d));
                    }
                }
                self.selected = best.map(|(i, _)| data.names[i].0.clone());
            }
        }
        self.zoom = zoom;
        self.pan = pan;
    }

    /// Paint edges under labeled part boxes; skips off-screen boxes.
    fn paint(
        ui: &mut egui::Ui,
        rect: egui::Rect,
        data: &GraphData,
        zoom: f32,
        pan: egui::Vec2,
        selected: Option<&str>,
    ) {
        let to_screen = |world: egui::Pos2| rect.min + ((world.to_vec2() - pan) * zoom);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        let selected_idx = selected.and_then(|s| data.index.get(s).copied());
        for (a, b, color) in &data.edges {
            let dim = selected_idx.is_some_and(|s| *a != s && *b != s);
            let c = egui::Color32::from_rgb(90 + color % 120, 140, 200);
            let ia = u16::try_from(*a).unwrap_or(u16::MAX);
            let ib = u16::try_from(*b).unwrap_or(u16::MAX);
            painter.line_segment(
                [
                    to_screen(Self::grid_pos(ia % data.cols, ia / data.cols)),
                    to_screen(Self::grid_pos(ib % data.cols, ib / data.cols)),
                ],
                egui::Stroke::new(
                    if dim { 0.5 } else { 1.5 },
                    c.linear_multiply(if dim { 0.25 } else { 0.9 }),
                ),
            );
        }
        for (i, (name, kind)) in data.names.iter().enumerate() {
            let iu = u16::try_from(i).unwrap_or(u16::MAX);
            let center = to_screen(Self::grid_pos(iu % data.cols, iu / data.cols));
            let size = egui::vec2(150.0, 40.0) * zoom;
            let r = egui::Rect::from_center_size(center, size);
            if !rect.intersects(r) {
                continue;
            }
            let is_sel = selected_idx == Some(i);
            let fill = if is_sel {
                egui::Color32::DARK_BLUE
            } else {
                ui.visuals().window_fill()
            };
            painter.rect(
                r,
                4.0,
                fill,
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
                egui::StrokeKind::Inside,
            );
            if zoom > 0.45 {
                let alpha = if data.matching[i] { 1.0 } else { 0.25 };
                painter.text(
                    r.center() + egui::vec2(0.0, -8.0),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::monospace(13.0 * zoom.clamp(0.6, 1.5)),
                    ui.visuals().text_color().linear_multiply(alpha),
                );
                painter.text(
                    r.center() + egui::vec2(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    kind,
                    egui::FontId::monospace(10.0 * zoom.clamp(0.6, 1.5)),
                    ui.visuals().weak_text_color().linear_multiply(alpha),
                );
            }
        }
    }
}

impl App {
    /// Wires tab: real DSN geometry fitted to screen; ignores `search`.
    fn draw_wires(&mut self, ui: &mut egui::Ui) {
        if self.model.wires.is_empty() {
            ui.label("No wire geometry in this file — see Tables.");
            return;
        }
        let bbox = wires_bbox(&self.model.wires).unwrap_or((0.0, 0.0, 1.0, 1.0));
        // Left tool strip (ISIS-like) + canvas.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("✏");
                if ui.button("Fit").on_hover_text("Encuadrar hoja").clicked() {
                    self.wires_fitted = false;
                }
                ui.checkbox(&mut self.layers.wires, "wires");
                ui.checkbox(&mut self.layers.labels, "etiquetas");
                ui.checkbox(&mut self.layers.grid, "reticula");
            });
            ui.separator();
            ui.vertical(|ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                // Don't poison the auto-fit when the first layout has no size yet.
                if !self.wires_fitted && rect.size().x > 1.0 && rect.size().y > 1.0 {
                    let (pan, zoom) = fit_view(bbox, rect.size());
                    self.wires_pan = pan;
                    self.wires_zoom = zoom;
                    self.wires_fitted = true;
                }
                self.interact_wires(ui, rect, &response);
                self.paint_wires(ui, rect);
            });
        });
        ui.separator();
        // Status bar with cursor position (mm + thou, like ISIS).
        ui.horizontal(|ui| {
            let n = self.model.wires.len();
            let (x0, y0, x1, y1) = bbox;
            ui.label(format!("{n} segmentos  {:.1}x{:.1} mm", x1 - x0, y1 - y0));
            ui.separator();
            if let Some((x, y)) = self.wires_cursor {
                ui.monospace(format!(
                    "X:{x:.2}  Y:{y:.2} mm   ({:.0} , {:.0} thou)",
                    x * 39.3701,
                    y * 39.3701
                ));
            } else {
                ui.label("X:— Y:—");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label("arrastrar: pan, rueda: zoom");
            });
        });
    }

    /// Pan (drag) + zoom (wheel) for the Wires view; no click-select.
    fn interact_wires(&mut self, ui: &mut egui::Ui, rect: egui::Rect, response: &egui::Response) {
        let zoom_delta = ui.input(egui::InputState::zoom_delta);
        let mut zoom = self.wires_zoom;
        let mut pan = self.wires_pan;
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            if let Some(ptr) = response.hover_pos() {
                let before = screen_to_world(ptr, pan, zoom, rect.min);
                zoom = (zoom * zoom_delta).clamp(0.05, 100.0);
                let after = screen_to_world(ptr, pan, zoom, rect.min);
                pan += before - after;
            }
        }
        if response.dragged() {
            pan -= response.drag_delta() / zoom;
        }
        self.wires_zoom = zoom;
        self.wires_pan = pan;
        // Cursor position in sheet mm for the status bar.
        self.wires_cursor = response.hover_pos().map(|ptr| {
            let w = screen_to_world(ptr, pan, zoom, rect.min);
            (f64::from(w.x), f64::from(w.y))
        });
    }

    /// Paint an ISIS-like sheet: light paper, minor/major grid,
    /// blue sheet border, dark wires, black refdes labels.: deep blue paper, 0.1\" dot grid,
    /// light wires, white refdes/value labels.
    /// Pixel math is inherently lossy (`usize`/`f64` counts to screen
    /// `f32`); counts here are bounded by the viewport.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn paint_wires(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let painter = ui.painter_at(rect);
        // Light ISIS paper.
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(201, 201, 201));
        let (pan, zoom) = (self.wires_pan, self.wires_zoom);
        // Grid: minor dots every 0.1", major lines every 1".
        if self.layers.grid {
            let mut step = 2.54;
            let approx = (f64::from(rect.width()) / (f64::from(zoom) * step)
                * (f64::from(rect.height()) / (f64::from(zoom) * step)))
                as usize;
            if approx > 20000 {
                step *= ((approx / 20000) as f64).sqrt().ceil();
            }
            let tl = screen_to_world(rect.min, pan, zoom, rect.min);
            let br = screen_to_world(rect.max, pan, zoom, rect.min);
            let minor = egui::Color32::from_rgb(150, 150, 150);
            let major = egui::Color32::from_rgb(60, 60, 60);
            let mut gx = (tl.x / step as f32).floor() * step as f32;
            while gx <= br.x {
                let mut gy = (tl.y / step as f32).floor() * step as f32;
                while gy <= br.y {
                    let wx = f64::from(gx);
                    let wy = f64::from(gy);
                    let is_major =
                        (wx / 25.4).fract().abs() < 1e-6 && (wy / 25.4).fract().abs() < 1e-6;
                    let p = world_to_screen(egui::pos2(gx, gy), pan, zoom, rect.min);
                    let r = if is_major { 2.0 } else { 1.0 };
                    painter.rect_filled(
                        egui::Rect::from_center_size(p, egui::vec2(r, r)),
                        0.0,
                        if is_major { major } else { minor },
                    );
                    gy += step as f32;
                }
                gx += step as f32;
            }
        }
        // Sheet border around the fitted bounding box.
        if let Some((x0, y0, x1, y1)) = wires_bbox(&self.model.wires) {
            let a = world_to_screen(mm_to_world(x0, y0), pan, zoom, rect.min);
            let b = world_to_screen(mm_to_world(x1, y1), pan, zoom, rect.min);
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(a.x.min(b.x), a.y.min(b.y)),
                    egui::pos2(a.x.max(b.x), a.y.max(b.y)),
                ),
                0.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 0, 200)),
                egui::StrokeKind::Inside,
            );
        }
        if !self.layers.wires {
            return;
        }
        let color = egui::Color32::from_rgb(20, 20, 120);
        for (x1, y1, x2, y2) in &self.model.wires {
            painter.line_segment(
                [
                    world_to_screen(mm_to_world(*x1, *y1), pan, zoom, rect.min),
                    world_to_screen(mm_to_world(*x2, *y2), pan, zoom, rect.min),
                ],
                egui::Stroke::new(1.5, color),
            );
        }
        // Placed refdes/value/terminal labels.
        if self.layers.labels && zoom > 0.6 {
            for (text, x, y) in &self.model.placements {
                let p = world_to_screen(mm_to_world(*x, *y), pan, zoom, rect.min);
                if rect.contains(p) {
                    painter.text(
                        p,
                        egui::Align2::CENTER_BOTTOM,
                        text,
                        egui::FontId::monospace(12.0),
                        egui::Color32::BLACK,
                    );
                }
            }
        }
    }

    /// Interactive batch simulation of the loaded `.MDF`/`.SDF`.
    ///
    /// Runs ngspice synchronously (sub-second for small circuits) and
    /// plots the first requested variable; unsupported parts surface as
    /// a readable error instead of a crash.
    fn draw_sim(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("vars:");
            ui.text_edit_singleline(&mut self.sim_vars);
            ui.label("tstop:");
            ui.text_edit_singleline(&mut self.sim_tstop);
            if ui.button("Run").clicked() {
                self.sim_trace = None;
                self.sim_error = None;
                match self.run_sim() {
                    Ok(t) => self.sim_trace = Some(t),
                    Err(e) => self.sim_error = Some(e),
                }
            }
        });
        ui.separator();
        if let Some(err) = &self.sim_error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
        }
        if !self.model.nets.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("nets:");
                for (id, _) in &self.model.nets {
                    let v = format!("v({id})");
                    if ui.button(&v).clicked() && !self.sim_vars.contains(&v) {
                        if !self.sim_vars.trim().is_empty() {
                            self.sim_vars.push(' ');
                        }
                        self.sim_vars.push_str(&v);
                    }
                }
            });
            ui.separator();
        }
        if let Some(trace) = &self.sim_trace {
            ui.label(format!("{} rows", trace.rows.len()));
            Self::paint_trace(ui, trace);
            if let Some((_, last)) = trace.rows.last() {
                ui.label(format!("final: {last:?}"));
            }
        } else if self.sim_error.is_none() {
            ui.label("Load an .MDF/.SDF file, set vars + tstop, press Run.");
        }
    }

    /// Manual time-series plot (zero extra deps): axes + polyline.
    /// Pixel coordinates are inherently `f32`; precision loss vs `f64`
    /// time/voltage is irrelevant on screen.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn paint_trace(ui: &mut egui::Ui, trace: &asim::Trace) {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 260.0),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        let nvars = trace.rows.first().map_or(0, |(_, v)| v.len());
        let mut t1 = 1e-9f64;
        let mut vmax = 1e-9f64;
        let mut vmin = 0.0f64;
        for (t, v) in &trace.rows {
            t1 = t1.max(*t);
            for &x in v {
                vmax = vmax.max(x);
                vmin = vmin.min(x);
            }
        }
        let span = (vmax - vmin).max(1e-12);
        let to_screen = |t: f64, v: f64| {
            egui::pos2(
                rect.min.x + ((t / t1.max(1e-12)) as f32) * rect.width(),
                rect.max.y - (((v - vmin) / span) as f32).clamp(0.0, 1.0) * rect.height(),
            )
        };
        // Axes.
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        );
        painter.line_segment(
            [rect.left_bottom(), rect.left_top()],
            egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        );
        // One polyline per variable, distinct hues (see TRACE_HUES).
        for col in 0..nvars {
            let (r, g, b) = TRACE_HUES[col % TRACE_HUES.len()];
            let mut prev: Option<egui::Pos2> = None;
            for (t, v) in &trace.rows {
                if let Some(&x) = v.get(col) {
                    let p = to_screen(*t, x);
                    if let Some(q) = prev {
                        painter.line_segment(
                            [q, p],
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(r, g, b)),
                        );
                    }
                    prev = Some(p);
                }
            }
        }
        painter.text(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("t [0, {t1:.4}]  v [{vmin:.3}, {vmax:.3}]  ({nvars} series)"),
            egui::FontId::monospace(11.0),
            ui.visuals().text_color(),
        );
    }

    /// Parse the loaded text and run one transient, returning its trace.
    fn run_sim(&self) -> Result<asim::Trace, String> {
        let text = self
            .model
            .sdf_text
            .as_deref()
            .ok_or_else(|| "no SDF/MDF loaded — open a .sdf or .mdf file".to_owned())?;
        let doc = pmodel::parse(text).map_err(|e| format!("parse: {e}"))?;
        let vars: Vec<&str> = self.sim_vars.split_whitespace().collect();
        if vars.is_empty() {
            return Err("no vars (e.g. v(OUT))".to_owned());
        }
        if self.sim_tstop.trim().is_empty() {
            return Err("empty tstop (e.g. 5m)".to_owned());
        }
        asim::simulate_tran(&doc, &self.model.name, "10u", &self.sim_tstop, &vars)
            .map_err(|e| format!("sim: {e}"))
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Drag&drop anywhere: open the first dropped file.
        let dropped: Vec<std::path::PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(path) = dropped.into_iter().next() {
            self.open_path(&path);
        }
        self.handle_shortcuts(ui);
        self.draw_menu(ui);
        ui.separator();
        if let Some(err) = self.file_error.clone() {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
            ui.separator();
        }
        if self.model.is_empty() {
            self.show_welcome(ui);
            return;
        }
        self.draw_panels(ui);
        self.show_tab(ui);
        self.draw_help(ui);
        self.draw_status(ui);
    }
}

impl App {
    /// Global shortcuts (Ctrl+O, Ctrl+1..4, Ctrl+L, Esc, F1).
    fn handle_shortcuts(&mut self, ui: &mut egui::Ui) {
        // Shortcuts: Ctrl+O open, Ctrl+1..4 tabs, Ctrl+L fit, Esc clear, F1 help.
        ui.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::O) {
                self.open_dialog();
            }
            for (n, tab) in [
                (egui::Key::Num1, Tab::Tables),
                (egui::Key::Num2, Tab::Graph),
                (egui::Key::Num3, Tab::Wires),
                (egui::Key::Num4, Tab::Simulate),
            ] {
                if i.modifiers.ctrl && i.key_pressed(n) {
                    self.tab = tab;
                }
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::L) {
                self.wires_fitted = false;
            }
            if i.key_pressed(egui::Key::Escape) {
                self.selected = None;
                self.panels.help = false;
            }
            if i.key_pressed(egui::Key::F1) {
                self.panels.help = !self.panels.help;
            }
        });
    }

    /// Top menu bar + header (Archivo / Ver / Ayuda + tabs + filter).
    fn draw_menu(&mut self, ui: &mut egui::Ui) {
        // Menu bar (ISIS-like): Archivo / Ver / Ayuda.
        ui.horizontal(|ui| {
            ui.menu_button("Archivo", |ui| {
                if ui.button("Abrir…  (Ctrl+O)").clicked() {
                    self.open_dialog();
                }
                if ui
                    .add_enabled(!self.model.is_empty(), egui::Button::new("Cerrar"))
                    .clicked()
                {
                    self.model = Loaded::empty();
                    self.reset_view();
                }
                ui.separator();
                if ui.button("Salir").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Ver", |ui| {
                ui.selectable_value(&mut self.tab, Tab::Tables, "Tables  (Ctrl+1)");
                ui.selectable_value(&mut self.tab, Tab::Graph, "Graph  (Ctrl+2)");
                ui.selectable_value(&mut self.tab, Tab::Wires, "Esquema  (Ctrl+3)");
                ui.selectable_value(&mut self.tab, Tab::Simulate, "Simulate  (Ctrl+4)");
                ui.separator();
                if ui.button("Fit esquema  (Ctrl+L)").clicked() {
                    self.wires_fitted = false;
                }
                ui.checkbox(&mut self.panels.explorer, "Explorador");
                ui.checkbox(&mut self.panels.inspector, "Inspector");
                ui.checkbox(&mut self.layers.wires, "Capa wires");
                ui.checkbox(&mut self.layers.labels, "Capa etiquetas");
                ui.checkbox(&mut self.layers.grid, "Capa retícula");
            });
            ui.menu_button("Ayuda", |ui| {
                if ui.button("Primeros pasos  (F1)").clicked() {
                    self.panels.help = true;
                }
            });
            ui.separator();
            ui.heading(self.model.name.clone());
            if ui.button("Abrir…").clicked() {
                self.open_dialog();
            }
            if ui.button("Fit").clicked() {
                self.wires_fitted = false;
            }
            ui.separator();
            ui.label("filter:");
            ui.text_edit_singleline(&mut self.search);
        });
        ui.separator();
    }

    /// Side explorer / inspector panels.
    fn draw_panels(&mut self, ui: &mut egui::Ui) {
        if self.panels.explorer && !self.model.is_empty() {
            egui::Panel::left("explorer")
                .resizable(true)
                .default_size(190.0)
                .show(ui, |ui| {
                    self.show_explorer(ui);
                });
        }
        if self.panels.inspector && !self.model.is_empty() {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| {
                    self.show_inspector(ui);
                });
        }
    }

    /// Active tab content.
    fn show_tab(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Tables => self.show_tables(ui),
            Tab::Graph => self.draw_graph(ui),
            Tab::Wires => self.draw_wires(ui),
            Tab::Simulate => self.draw_sim(ui),
        }
    }

    /// Floating help window (F1).
    fn draw_help(&mut self, ui: &mut egui::Ui) {
        let mut help = self.panels.help;
        if help {
            egui::Window::new("Ayuda — Atenea")
                .open(&mut help)
                .show(ui, |ui| {
                    Self::show_help(ui);
                });
        }
    }

    /// Bottom status bar.
    fn draw_status(&self, ui: &mut egui::Ui) {
        // Global status bar (ISIS-like): file, position, mode.
        ui.separator();
        ui.horizontal(|ui| {
            ui.monospace(&self.model.name);
            ui.separator();
            if let Some((x, y)) = self.wires_cursor {
                ui.monospace(format!(
                    "X:{x:.1} Y:{y:.1} mm  ({:.0} : {:.0} thou)",
                    x * 39.3701,
                    y * 39.3701
                ));
            } else {
                ui.monospace("X:— Y:—");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!(
                    "zoom {:.0}% | {:?}",
                    self.wires_zoom * 100.0,
                    self.tab
                ));
            });
        });
    }

    /// Left design-explorer tree: parts, nets, blocks with counts.
    fn show_explorer(&mut self, ui: &mut egui::Ui) {
        ui.heading("Explorador");
        egui::ScrollArea::both().show(ui, |ui| {
            egui::CollapsingHeader::new(format!("Partes ({})", self.model.parts.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for (r, k, v) in self.model.parts.clone() {
                        let label = format!("{r} {k} {v}");
                        if ui
                            .selectable_label(self.selected.as_deref() == Some(&r), label)
                            .clicked()
                        {
                            self.selected = Some(r.clone());
                            self.tab = Tab::Graph;
                        }
                    }
                });
            egui::CollapsingHeader::new(format!("Nets ({})", self.model.nets.len())).show(
                ui,
                |ui| {
                    for (id, n) in self.model.nets.clone() {
                        if ui.selectable_label(false, format!("{id} ({n})")).clicked() {
                            use std::fmt::Write as _;
                            if !self.sim_vars.contains(&format!("v({id})")) {
                                if !self.sim_vars.trim().is_empty() {
                                    self.sim_vars.push(' ');
                                }
                                let _ = write!(self.sim_vars, "v({id})");
                            }
                            self.tab = Tab::Simulate;
                        }
                    }
                },
            );
            if !self.model.blocks.is_empty() {
                egui::CollapsingHeader::new(format!("Bloques ({})", self.model.blocks.len())).show(
                    ui,
                    |ui| {
                        for (h, _) in self.model.blocks.clone() {
                            ui.monospace(h);
                        }
                    },
                );
            }
        });
    }

    /// Right property inspector for the selected part.
    fn show_inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Inspector");
        let Some(sel) = self.selected.clone() else {
            ui.label("Sin selección.");
            ui.label("Click en una parte (Tables, Graph o Explorador).");
            return;
        };
        ui.monospace(&sel);
        ui.separator();
        if let Some((_, k, v)) = self.model.parts.iter().find(|(r, _, _)| r == &sel) {
            egui::Grid::new("insp").show(ui, |ui| {
                ui.label("Tipo");
                ui.monospace(k);
                ui.end_row();
                ui.label("Valor");
                ui.monospace(v);
                ui.end_row();
            });
        }
        if let Some(rest) = self.model.part_rest.get(&sel) {
            if !rest.is_empty() {
                ui.separator();
                ui.strong("Propiedades");
                for f in rest {
                    ui.monospace(f);
                }
            }
        }
        let nets: Vec<String> = self
            .model
            .nets_full
            .iter()
            .filter(|(_, pins)| pins.iter().any(|(r, _)| r == &sel))
            .map(|(id, _)| id.clone())
            .collect();
        if !nets.is_empty() {
            ui.separator();
            ui.strong("Nets");
            for id in nets {
                ui.monospace(id);
            }
        }
    }

    /// Help window content (Primeros pasos / Ayuda / Acerca de).
    fn show_help(ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Primeros pasos");
            ui.label("1. Abrí un .pdsprj o un .DSN (Archivo > Abrir, Ctrl+O).");
            ui.label("2. Mirá la pestaña Esquema (wires + etiquetas reales).");
            ui.label("3. Probá Simulate con un .SDF + ngspice.");
            ui.separator();
            ui.heading("Atajos");
            ui.monospace("Ctrl+O abrir · Ctrl+1/2/3/4 tabs · Ctrl+L fit · Esc limpiar · F1 ayuda");
            ui.separator();
            ui.heading("Acerca de");
            ui.label("Atenea GPL-3.0-or-later. RE interoperable, sin binarios vendor.");
            ui.monospace("Log: /tmp/atenea.log · Docs: docs/01-empezar.md");
        });
    }

    fn show_tables(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().show(ui, |ui| {
            egui::CollapsingHeader::new("Summary")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("summary").show(ui, |ui| {
                        for (k, v) in &self.model.summary {
                            ui.label(k);
                            ui.monospace(v);
                            ui.end_row();
                        }
                    });
                });
            if !self.model.parts.is_empty() {
                egui::CollapsingHeader::new(format!("Parts ({})", self.model.parts.len()))
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("parts").striped(true).show(ui, |ui| {
                            ui.label("Ref");
                            ui.label("Type");
                            ui.label("Value");
                            ui.end_row();
                            for (r, k, v) in &self.model.parts {
                                if ui
                                    .selectable_label(
                                        self.selected.as_deref() == Some(r.as_str()),
                                        r,
                                    )
                                    .clicked()
                                {
                                    self.selected = Some(r.clone());
                                    self.tab = Tab::Graph;
                                }
                                ui.monospace(k);
                                ui.monospace(v);
                                ui.end_row();
                            }
                        });
                    });
            }
            if !self.model.nets.is_empty() {
                egui::CollapsingHeader::new(format!("Nets ({})", self.model.nets.len())).show(
                    ui,
                    |ui| {
                        egui::Grid::new("nets").striped(true).show(ui, |ui| {
                            ui.label("Net");
                            ui.label("Pins");
                            ui.end_row();
                            for (id, n) in &self.model.nets {
                                ui.monospace(id);
                                ui.label(n.to_string());
                                ui.end_row();
                            }
                        });
                    },
                );
            }
            if !self.model.blocks.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "Blocks ({}, showing {})",
                    self.model.blocks.len(),
                    self.model.blocks.len().min(500)
                ))
                .show(ui, |ui| {
                    egui::Grid::new("blocks").striped(true).show(ui, |ui| {
                        ui.label("Head");
                        ui.label("Body");
                        ui.end_row();
                        for (h, b) in self.model.blocks.iter().take(500) {
                            ui.monospace(h);
                            ui.monospace(b);
                            ui.end_row();
                        }
                    });
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_scale_matches_tenth_inch_grid() {
        let raw = [
            plib::dsn::Wire {
                x1: 0,
                y1: 0,
                x2: 254_000,
                y2: 0,
            },
            plib::dsn::Wire {
                x1: 0,
                y1: 0,
                x2: 0,
                y2: -2_540_000,
            },
        ];
        let mm = wires_to_mm(&raw);
        assert_eq!(mm.len(), 2);
        assert!((mm[0].2 - 2.54).abs() < 1e-9);
        assert!((mm[1].3 - -25.4).abs() < 1e-9);
        assert_eq!(wires_to_mm(&[]), Vec::new());
    }

    #[test]
    fn bbox_covers_all_endpoints() {
        assert_eq!(wires_bbox(&[]), None);
        let ws = vec![(0.0, 0.0, 2.54, 0.0), (-1.0, 5.0, 3.0, -2.0)];
        let bb = wires_bbox(&ws).unwrap_or((0.0, 0.0, 0.0, 0.0));
        assert!((bb.0 - -1.0).abs() < 1e-12);
        assert!((bb.1 - -2.0).abs() < 1e-12);
        assert!((bb.2 - 3.0).abs() < 1e-12);
        assert!((bb.3 - 5.0).abs() < 1e-12);
    }

    #[test]
    fn world_screen_roundtrip() {
        let origin = egui::pos2(10.0, 20.0);
        let pan = egui::vec2(100.0, -50.0);
        let zoom = 2.5;
        let world = egui::pos2(123.4, -67.8);
        let screen = world_to_screen(world, pan, zoom, origin);
        let back = screen_to_world(screen, pan, zoom, origin);
        assert!((back.x - world.x).abs() < 1e-4);
        assert!((back.y - world.y).abs() < 1e-4);
    }

    #[test]
    fn fit_view_contains_bbox() {
        // siggen-like bbox (~350x147 mm) in an 800x600 viewport.
        let bbox = (-184.15, -82.55, 166.37, 64.77);
        let size = egui::vec2(800.0, 600.0);
        let (pan, zoom) = fit_view(bbox, size);
        assert!(zoom > 0.0);
        for (wx, wy) in [(bbox.0, bbox.1), (bbox.2, bbox.3)] {
            let s = world_to_screen(mm_to_world(wx, wy), pan, zoom, egui::Pos2::ZERO);
            assert!(s.x >= -1.0 && s.x <= size.x + 1.0);
            assert!(s.y >= -1.0 && s.y <= size.y + 1.0);
        }
    }
}
