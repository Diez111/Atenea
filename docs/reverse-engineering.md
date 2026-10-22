# Reverse engineering notes (interoperability only)

Facts observed from Proteus 8.6 SP2 (wine prefix + shipped samples).
No Labcenter binaries or data are reproduced here.

## Layout

- `BIN/PDS.EXE` (PE32, 1.5 MB): single launcher, `ISIS.DLL`/`ARES.DLL` inside.
- `BIN/`: ~70 VSM model DLLs (`AVR`, `PIC16/18`, `ARM7TDMI`, `LPC2000`...),
  `PROSPICE`, Qt4 + python35 + lua runtimes, `Tools/` (MPASM, AVRDUDE...).
- `LIBRARY/`: ~130 `.IDX`+`.LIB` pairs + 7 lone `.LIB` + ~20 `.VML` +
  `PROPDEFS.INI`, `PWRRAILS.INI` (plaintext, property vocabulary).
- `MODELS/`: 10× `.MDF`, 37× `.SML`, 35× `.LML`, ~150 VSM `.DLL`.
- `SAMPLES/`: mostly `.pdsprj`, 23 legacy `.DSN`, `.LYT`, `.SDF`, firmware sources.
- `Translations/`: 29× Qt `.qm`. `HELP/`: 54× `.chm` (docs, `VSMSDK.chm` most
  relevant for VSM DLL interface — not yet read).

## Ghidra findings (REA + Ghidra 12.1.2)

- `BIN/ISIS.DLL` (PE32, sha256 `e0cec780…`): 3040 procedures, 3108 strings.
  No PDB (`ISIS.pdb` referenced, absent) → recovered names only.
- Netlisting is delegated to `BIN/NETLIST.dll` (200 KB) via C++ imports:
  classes `NETLIST`, `PARTITION`, `PINOUT`, `NETLOG`, `NETCACHE`, `PROPDEF`.
- `NETLIST` API surface (MSVC mangled imports observed in ISIS.DLL):
  `setcdbroot`, `setdsnprops`, `setdsnstats`, `resolve`, `validate`,
  `addwire`, `addnet`/`newnet`, `addpin`/`addlabel`/`addelement`/`addmodule`,
  `parse_script`/`parse_define`/`parse_map`/`parse_models`/`parse_field`/
  `parse_netprop`/`parse_requires`, `parse_propdefs`/`format_propdefs`,
  `net_gen`, `net_erc`, `write`/`writeheader`, `mdf_id`.
- Strings: `SPICEMODEL`, `MODFILE`, `VSM DLL Model [%s]`, `DSNCVT40.DLL`,
  `dsn_editdesign`, `Complile Netlist` [sic].
- `NETLIST::parse_models` (`?parse_models@...`, RVA **0x1bb90** (368 B, Ghidra 12.1.2 verifica nombre+body) — corrección:
  0x1bb90 es `parse_netprop`): lee líneas hasta EOF o línea que empiece
  con `*`; `getpropname(ptr,buf,':')` + `skipspaces` →
  `PROPLIST::add(lista+0x9c, nombre, valor)`. Confirma el diseño de
  `pmodel` (preámbulo `Clave: valor`, secciones `*`).
- `NETLIST::write` (`?write@...`, decompilado por nombre via REA/Ghidra:
  `void __thiscall NETLIST::write(NETLIST*, ostream*, int)`): emite en orden
  `*PROPERTIES,XXXXX` → `*MAPPINGS,` (SIN conteo) → `*MODELDEFS[,XXXXX]` →
  `*SCRIPT ` … `*ENDSCRIPT` → `*PARTLIST[,XXXXX]` → `*NETLIST[,XXXXX]`
  (backpatch `XXXXX`, `#%05d`); excluye props `DSN_*` (strncmp); separadores
  ` : ` y `, `. `pmodel::emit` replica todo (MAPPINGS bare, ENDSCRIPT,
  filtro DSN_*). (RVA **0x55f0** — corrección: 0x17af0 era
  file-offset de otra build): emite `*PROPERTIES,XXXXX\n` (conteo como
  placeholder a rellenar), itera props excluyendo `DSN_*`. Patrón writer:
  sección + backpatch de conteo.
