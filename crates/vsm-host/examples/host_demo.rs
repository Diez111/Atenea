//! Load a plugin `.so` and step it ten times.
//!
//! Usage: `cargo run -p vsm-host --example host_demo -- <plugin.so>`

use std::env;

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: host_demo <plugin.so>");
        std::process::exit(2);
    };
    // SAFETY: demo loads its own freshly built reference plugin.
    let mut plugin = match unsafe { vsm_host::Plugin::load(std::path::Path::new(&path)) } {
        Ok(p) => p,
        Err(e) => {
            eprintln!("host_demo: load failed: {e}");
            std::process::exit(1);
        }
    };
    for _ in 0..10 {
        if let Err(code) = plugin.step(1000) {
            eprintln!("host_demo: step failed: {code}");
            std::process::exit(1);
        }
    }
    println!("stepped 10x1000ps OK: {path}");
}
