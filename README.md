# Atenea ⚡

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-edition_2024-orange.svg)](Cargo.toml)
[![Linux](https://img.shields.io/badge/platform-linux_x86__64-lightgrey.svg)](docs/01-empezar.md)

Reimplementación nativa para Linux de los formatos y herramientas del
ecosistema Labcenter Proteus (ingeniería inversa con fines de
interoperabilidad). No incluye ni distribuye ningún binario ni dato
propietario de Labcenter: solo parsers y herramientas escritas desde cero.

> Estado: visor nativo + parsers verificados + simulación batch real.
> No es un clon funcional completo — ver [Roadmap](#roadmap).

## Quickstart (60 segundos)

Requisitos: Linux x86_64, Rust 1.85+, `ngspice` (solo para simular).

```bash
git clone https://github.com/Diez111/Atenea.git && cd Atenea
cargo run -p atenea -- --help
cargo run -p atenea -- <archivo> [--tab Esquema|Simulate|Tables|Graph] [--gl]
# Sin archivo abre igual (bienvenida + Abrir + drag&drop). Doble-click en
# .pdsprj/.DSN/.LYT/.SDF/.MDF abre Atenea (MIME registrado). Log: /tmp/atenea.log
```

Sin Vulkan/VM: agregá `--gl`. Release instalado: `~/.local/bin/atenea` +
`atenea.desktop`. Detalles en [docs/01-empezar.md](docs/01-empezar.md).

## Arquitectura

```text
                    ┌─────────────┐
                    │  aui/atenea │  UI Slint 100% Rust (roadmap)
                    │  (binario)  │  hoy: egui+GPU Tables|Graph|Esquema|Simulate
                    └──────┬──────┘
                           │  fachada async aengine (sin UI lag)
        ┌─────────┬────────┴────────┬──────────────┐
        │         │                 │              │
   ┌────▼───┐ ┌───▼────┐      ┌─────▼─────┐  ┌────▼─────┐
   │  plib  │ │ pmodel │      │   pproj   │  │   asim   │
   │DSN/LYT │ │MDF/SDF │      │ pdsprj    │  │SDF->ngspice│
   │IDX/LIB │ │SML/LML │      │ PROJECT.XML│ │ngspice -b │
   │wires   │ │SPICE   │      │ ROOT.CDB  │  │Trace parse│
   └────────┘ └────────┘      └───────────┘  └────┬─────┘
        │         │                 │              │
        └─────────┴────────┬────────┴──────────────┘
                           │
                    ┌──────▼──────┐
                    │  vsm +      │  espejo ABI VSM + dlopen atenea_model_*
                    │  vsm-host   │  (+nullmodel, ne555, adc0808)
                    └─────────────┘
```

| Crate | Hace | Evidencia |
|---|---|---|
| `plib` | magics IDX/LIB, extractor DSN/LYT, wires, placements, join IDX↔LIB | 23/23 DSN, Cpu.LYT 43 bloques, join 6/7 |
| `pmodel` | LISA MDF/SDF, splitters SML/LML, spice (R/C/D/E/F/G/H/I/V) | 145/145 MDF, 1097 LML, 6 SML byte-exactos |
| `pproj` | ZIP pdsprj + PROJECT.XML + ROOT.CDB | 534/534 pdsprj, 2 sabores, 35 comps/120 recs |
| `vsm` | traits Spice/Dsim/Active + exports verificados | `createactivemodel` en ISIS.DLL |
| `asim` | SDF→netlist, `ngspice -b`, `Trace` | LM123 5.0998 V; RC tau=0.91ms |
| `vsm-host`+models | dlopen ABI propia + ne555/adc0808 | demo 10×1000ps, NE555 4.8kHz, ADC 128/255 |
| `atenea` | visor GPU: hoja ISIS, plot, Simulate | release 28-30 MB + tarball |

## Roadmap

- [x] Parsers: IDX/LIB join, SML/LML, LISA, pdsprj+CDB, DSN wires+labels
- [x] Conformidad: 568/568 en-scope, 145/145 MDF, 534/534 pdsprj, 1097 LML
- [x] Visor: hoja estilo ISIS, plot propio, `--gl`, MIME+desktop, log
- [x] Sim batch: ngspice LM123 5.0998 V + plugins dlopen + NE555/ADC0808
- [ ] Simulación interactiva paso a paso + sondas en vivo
- [ ] Editor esquemático/PCB + router (hoy solo visor)
- [ ] Geometría vectorial pads/trazos/símbolos (hoy: posiciones + wires)
- [ ] Empaquetado distro (PKGBUILD/AUR; hoy tarball + install.sh)

## Estado

- `crates/plib`: magics + string-table de la familia `.IDX`/`.LIB`
  (`DEVICE`/`PACKAGE`/`MODEL`/`SYMBOL` + índices), extractor `{...}` de
  `.DSN`/`.LYT`, colocados `ff|len|texto|x|y`, índice+directorio
  (join IDX↔LIB por key, offsets a cuerpos), examples `dsn_dump` y
  `corpus_check` (cobertura del corpus).
- `crates/pmodel`: documentos LISA `.MDF`/`.SDF` (preamble, `PARTLIST`
  con CSV entre comillas, `NETLIST` con redes nombradas, `MODELDEFS`)
  + splitter `.SML`/`.LML` (TOC 32 B + chunks con longitud; 4+2 decks reales SML,
  1097 decks LML), examples `lisa_dump`, `sml_dump`, `lml_dump`.
- `crates/pproj`: contenedores `.pdsprj` (ZIP + `PROJECT.XML` con entidades,
  `ROOT.DSN` validado, `ROOT.CDB` con 35 componentes/120 records en sample,
  cap anti zip-bomb), examples `pproj_dump`, `cdb_dump`.
- `crates/vsm`: modelo Rust de la interfaz VSM (exports `createactivemodel`/
  `deleteactivemodel`/`createdevice`/`deletedevice` verificados en `ISIS.DLL`,
  traits `SpiceModel`/`DsimModel`/`ActiveModel`).
- `crates/atenea` (dir `pview/`): visor nativo egui + GPU con pestañas
  Tables | Graph | **Esquema** | Simulate. La pestaña Esquema imita la hoja
  ISIS: fondo claro, retícula menor/mayor (0.1"/1"), borde de hoja azul,
  wires oscuros, etiquetas de componentes, capas conmutables, pan/zoom con
  autofit, status bar con cursor en mm y thou (`--tab TAB`, `--gl` para VMs).
  Binario `atenea` + launcher `atenea.desktop`.
- `crates/asim`: simulación batch (SDF → netlist ngspice con `9k1`→`9.1k`,
  `4M7`→`4.7MEG`, red GND→nodo 0; corre `ngspice -b`, parsea `Trace`).
- `crates/vsm-host` + `crates/nullmodel`: plugins nativos (`dlopen` +
  ABI `atenea_model_*`; demo carga y pisa 10×1000 ps verificado).
- Cobertura medida: `corpus_check` sobre 2374 archivos reales.
- Conformidad: 568/568 archivos en-scope, 1097 decks LML, 6 decks SML
  byte-idénticos a GETSPICE/GETMDF oficiales, 145/145 MDF extraídos
  parsean (incl. sub-secciones `*RED,n` y variante sin título SPICELNK).
- Licencia del workspace: GPL-3.0-or-later (`LICENSE`).

## Mapeo total (RE propia, verificada en disco)

### 1. Binarios de Proteus 8.6 (`BIN/`, PE32, analizados con Ghidra 12.1.2 vía REA)

| Binario | Rol | Hallazgo clave |
|---|---|---|
| `PDS.EXE` (1.5 MB) | Launcher único | entry point; ISIS/ARES son DLLs cargadas por él |
| `ISIS.DLL` (2.6 MB, 3040 fns) | Editor esquemáticos | importa `createactivemodel`/`deleteactivemodel` (carga modelos VSM); usa `NETLIST.dll` |
| `ARES.DLL` (2.6 MB, 4324 fns) | Editor PCB | usa `CDBCORE` (createcdb/save/load/begin+endsyncboard) como puente ISIS↔ARES |
| `NETLIST.dll` (200 KB) | Netlister | clases `NETLIST/PARTITION/PINOUT/NETLOG/NETCACHE/PROPDEF` + `CDBCORE`; `parse_models`/`write`/`net_gen`/`resolve` descompilados |
| `DSIM.DLL` | Simulación digital | expone `dsim_create` + `IIDSIMMODEL` |
| `SPICESIM.DLL`, `PROSPICE.*` | Simulación analógica (SPICE3F5 + ext.) | 24 analógicas + 22 digitales + mixtas + PLD + sondas (ver `docs/`) |
| `VSMDEBUG.dll` | Carga de modelos | `GetProcAddress` + `MODDLL`; `createmoduleinstance` es solo factory |
| `GETSPICE/PUTSPICE/GETMDF/PUTMDF` | CLI de librerías | extraen/insertan decks; usados para validación byte-exacta |
| `SDFGEN.EXE` | Conversor | ISIS 2.0 → SDF 3.0 |
| `LICENCE.EXE`, `DIYSTAMP` (16-bit), `LUAC` | varios | fuera de alcance (licencias: no se tocan) |

### 2. Formatos de archivo (todos parseados por Atenea)

| Formato | Magic / contenedor | Parser | Evidencia |
|---|---|---|---|
| `.IDX` | `DEVICE LIBRARY INDEX\x1A`, count en offset 36 | `plib::libdir::parse_idx` | 7 entradas ArduinoTEP, join por key |
| `.LIB` (DEVICE/PACKAGE/MODEL/SYMBOL) | magics + `\x1A` | `plib` + `libdir::parse_lib_dir` | directorio: nombre[64]+seq+key+offset, stride 80, 6/7 offsets vivos |
| `.LML` | `MODEL LIBRARY\x1A` + TOC 32 B | `pmodel::lml` | 33 archivos, 1097 decks parseados |
| `.SML` | `SPICE LIBRARY\x1A` + TOC + chunks con longitud | `pmodel::sml` | 6 decks **byte-idénticos** a GETSPICE oficial |
| `.MDF`/`.SDF` (LISA) | títulos `FORMAT x.y`, secciones `*N,n` | `pmodel` | 145/145 MDF + Ppsu.SDF, incl. `*RED,n` anidadas |
| `.DSN` | `ISIS SCHEMATIC FILE\x1A` | `plib::dsn` | 23/23; wires `WIRE+pts i32 LE`, escala 2.54M uds/pulg |
| `.pdsprj` | ZIP (`PROJECT.XML`, `ROOT.CDB`, `ROOT.DSN`) | `pproj` | 534/534, 2 sabores (esquemático / firmware) |
| `.CDB` | binario + `{...}` | `pproj::cdb` | 35 comps + 120 records en sample; validado vs `CDBCORE::save/load` |
| `.LYT` | `PCB LAYOUT FILE\x1A` | `plib::dsn` (reutilizado) | Cpu.LYT: 43 bloques `{*VIEW3D}` |
| `.VML`/`.3DS` | magics `MM`/`LX` | clasificados (sin parser geométrico) | 33 archivos |
| `.qm`, `.chm`, firmware | varios | docs extraídos, no parseados | 63+136 páginas HTML leídas |

### 3. ABI VSM (modelos de simulación)

Documentación comunitaria + strings verificados en binarios: clase
`IACTIVEMODEL::{initialize, getspicemodel, getdsimmodel, plot, animate,
actuate}`, especializaciones `ISPICEMODEL`/`IDSIMMODEL`/`IMIXEDMODEL`,
exports `createactivemodel`/`deleteactivemodel` (+ batch
`createdevice`/`deletedevice`), binding `MODDLL=<dll>` +
`PRIMITIVE=ACTIVE`. Atenea implementa el espejo nativo (`crates/vsm` +
`vsm-host` con `dlopen` + ABI `atenea_model_*` + `nullmodel` de ejemplo).
El `VSMSDK.chm` oficial es solo una página de contacto con NDA.

### 4. Herramientas y skills de RE usados

- **REA 6.7.0** (`~/.local/bin/rea`, MCP `rea` en Pi + skill
  `reverse-engineer-anything`): `inspect`, `search`, `decompile`,
  `inspect-pe-resources`, `inspect-analysis-view`, snapshots Ghidra.
- **Ghidra 12.1.2** (`/opt/ghidra`, `GHIDRA_INSTALL_DIR`): imports,
  decompilación, pseudocódigo con mangling MSVC preservado.
- **7z/innoextract/strings/Ghidra-headless**: CHM→HTML, Inno Setup,
  inventario de exports, batch CLI verificable.
- **ngspice-47**: backend de simulación batch (`asim`, LM123 → 5.0998 V).
- **KiCad importer thread** (comunidad): CDB 98% + scaler DSN (corregido
  por nosotros a 2.54M tras refutar el 2540).
- **ee-diary / TEP**: ABI VSM pública documentada por la comunidad.

### 5. Qué falta (honesto)

Simulación interactiva paso a paso; editor esquemático/PCB con router;
geometría de pads/trazos/símbolos (posiciones sí, dibujo no);
netlist exportada editable; empaquetado distro (PKGBUILD/AUR).

## Validar

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

## Licencia

GPL v3 o posterior — ver `LICENSE`.