- Export table (515 exports; RVAs de parse manual NO verificadas —
  el parseo crudo desalinea `AddressOfFunctions`: Ghidra-por-nombre es la
  fuente de verdad). Nombres confirmados por string: `parse_field`,
  `parse_map`, `parse_netprop`, `parse_define`, `parse_requires`,
  `parse_script`, `read_mappings/models/nets/parts/props/script`,
  `resolve`, `net_erc`, `net_gen`, `bom`, `dxp`, `addscript`,
  `writeheader`, `propkwd/propsubst/propval`, `validate`, `sortnets`,
  `gettitle`, `write`, `setcdbroot`, `setdsnprops`.
- `net_gen` ×2 (0x1000fa00 ostream / 0x1000fc50 path): despacha por índice
  de formato vía tabla `net_formats` + punteros `PTR_FUN_10030390[i]`;
  genera nombres con `tfngen`/`addext`, abre con `fopen(..."w")`.
  Pseudocódigo completo guardado en `/tmp/nl_decomp/`.
- Mapping to native crates: `PROPDEF` ↔ `*PROPDEFS` vocabulary
  (`PROPDEFS.INI`); `NETLIST::write` ↔ `*PARTLIST`/`*NETLIST` in `pmodel`;
  `setcdbroot` ↔ `ROOT.CDB` in `pproj`.
- `CDBCORE::save/load` (`NETLIST.dll`, 0x100088a0/0x100083f0, 442/304 B)
  descompilados: I/O por `VFILE` virtual (`vfgetstr`/`vfputstr` para
  strings Pascal), conteos y luego módulos/hojas (`newmodule`). `pproj::cdb`
  v1: records = `u32 seq | PString name | u32 pin-count | 2N PStrings |
  footer` — el `u32` previo es **secuencia** (0x28,0x29,0x2A… por record),
  no longitud; tablas de pines son pares planos (U1 = 7 pares).
  descompilados: I/O por `VFILE` virtual (`vfgetstr`/`vfputstr` para
  strings Pascal), conteos y luego módulos/hojas (`newmodule`). Confirma
  el diseño de `pproj::cdb` (strings con longitud + records `{...}`).

## VSM + PROSPICE (from extracted CHM docs + community + ISIS strings)

- ISIS loads models itself (`ISIS.DLL` strings): `LoadLibrary(%s)` →
  signature check → `GetProcAddress(createactivemodel)` → create →
  `authorize(ILICENCESERVER)` (`GLE=0x%08lX` = `GetLastError`);
  missing `deleteactivemodel` is only a warning. `vsm-host` mirrors
  exactly this sequence with `libloading`.
- `dsim_create` lives in `DSIM.DLL` (not ISIS); `MODDLL`,
  `GETSPICE`/`GETMDF`/`SDFGEN` are external `BIN/*.EXE`, not ISIS
  imports — ISIS receives an already-resolved path.
- `VSMSDK.chm` is an NDA contact page only — no VSM C++ ABI documented.
  Community docs (ee-diary, public VSM guides) describe the de-facto ABI:
  ```cpp
  class my_device : public IACTIVEMODEL {
    VOID initialize(ICOMPONENT*);
    ISPICEMODEL* getspicemodel(CHAR* primitive);
    IDSIMMODEL* getdsimmodel(CHAR* primitive);
    VOID plot(ACTIVESTATE); VOID animate(INT, ACTIVEDATA*);
    BOOL actuate(WORD, INT, INT, DWORD);
  };
  extern "C" IACTIVEMODEL* createactivemodel(CHAR*, ILICENCESERVER*);
  extern "C" VOID deleteactivemodel(IACTIVEMODEL*);
  // batch mode: createdevice / deletedevice
  ```
  Specializations `ISPICEMODEL` / `IDSIMMODEL` / `IMIXEDMODEL`.
  Binding: `MODDLL=<dll>` + `PRIMITIVE=ACTIVE` in `*PARTLIST`.
  Loader: `VSMDEBUG.dll` (`GetProcAddress` + `MODDLL` strings observed);
  `DSIM.DLL` exports `dsim_create` + `IIDSIMMODEL`.
  Proteus 8 (32-bit) models are x86 DLLs; Proteus 9 moved to 64-bit.
