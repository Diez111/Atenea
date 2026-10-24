//! `aui`: Slint shell, 100 % glue over [`aengine`].
//!
//! The UI thread never parses, simulates or touches the disk: every action
//! sends an [`aengine::Job`] and polls the reply channel on a timer.

slint::include_modules!();

use slint::Model as _;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Poll a reply channel without blocking the UI thread.
fn poll<T>(rx: &Receiver<T>, wait: Duration) -> Option<T> {
    rx.recv_timeout(wait).ok()
}

fn parts_of(snap: &aengine::DocSnapshot) -> Vec<PartRow> {
    snap.parts
        .iter()
        .map(|p| PartRow {
            reference: p.reference.clone().into(),
            kind: p.kind.clone().into(),
            value: p.value.clone().into(),
        })
        .collect()
}

fn nets_of(snap: &aengine::DocSnapshot) -> Vec<NetRow> {
    snap.nets
        .iter()
        .map(|n| {
            let pins = n
                .pins
                .iter()
                .map(|(r, pin, _)| format!("{r}.{pin}"))
                .collect::<Vec<_>>()
                .join(" ");
            NetRow {
                id: n.id.clone().into(),
                pins: pins.into(),
            }
        })
        .collect()
}

fn erc_text_of(snap: &aengine::DocSnapshot) -> String {
    if snap.erc.is_empty() {
        return "No ERC errors found.".to_owned();
    }
    snap.erc
        .iter()
        .map(|(sev, msg)| format!("{sev}: {msg}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> Result<(), slint::PlatformError> {
    let app = Atenea::new()?;
    let engine = aengine::Engine::spawn();
    // Never show an empty black hole: default beige ISIS sheet.
    {
        let empty = aengine::DocSnapshot {
            name: "UNTITLED".to_owned(),
            kind: "dsn".to_owned(),
            ..aengine::DocSnapshot::default()
        };
        let svg = aengine::sheet_svg(&empty, 1200, 800);
        let path = std::env::temp_dir().join("atenea-sheet.svg");
        if std::fs::write(&path, svg).is_ok()
            && let Ok(img) = slint::Image::load_from_path(&path)
        {
            app.set_sheet(img);
        }
        app.set_status("abrí un archivo con File > Open".into());
    }
    let last_sdf = std::sync::Arc::new(std::sync::Mutex::new(None));
    let lib_cache: std::sync::Arc<std::sync::Mutex<Vec<PartRow>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    {
        let last_sdf = last_sdf.clone();
        let app_w = app.as_weak();
        let eng = engine.clone();
        app.on_open_file(move |path| {
            let path = if path.is_empty() {
                rfd::FileDialog::new()
                    .add_filter(
                        "Proteus",
                        &["pdsprj", "sdf", "mdf", "dsn", "lyt", "idx", "pdsclip"],
                    )
                    .pick_file()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                path.to_string()
            };
            if path.is_empty() {
                return;
            }
            if let Some(app) = app_w.upgrade() {
                match poll(
                    &eng.open(std::path::Path::new(&path)),
                    Duration::from_secs(30),
                ) {
                    Some(Ok(snap)) => {
                        app.set_status(
                            format!(
                                "{} — {} parts, {} nets",
                                snap.name,
                                snap.parts.len(),
                                snap.nets.len()
                            )
                            .into(),
                        );
                        app.set_parts(
                            std::rc::Rc::new(slint::VecModel::from(parts_of(&snap))).into(),
                        );
                        app.set_nets(
                            std::rc::Rc::new(slint::VecModel::from(nets_of(&snap))).into(),
                        );
                        app.set_erc_text(erc_text_of(&snap).into());
                        let errs = snap.erc.iter().filter(|(s, _)| *s == 'E').count();
                        let warns = snap.erc.len() - errs;
                        app.set_drc(
                            if errs == 0 && warns == 0 {
                                "No ERC errors".to_owned()
                            } else {
                                format!("{errs} errors, {warns} warnings")
                            }
                            .into(),
                        );
                        if snap.kind == "dsn" || snap.kind == "lyt" {
                            // ISIS = hoja beige clara, ARES = hoja negra.
                            let svg = if snap.kind == "lyt" {
                                aengine::sheet_svg_dark(&snap, 1200, 800)
                            } else {
                                aengine::sheet_svg(&snap, 1200, 800)
                            };
                            let svg_path = std::env::temp_dir().join("atenea-sheet.svg");
                            if std::fs::write(&svg_path, svg).is_ok()
                                && let Ok(img) = slint::Image::load_from_path(&svg_path)
                            {
                                app.set_sheet(img);
                            }
                        }
                        if snap.kind == "lisa"
                            && let Ok(text) = std::fs::read_to_string(&path)
                        {
                            *last_sdf
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                                Some((snap.name.clone(), text));
                        }
                    }
                    Some(Err(e)) => app.set_status(format!("error: {e}").into()),
                    None => app.set_status("timeout abriendo archivo".into()),
                }
            }
        });
    }
    {
        let last_sdf = last_sdf.clone();
        let app_w = app.as_weak();
        let eng = engine.clone();
        app.on_run_sim(move || {
            let guard = last_sdf
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some((name, sdf)) = guard.clone() else {
                if let Some(app) = app_w.upgrade() {
                    app.set_status("abrí un .sdf/.mdf primero".into());
                }
                return;
            };
            drop(guard);
            if let Some(app) = app_w.upgrade() {
                let vars = vec!["v(MID)".to_owned()];
                match poll(
                    &eng.simulate(&name, &sdf, &vars, "5m"),
                    Duration::from_secs(120),
                ) {
                    Some(Ok(t)) => {
                        let last = t.last.first().copied().unwrap_or(f64::NAN);
                        let mut msg = format!("{} puntos, última v(MID)={last:.4} V", t.rows.len());
                        if !t.subs.is_empty() {
                            msg.push_str(&format!("\nmodelos: {}", t.subs.join("; ")));
                        }
                        let plot_svg = aengine::plot_svg(&t, 800, 400);
                        let plot_path = std::env::temp_dir().join("atenea-plot.svg");
                        if std::fs::write(&plot_path, plot_svg).is_ok()
                            && let Ok(img) = slint::Image::load_from_path(&plot_path)
                        {
                            app.set_plot(img);
                        }
                        app.set_sim_text(msg.into());
                        app.set_status("sim OK".into());
                    }
                    Some(Err(e)) => app.set_sim_text(format!("error: {e}").into()),
                    None => app.set_sim_text("timeout sim".into()),
                }
            }
        });
    }
    {
        let app_w = app.as_weak();
        app.on_run_erc(move || {
            if let Some(app) = app_w.upgrade() {
                app.set_status("ERC recalculado (ver pestaña ERC)".into());
            }
        });
    }
    {
        let lib_cache = lib_cache.clone();
        let app_w = app.as_weak();
        let eng = engine.clone();
        app.on_open_lib(move || {
            let path = rfd::FileDialog::new()
                .add_filter("Proteus library", &["idx"])
                .pick_file()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_empty() {
                return;
            }
            if let Some(app) = app_w.upgrade() {
                match poll(
                    &eng.open(std::path::Path::new(&path)),
                    Duration::from_secs(30),
                ) {
                    Some(Ok(snap)) => {
                        let rows = parts_of(&snap);
                        *lib_cache
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = rows.clone();
                        app.set_lib_parts(std::rc::Rc::new(slint::VecModel::from(rows)).into());
                        app.set_status(
                            format!("{} — {} dispositivos", snap.name, snap.parts.len()).into(),
                        );
                    }
                    Some(Err(e)) => app.set_status(format!("error: {e}").into()),
                    None => app.set_status("timeout abriendo IDX".into()),
                }
            }
        });
    }
    {
        let lib_cache = lib_cache.clone();
        let app_w = app.as_weak();
        app.on_filter_lib(move |q| {
            let q = q.to_lowercase();
            let guard = lib_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let rows: Vec<PartRow> = guard
                .iter()
                .filter(|p| {
                    q.is_empty()
                        || p.reference.to_lowercase().contains(&q)
                        || p.kind.to_lowercase().contains(&q)
                        || p.value.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
            let n = rows.len();
            if let Some(app) = app_w.upgrade() {
                app.set_lib_parts(std::rc::Rc::new(slint::VecModel::from(rows)).into());
                app.set_status(format!("{n} coincidencia(s)").into());
            }
        });
    }
    {
        let app_w = app.as_weak();
        app.on_tool_selected(move |t| {
            if let Some(app) = app_w.upgrade() {
                app.set_status(format!("herramienta: {t}").into());
            }
        });
    }
    // Session schematic (SimulIDE-style: palette -> place -> simulate).
    // Parts placed here render on the sheet, list in Tables and feed ERC.
    let session: std::sync::Arc<
        std::sync::Mutex<(
            Vec<aengine::PartView>,
            Vec<(String, f64, f64)>,
            std::collections::HashMap<String, usize>,
            f64,
        )>,
    > = std::sync::Arc::new(std::sync::Mutex::new((
        Vec::new(),
        Vec::new(),
        std::collections::HashMap::new(),
        25.4,
    )));
    // Autoload the bundled Proteus library: the palette is never empty.
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/diez".to_owned());
        let idx = std::path::PathBuf::from(format!(
            "{home}/.wine-proteus/drive_c/Program Files (x86)/Labcenter Electronics/Proteus 8 Professional/LIBRARY/ArduinoTEP.IDX"
        ));
        if idx.exists() {
            match poll(&engine.open(&idx), Duration::from_secs(30)) {
                Some(Ok(snap)) => {
                    let rows = parts_of(&snap);
                    *lib_cache
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = rows.clone();
                    app.set_lib_parts(std::rc::Rc::new(slint::VecModel::from(rows)).into());
                    app.set_status(
                        format!("{} dispositivos listos — elegí y Place", snap.parts.len()).into(),
                    );
                }
                _ => app.set_status("no se pudo autocargar IDX".into()),
            }
        }
    }
    {
        let app_w = app.as_weak();
        app.on_select_lib(move |i| {
            if let Some(app) = app_w.upgrade() {
                let n = app.get_lib_parts().iter().count();
                if i >= 0
                    && (i as usize) < n
                    && let Some(row) = app.get_lib_parts().iter().nth(i as usize)
                {
                    app.set_selected_lib(i);
                    app.set_selected_name(format!("{} — {}", row.reference, row.value).into());
                }
            }
        });
    }
    {
        let app_w = app.as_weak();
        let session = session.clone();
        app.on_place_selected(move || {
            let Some(app) = app_w.upgrade() else { return };
            let i = app.get_selected_lib();
            let Some(row) = app.get_lib_parts().iter().nth(i.max(0) as usize) else {
                app.set_status("elegí un dispositivo primero".into());
                return;
            };
            let prefix: String = row
                .kind
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(2)
                .collect();
            let prefix = if prefix.is_empty() {
                "U".to_owned()
            } else {
                prefix.to_uppercase()
            };
            let mut guard = session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (parts, placements, counters, cursor_x) = &mut *guard;
            let n = counters.entry(prefix.clone()).or_insert(0);
            *n += 1;
            let reference = format!("{prefix}{n}");
            parts.push(aengine::PartView {
                reference: reference.clone(),
                kind: row.kind.to_string(),
                value: row.value.to_string(),
                rest: Vec::new(),
            });
            placements.push((reference.clone(), *cursor_x, 0.0));
            *cursor_x += 25.4;
            let snap = aengine::DocSnapshot {
                name: "UNTITLED".to_owned(),
                kind: "dsn".to_owned(),
                parts: parts.clone(),
                placements: placements.clone(),
                erc: aengine::erc_of(&aengine::DocSnapshot {
                    name: "UNTITLED".to_owned(),
                    kind: "dsn".to_owned(),
                    parts: parts.clone(),
                    placements: placements.clone(),
                    ..aengine::DocSnapshot::default()
                }),
                ..aengine::DocSnapshot::default()
            };
            let svg = aengine::sheet_svg(&snap, 1200, 800);
            let svg_path = std::env::temp_dir().join("atenea-sheet.svg");
            if std::fs::write(&svg_path, svg).is_ok()
                && let Ok(img) = slint::Image::load_from_path(&svg_path)
            {
                app.set_sheet(img);
            }
            app.set_parts(std::rc::Rc::new(slint::VecModel::from(parts_of(&snap))).into());
            app.set_erc_text(erc_text_of(&snap).into());
            let errs = snap.erc.iter().filter(|(s, _)| *s == 'E').count();
            app.set_drc(
                if errs == 0 {
                    "No ERC errors".to_owned()
                } else {
                    format!("{errs} errors")
                }
                .into(),
            );
            app.set_status(format!("{reference} colocado en la hoja").into());
        });
    }
    {
        let app_w = app.as_weak();
        app.on_show_page(move |p| {
            if let Some(app) = app_w.upgrade() {
                app.set_page(p);
            }
        });
    }
    {
        let app_w = app.as_weak();
        app.on_pick_device(move || {
            if let Some(app) = app_w.upgrade() {
                app.set_status("Pick Device: elegí en la lista y Place".into());
            }
        });
    }

    app.run()
}
