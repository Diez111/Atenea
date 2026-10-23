//! `aui`: Slint shell, 100 % glue over [`aengine`].
//!
//! The UI thread never parses, simulates or touches the disk: every action
//! sends an [`aengine::Job`] and polls the reply channel on a timer.

slint::include_modules!();

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
    let last_sdf: std::sync::Mutex<Option<(String, String)>> =
        std::sync::Mutex::new(None);

    {
        let app_w = app.as_weak();
        let eng = engine.clone();
        app.on_open_file(move |path| {
            let path = if path.is_empty() {
                rfd::FileDialog::new()
                    .add_filter("Proteus", &["pdsprj", "sdf", "mdf", "dsn", "idx"])
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
                            format!("{} — {} parts, {} nets", snap.name, snap.parts.len(), snap.nets.len()).into(),
                        );
                        app.set_parts(parts_of(&snap).into());
                        app.set_nets(nets_of(&snap).into());
                        app.set_erc_text(erc_text_of(&snap).into());
                        if snap.kind == "lisa" {
                            if let Ok(text) = std::fs::read_to_string(&path) {
                                *last_sdf.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                                    Some((snap.name.clone(), text));
                            }
                        }
                    }
                    Some(Err(e)) => app.set_status(format!("error: {e}").into()),
                    None => app.set_status("timeout abriendo archivo".into()),
                }
            }
        });
    }
    {
        let app_w = app.as_weak();
        let eng = engine.clone();
        app.on_run_sim(move |_| {
            let guard = last_sdf.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                        app.set_sim_text(
                            format!("{} puntos, última v(MID)={last:.4} V", t.rows.len()).into(),
                        );
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
        app.on_run_erc(move |_| {
            if let Some(app) = app_w.upgrade() {
                app.set_status("ERC recalculado (ver pestaña ERC)".into());
            }
        });
    }
    {
        let app_w = app.as_weak();
        app.on_pick_device(move |_| {
            if let Some(app) = app_w.upgrade() {
                app.set_status("Pick Device: buscador IDX (en curso)".into());
            }
        });
    }

    app.run()
}