- `MODELS.chm` documents the PROSPICE primitive set (SPICE3F5 + extensions
  like `PRECHARGE`, `pwrs`, `u/stp/uramp`): analog R/C/D/BJT/MOS(7 levels)/
  JFET/sources/switches/inductors, digital gates/BOOL/MEMORY, mixed
  ADC/DAC/DSWITCH/N-bit converters, PLD MATRIX/FUSE, real-time probes.
  This is the functional spec for reimplementing primitives (ngspice
  backend candidate). Full inventory in delegate artifacts.

## ARES / PCB (from extracted ARES.chm)

- No netlist *file* travels ISIS→ARES: live netlisting or batch sync
  into the project DB (`CDBCORE`: createcdb/save/load/begin/endsyncboard).
- Net classes defined in ISIS (wire label → Net Class tab); `POWER` default
  for GND/VCC, `BUS` for buses, rest → `SIGNAL`.
- Packages carry pads+silk; `USERPKG.LIB` for user parts; PADS ASCII
  2007/9.5 import for third-party footprints.
- Outputs: Gerber X2 (default, +IPC-D-356A netlist), RS274X, NC drill,
  ODB++, Pick&Place CSV, testpoint file, IDF/STEP/IGES/STL, DXF, panels.
- `.LYT`/`.DSN` are pre-v8 legacy (import wizard only); v8 works inside
  `.PDSPRJ`. No `.DTF`/`.LTF` mentioned anywhere.
- Outputs detail (ARES.DLL strings, 2026-10-22): `*.PKP` Pick&Place
  (fields `Part ID, Value, Package, Layer, Rotation, X, Y` + optional
  `Stock Code`, units thou/mm), `*.TPI` Testpoint Information
  (`LABCENTER PROTEUS TESTPOINT INFORMATION FILE`,
  `Testpoint positions for %s`). Gerber X2 needs a valid layer stackup;
  RS274X/X2 selector; NC drill literal
  `Format: Excellon, ASCII, %s, %s, absolute, eob=<CR><LF>, no zero suppression.`
- Layer stackup: `0=TOP, 15=BOTTOM, 1-14=INNER` + pseudo-layer `Drill Layer`
  (hole-only pads); drill X,Y relative to Output Origin.
- ARES imports `CDBCORE` API (`?createcdb@@YAPAVCDBCORE@@XZ`,
  `releasecdb`, `beginsyncboard`/`endsyncboard`, `tagpart`, `getpart`,
  sheet/module walkers) — live netlisting ISIS→ARES confirmed at string
  level (ARES.DLL 0xf3042/0xf3179). Pin role codes:
  `VCC IP OP IO PU PD PP LBL GT IT OT BT PT PR MOD` (+ `*NETLIST`,
  `*PARTLIST`, `DEVICE` section vocabulary).
- Router rules DSL (Electra-DSN style): `(layer_rule`, `(use_layer`,
  `(circuit`, `(use_net`, `(type signal/power/mixed)`, `(flip_style`,
  `(cost high (type length))`, `(direction off/vertical/horizontal/orthogonal)`;
  9 `clearance` pairs (area_via, area_wire, via_smd, via_via, via_pin,
  wire_wire, wire_pin, wire_smd, wire_via).
- Optimizer: gate-swap (`Ratsnest reduced by %g inches in %d passes`).

.UI icons (negative result, 2026-10-22): PE resource sections in
PDS.EXE/ISIS.DLL/ARES.DLL are 1536 bytes each (version info only, no
`RT_ICON`/`RT_BITMAP` gallery); no `.rcc`/`.qrc`/toolbar PNGs ship with
the product (Qt4 `QtGui4.dll` draws chrome/icons in code). Conclusion:
no extractable icon set exists — native UI draws its own glyphs
(recepción limpia, no copied assets).

## Third-party tools (native replacement path)

### ELECTRA router (ELECTRA.DLL, strings dump)

- Specctra-compatible CLI: `-design <file> -do <file> -w <routes> [-nog] [-quit]`;
  `.ses` sessions, `.do` scripts, `log.sts`.
- Report: `Completion / Total Pins / Power Layers / Signal Layers /
  Routed Length / Vias / Connections / Components`.
