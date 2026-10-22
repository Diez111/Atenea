# Atenea

Goal: push `atenea` toward a working native stack. Each package
runs as a background `pi -p` agent with strict STOP conditions; the
supervisor (me) verifies `fmt + clippy -D warnings + test` before
dispatching the next one.

Rules for every package: work inside `~/proyectos/atenea` only;
no licensing/activation/dongle work ever; no new dependencies without
need; keep the workspace green; time-box ~60 min; on failure document
negative findings in `docs/reverse-engineering.md` and stop.

## P1 — DSN wire coordinates (DONE 2026-10-22)
Decoded `WIRE` records, extended `plib::dsn` with `Wire{x1,y1,x2,y2}` +
`wires()` + `to_mm()`, tests on real `siggen.DSN`, docs updated.
Result: 101 plain + 17 bus records → **153 segments**, bbox
350.52×147.32 mm (units `[-18415000,16637000]` × `[-8255000,6477000]`).
Scale finding: **2_540_000 units/inch (1 unit = 10 nm)** — the
2540/10um guess from the KiCad community is refuted (1000× off;
2540 units = 1 thou). Verified on 5 shipped samples, all segments
Manhattan, lengths multiples of 25_400 units. See
`docs/reverse-engineering.md` §".DSN wire records".

## P2 — SPICE element parser (DONE 2026-10-22)
New `pmodel::spice` (`Element{designator,kind,nodes,value,params}`,
`R`/`C`/`D`/`E`/`F`/`G`/`H`/`I`/`V`, `.MODEL`, `.SUBCKT`, SPICE scales
with bare-`M`=milli vs `MEG`=mega documented). Verified on the 6 real
decks: LM123/LM323A/LM323K 62 elems each, LM338/LM2940T_5/LM2940T_10
66 each; every deck >5 elems, LM123 has E1/V1/F1/R1/C1/I1/D1.
See `docs/reverse-engineering.md` §"`.SML` SPICE decks".

## P3 — ngspice smoke test (DONE 2026-10-22)
LM123 deck (`ANALOG.SML`) ran verbatim in `ngspice-47` batch mode,
converged, exit 0, no errors. `.op` @ Vin=8 V → Vout=5.0998 V,
Iin≈1.032 A (5 Ω load + ≈12 mA quiescent); `.dc` 5→12 V shows
dropout <≈6.7 V, flat ≈5.10 V above (≈1 mV/V). Pins 3:IN 1:OUT 2:GND.
Testbench in `docs/ngspice-lm123.cir` (+ `lm123.mod` extracted via
`pmodel::sml`, procedure in `docs/reverse-engineering.md` §"ngspice
smoke test: LM123"). No Rust changes needed (deck + parser as-is).

## P4 — real-coordinate rendering (DONE 2026-10-22)
`Loaded` gained `wires: Vec<(f64,f64,f64,f64)>` (mm) via
`plib::dsn::wires` + `to_mm`, populated in `open_dsn()` and
`open_pdsprj()` (dsn present only); new `Tables|Graph|Wires` toggle.
Graph tab unchanged (grid + net edges + search); Wires tab draws only
the real geometry (thin green lines) fitted to screen on load
(`fit_view` on the mm bbox, pan/zoom reuse the Graph interact math,
`fit` button re-fits, search ignored). Pure helpers
(`wires_to_mm`, `wires_bbox`, `world_to_screen`/`screen_to_world`,
`fit_view`) + 4 `#[cfg(test)]` tests in `pview` (first unit tests in
that binary crate). `fmt + clippy -D warnings + test` green
(38 unit + 1 doc). On `siggen.DSN` the viewport shows **153 segments
spanning 350.52×147.32 mm**.

## P5 — DONE 2026-10-22
`LML` (`MODEL LIBRARY`) splitter; `corpus_check` classifier fix;
full sweep + docs. Finding: `ACTIVE.LML` reuses the `.SML` container
layout exactly (TOC count at `0x18`, 32-byte entries at `0x20`, chunks =
`u32` total length incl. 4 bytes + payload) — only the magic differs,
so `pmodel::sml` was generalized into shared `pmodel::container::split`
(`sml::split` signature unchanged, existing tests + `sml_dump` green)
with new `pmodel::lml` + `lml_dump`. Payload is LISA model text
(`FORMAT 8.0` mostly; `3.0`/`4.5`/`6.1` in DIGITAL/LAPLACE/PICAXE),
all 41 `ACTIVE.LML` decks parse via `pmodel::parse`; quirk: 6
title-less SPICELNK decks in `ANALOG.LML`. Sweep: 33 files →
**1097 decks** (full per-file counts in `docs/reverse-engineering.md`
§"`.LML` model libraries"). `corpus_check` dead `lml` arm fixed
(duplicate magic); `MODELS/` coverage 43/291 → 79/291 (27.1%).
`fmt + clippy -D warnings + test` green (42 unit + 1 doc).