- Commands: fit/grid/off/max_total_vias/bend/limit_bends/cross/via/way/star/
  starburst/daisy_chain/term/limit/max_stub/junction/tjunction/reorder/
  max_stagger/via_on_pin/via_at_smd/clearance; costs
  `C@Max cost def/for/hig/med/low/free`; `route (share_len/max_len/
  via_share/pin_share/direction in/out)`; `rule pcb (via_at_smd on/off)
  (grid on/off) (fit on/off)`; `bus diagonal`, `squeeze/side_exit/off_center`.
- Protection: `Protect/Lock/UnProtect/Unlock All Wires`.

### SDFGEN / DXFCVT / DSNCVT40 / CONSOLE (strings dumps)

- `SDFGEN <file> <format>` converts to SDF 3.0 (`MULTIWIRE`, `TANGO`);
  emits `ISIS SCHEMATIC DESCRIPTION FORMAT 2.0` + Design/Doc.no./Revision/
  Created/Modified + `*PARTLIST,%d` (`%s,%s,%s,PACKAGE=%s`) + `*NETLIST,%d`;
  pin codes `PS IP OP IO TS OE OC PP LBL GT IT OT BT PR MOD`;
  `... too long (max %d) - clipped` warnings.
- DXFCVT exports `convert endmsg getinfo initialise terminate`; DXF
  `AC1009/1012/1014/1500` (release 12-15 ASCII, `HEADER/ENTITIES/BLOCKS/
  OBJECTS/LAYERS`); output `ARES REGION FILE` (`RGN`, `*HEADER`);
  units `Unitless..Parsecs`, `LX_REFERENCE_LAYER`.
- CONSOLE.EXE = PROSPICE runner (`simulate`, `dumptables`; PROSPICE.dll +
  LICENCE.dll, build 8.4.21720).

- `Tools/ARDUINO/`: Arduino IDE 1.8.1 copy (avr-gcc 4.9.2, avrdude 6.3.0,
  core 1.6.17). Native: `avr-gcc`, `avr-libc`, `avrdude`, `make` (Arch
  packages, certain). Windows-only bits (`dpinst`, `giveio.sys`, `msys-1.0.dll`)
  are dropped on Linux.
- `VSM Studio/drivers/arduino/`: open model sources (Firmata LGPL, SD GPLv3,
  TFT MIT, GPS BSD, Zumo MIT) — reusable as native model logic.
- Closed, no direct replacement: MPASM, PICAXE, AVRASM 1.30, ASM11, ASEM51,
  dongle drivers, VirtualUSB/Network drivers.
- `VSM Studio/SYNTAX/README.TXT` specifies the `.SYN`/`.MAP` highlighter
  format (PCRE, max 64 rules) — cheap to reimplement.

## Formats

.DSNCVT40 identity (strings dump): `ISIS IV Design Converter /
DSNCVT`, magics `ISIS CIRCUIT FILE\x1A`, `ISIS SECTION V3.5`,
`DESIGN CONFIGURATION DATA\x1A`, `CCT%03d OBJECT DATA`,
`ROOT%02d/REF/SUBSHEET%02d` (multi-sheet naming). `plib::dsn`
recognizes the CIRCUIT magic (framing family).

.NETLIST::net_gen dispatch (Ghidra 12.1.2 via REA, 2026-10-22):
`?net_gen@@YAXAAVNETLIST@@AAV?$basic_ostream@DU?$char_traits@D@std@@@std@@HPAD2@Z`
= `void net_gen(NETLIST&, ostream&, int, char*, char*)` @0x1000fa00.
Format table @0x10030388: 12-byte entries `{u32 key, u32 subkey, writer fn}`;
inline 3-letter codes `SDF NET CPT CMP CIR MDF` (+`NET` subkey on one entry),
writers @0xfd90..0x11c10. Failure path pushes error `0xa002`.
`pmodel::emit` mirrors the registry (`NETLIST_FORMATS` + `emit_format`);
only LISA (`SDF`/`MDF`) has a native writer.

### `.DSN` wire records (verified 2026-10-22, `plib::dsn`)

- Geometry: `WIRE 00 00 00 <n> 00` + `n` points of `(x, y)` little-endian
  `i32`. `BUS WIRE` shares the layout. `n`-point polylines expand to
  `n - 1` segments. Non-geometry records skipped: `WIRE LABEL`
  (label placement), `WIRE DOT` (junction dots), `WIRE 00 01 00 ff ...`
  (net annotations like `STRAT=POWER`).
- Scale: **2_540_000 units per inch (1 unit = 10 nm)**. Evidence over 5
  shipped samples (`siggen` 118 recs/153 segs, `Dbell` 207/269, `Cpu`
  241/281, `Epe` 761/907, `Features` 90/100): every segment is
  Manhattan, every length a multiple of 25_400 units (0.01 inch), modal
  segment 254_000 units = 0.1 inch; bboxes (239–361 × 142–215 mm)
  fit metric sheets (`siggen` 350.52×147.32 mm on A3, `LXFOOTER A3`
  in-file).
- Negative finding: the KiCad-community guess "scaled by 2540 =
  1 unit = 10 um" is **refuted** (1000× off — it would make a 0.1-inch
  wire 2.54 m long and the `siggen` sheet ~350 m wide). Plausible origin
  of the confusion: 2540 units = 1 thou (0.001 inch) = 25.4 um.
- Rendered natively since P4: `pview` Wires tab draws these segments in
  mm (`wires_to_mm`), auto-fitted (`siggen.DSN`: 153 segs, 350.52×147.32 mm).

| Format | Magic/head | Kind | Parser |
|---|---|---|---|
| `.IDX` | `DEVICE LIBRARY INDEX\x1A`, count u32@36, records `key+CAT[/MFR]/DESC` split by `01 00 00 00` + `12345678` sentinel | binary index + ASCII | `plib::libdir` (ArduinoTEP: 7 entries) |
| `.LIB` | `DEVICE LIBRARY\x1A`, dir@32 stride 80: `name[64]+seq+key+offset+flags`; `{*DEVICE}/{*COMPONENT}` blocks | binary + text frags | `plib::libdir::parse_lib_dir` + strings; join IDX↔LIB by key (6/7; UNO dup, Rev3 sin slot) |
| `.MDF` | `LISA MODEL DESCRIPTION FORMAT x.y` | text, `*SECTION,n` | `pmodel` |
| `.SDF` | `ISIS SCHEMATIC DESCRIPTION FORMAT x.y` | text, `*PARTLIST/*NETLIST` | `pmodel` |
| `.SML` | `SPICE LIBRARY\x1A`, `.SUBCKT/.MODEL/.ENDS` | text SPICE | `pmodel::sml` (split) + `pmodel::spice` (decks) |
| `.LML` | `MODEL LIBRARY\x1A`, LISA model text | text LISA | `pmodel::lml` (split, shared `pmodel::container`) + `pmodel::parse` |
| `.pdsprj` | `PK\x03\x04` (`PROJECT.XML`, `ROOT.CDB`, `ROOT.DSN`) | zip | `pproj` (XML + cap 256 MB + `cdb`: 35 comps/120 records en sample) |
| `.DSN` | `ISIS SCHEMATIC FILE\x1A` + `DESIGN CONFIGURATION DATA` | binary + tokens | `plib::dsn` (99–378 bloques por archivo; `WIRE`/`BUS WIRE`: 101+17 recs / 153 segmentos en `siggen.DSN`, bbox 350.52×147.32 mm) |
| `.LYT/.LTF` | `PCB LAYOUT FILE\x1A`, bloques `{*VIEW3D}`, padstacks, capas | mixed | `plib::dsn` (Cpu.LYT: 43 bloques); pads = `02 00 estilo pin`; escala 2.54M verificada (pitch 0.1\"=254000 y row 0.3\"=762000 i32 LE junto a estilos) |
| `.MDF` MODDLL | `MODDLL=X.DLL` refs into `MODELS/*.DLL` | native PE32 | stub only, no DLL RE in phase 1 |

## Roadmap (honest)

What exists today: format readers. A full Proteus replacement
(schematic editor + mixed-signal simulator + PCB router + MCU VSM models)
is a multi-year effort. Sensible phases:

1. Lectores (ahora): `plib`, `pmodel`, luego `pdsprj` + extractor tokens `.DSN`.
2. Visor: renderizar netlist/`PARTLIST` a esquemático básico (egui/wgpu).
3. Simulación: ngspice como backend para `.SML`/netlists (no reinventar SPICE).
4. PCB: importar `LYT`/`LTF` como solo-lectura antes de editar.
5. VSM: stubs de `MODDLL` con Unicorn/QEMU para cores MCU, último.

## Evidence

- REA CLI: `rea inspect-pe-resources BIN/PDS.EXE` → PE32, sha256 `f8b21643…`.
- Ghidra 12.1.2 + JDK 27 at `/opt/ghidra`, `rea doctor --provider ghidra` healthy.

### `.SML` SPICE decks (verified 2026-10-22, `pmodel::spice`)

- `MODELS/ANALOG.SML` → 4 decks (`LM123`, `LM323A`, `LM323K` 1072–1075 B,
  `LM338` 1441 B); `MODELS/TEXAS.SML` → 2 decks (`LM2940T_5` 1981 B,
  `LM2940T_10` 1973 B). Each deck is one `.SUBCKT` (`LM123` nodes `3 1 2`)
  with 4 `.MODEL` lines, parsed via `pmodel::sml::split` + `pmodel::spice::parse`.
- Element counts (parser output, `*` comments + trailing NUL line ignored):
  `LM123`/`LM323A`/`LM323K` = **62** each
  (13×`E`, 9×`V`, 8×`F`, 2×`H`, 15×`R`, 4×`C`, 2×`I`, 9×`D`);
  `LM338`/`LM2940T_5`/`LM2940T_10` = **66** each
  (13×`E`, 9×`V`, 8×`F`, 2×`H`, 19×`R`, 4×`C`, 2×`I`, 9×`D`).
  `LM123` contains `E1`/`V1`/`F1`/`R1`/`C1`/`I1`/`D1`; `R1 1 2 1E8` = 1e8 Ω.
- Grammar covered: `R`/`C` (value + optional model/`K=V` in `params`),
  `D` (model only, `value: None`), `E`/`G` (4 nodes + gain),
  `F`/`H` (2 nodes + sense source in `params[0]` + gain),
  `I`/`V` (2 nodes, first numeric token is `value`, `DC`/`AC` stay in
  `params`), `.MODEL name type params...` (merges `TC1=` + `0.5` splits),
  `.SUBCKT name nodes...`; `*` full-line comments, `+` continuations,
  unknown `.` directives skipped, stop at `.ENDS`.
- Scales (`parse_value`, case-insensitive): `T` 1e12, `G` 1e9, `MEG` 1e6,
  `K` 1e3, `M` 1e-3, `U` 1e-6, `N` 1e-9, `P` 1e-12, `F` 1e-15,
  `MIL` 25.4e-6; plain/`e`-exponents (`1E8`, `7.94228E+06`) + trailing
  units ignored. **Documented choice: bare `M` = milli (SPICE3), mega
  requires `MEG`** — so `1M` = 0.001, `1MEG` = 1_000_000.
- Quirks: `R` with model puts the model first (`R8 133 132 RQ 833.333`
  → `value` 833.333, `params` `["RQ"]`); `D` lines are model-only
  (`D1 15 16 DMOD`); multi-letter designators classify by first letter
  (`R_YY`, `ECCX`, `DON1`, `CBYPS`, `HSTEP` all parse); `sml` deck text
  ends with a NUL byte that yields an empty line the parser drops.

### `.LML` model libraries (verified 2026-10-22, `pmodel::lml`)

- `MODELS/ACTIVE.LML` (49632 bytes) reuses the `.SML` container layout
  exactly — only the magic differs (`MODEL LIBRARY\x1a`): `u32` deck
  count at `0x18` (41), 32-byte TOC entries from `0x20`
  (`name[16]` NUL-padded + `u32` zero + `u32` chunk offset +
  `u32` timestamp + `u32` unknown), chunks are `u32` total length
  including the 4 bytes + payload text, packed contiguously.
- Refactor: the split logic now lives in `pmodel::container::split`
  (takes the expected magic); `pmodel::sml::split` (same signature,
  `sml_dump` + existing tests untouched) and the new `pmodel::lml::split`
  (+ `lml_dump` example) delegate to it. Types `Deck`/`ParseError` are
  canonical in `container` and re-exported by both.
- Payload is LISA model text (`LISA MODEL DESCRIPTION FORMAT 8.0` in
  30/33 files; `3.0` in `DIGITAL.LML`, `4.5` in `LAPLACE.LML`, `6.1` in
  `PICAXE.LML`), parseable with `pmodel::parse` — verified for all 41
  `ACTIVE.LML` decks (`ACIMETER` … `WMETER`). Quirk: 6 title-less
  SPICELNK-converted decks in `ANALOG.LML` (`7806`, `7808`–`7810`,
  `7818`, `7824`) start at `Design:` with no `FORMAT` title line, so
  they split fine but `pmodel::parse` rejects them (`BadTitle`).
- Sweep: all 33 `MODELS/*.LML` split cleanly → **1097 decks**
  (262 `DIGITAL`, 145 `ANALOG`, 99 `MAXIM`, 85 `MICROCHIP`, 51 `MSP430`,
  47 `ANALOGD`, 44 `CM0_NXP`, 41 `ACTIVE`, 37 `LAPLACE`, 27 `DISPLAY`,
  25 `TRXD`, 25 `NATDAC`, 20 `PICCOLO`, 18 `TEXAS`, 17 `L_CMOS`,
  16 `IRPOWER`, 15 `CM3_ATMEL`, 14 `I2CIO`, 12 `OPTO`, 12 `MOTORS`,
  11 `STELLARIS`, 11 `LTDAC`, 10 `MEMORY`, 9 `PICAXE`, 9 `PIC16`,
  9 `CM3_NXP`, 8 `CM3_STM32`, 5 `NXP`, 4 `PROSPICE`, 3 `ONSEMI`,
  3 `BURRBROWN`, 2 `FAIRCHLD`, 1 `CM0_GENERIC`).
  1091/1097 decks start with a `LISA MODEL DESCRIPTION FORMAT` title.
- Drive-by fix in `corpus_check`: the `lml` arm was dead code (it
  checked the same `MODEL LIBRARY\x1a` magic as the earlier `model-lib`
  arm, so LML files classified as `model-lib` and `lml` never matched).
  LML files now classify as `lml`, and `lml`+`sml` count as
  covered-by-parsers: `MODELS/` goes from 43/291 (14.7%) to
  79/291 (27.1%) — 33 `lml` + 36 `sml` + 10 `mdf`.

### ngspice smoke test: LM123 (verified 2026-10-22, P3)

- First real batch simulation of a Proteus deck, no GUI: `ngspice-47`
  (`/usr/bin/ngspice`) ran the `ANALOG.SML` → `LM123` deck verbatim
  (only the trailing NUL stripped). `.MODEL xx RES` lines needed no shim.
- Pin mapping (inferred from the deck, confirmed empirically):
  `.SUBCKT LM123 3 1 2` = **3:IN, 1:OUT, 2:GND** — node 2 is the common
  reference of every internal source, and pin-1 current (via `V2`/`F3`)
  is mirrored as supply current drawn from pin 3 (series-regulator
  topology). Testbench (`docs/ngspice-lm123.cir`): `V1 in 0 DC 8`,
  `Rload out 0 5` (≈1 A), `X1 in out 0 LM123`, `.op` + `.dc V1 5 12`.
- Deck extraction (no python re-parse of the container — `pmodel::sml`):
  list with `cargo run -q -p pmodel --example sml_dump -- ANALOG.SML`,
  then dump the deck with a one-off using the crate, e.g.
  `/tmp/lm123x/src/main.rs` with `pmodel = { path = "<repo>/crates/pmodel" }`:
  `for d in pmodel::sml::split(&data).unwrap() { if d.name == "LM123"
  { print!("{}", d.text.replace('\0', "")); } }` → save as `lm123.mod`
  next to the `.cir` (`.include lm123.mod`), run from that dir:
  `ngspice -b -o out.log ngspice-lm123.cir`.
- Result: **converged, exit 0, zero errors/warnings.** `.op` @ Vin=8 V:
  `v(out)=5.0998 V`, `i(V1)=-1.0319 A` (1.020 A load + ≈12 mA quiescent).
  `.dc` sweep: dropout below ≈6.7 V, flat **≈5.10 V** from 7→12 V
  (line regulation ≈1 mV/V). The model is a 5 V / 3 A series-regulator
  macromodel behaving to spec; dropout ≈1.6 V.
