# Reverse engineering notes (interoperability only)

Facts observed from Proteus 8.6 SP2 (wine prefix + shipped samples).
No Labcenter binaries or data are reproduced here.

## Layout

- `BIN/PDS.EXE` (PE32, 1.5 MB): single launcher, `ISIS.DLL`/`ARES.DLL` inside.
- `BIN/`: ~70 VSM model DLLs (`AVR`, `PIC16/18`, `ARM7TDMI`, `LPC2000`...),
  `PROSPICE`, Qt4 + python35 + lua runtimes, `Tools/` (MPASM, AVRDUDE...).
- `LIBRARY/`: ~130 `.IDX`+`.LIB` pairs + 7 lone `.LIB` + ~20 `.VML` +
  `PROPDEFS.INI`, `PWRRAILS.INI` (plaintext, property vocabulary).
  `PWRRAILS.INI` bindings: `VCC=VCC/VDD`, `VDD=VCC/VDD`,
  `VSS=GND`, `GND=GND`, `VEE=VEE` — rail equivalence classes
  (VCC≡VDD 5V, GND≡VSS, VEE −5V); ERC `Power rails interconnected`
  fires only ACROSS classes (same-class = default binding, OK).
- `MODELS/`: 10× `.MDF`, 37× `.SML`, 35× `.LML`, ~150 VSM `.DLL`.
- `SAMPLES/`: mostly `.pdsprj`, 23 legacy `.DSN`, `.LYT`, `.SDF`, firmware sources.
- `Translations/`: 29× Qt `.qm`. `HELP/`: 54× `.chm` (docs, `VSMSDK.chm` most
  relevant for VSM DLL interface — not yet read).

## Frontera verificada (no-SPICE)

- `OA_BIP` (ANALOG.LML, 5106 B) = MAPPINGS de parámetros (`GAIN/CMRR/FT/SR/IB...` por tipo 709/741/LM324...) + PARTLIST de primitivas DSIM (`B`), NO subcircuito SPICE. `U1,CA3140` → `MODFILE=OA_BIP` + `PACKAGE=DIL08`: las funciones de pin (in+/in-/out) viven en el modelo DSIM compilado, no en la librería. Conclusión: `U`/`X` exigen kernel DSIM interactivo (roadmap mayor), no backend ngspice.

## Roadmap sim (diseño concreto)

- `X` (p.ej. `U1,CA3140` en Ppsu): el SDF trae `MODELDEFS` vacío; el vendor resuelve por librería (`NETLIST::resolve`). Camino nativo: 1) `pmodel::spice::parse_many` sobre el deck LML (hecho); 2) mapa `kind->Subckt`; 3) `asim` emite `Xref n* DECK` + re-emite el deck (falta emitter `Subckt->texto` + aplanar pines por `PACKAGE`); 4) U/CPU con firmware (HEX via LOADERS) quedan en DSIM, fuera de ngspice.
- `M` (MOSFET): `M d g s b modelo [L= W=]` + `.MODEL NMOS/PMOS` (ngspice trivial, falta mapear `L=/W=/AD/AS`).

## Ghidra findings (REA + Ghidra 12.1.2)

Decompiled & applied (loop ledger):

- [x] `NETLIST::net_gen` 0x1000fa00 — dispatch tabla 12B, 6 formatos
- [x] `NETLIST::parse_models` 0x1bb90 — loop preámbulo `:`
- [x] `NETLIST::write` (por nombre) — orden canónico, backpatch, DSN_
- [x] `NETLIST::read_parts` — tolerancia VALUE, `=`/`,`
- [x] `NETLIST::read_nets` — addpin enum, `sdf_etstrings` + remapeo IP
- [x] `NETLIST::read_script` 0x10018b60 — getline hasta `*ENDSCRIPT`
- [x] `NETLIST::validate` — rails interconectados, hide-common-pins
- [x] `NETLIST::net_erc` — UNDRIVEN / SINGULAR LABEL / connected to
- [x] `NETLIST::resolve` — MODEL/MODFILE + CDBCORE walk
- [x] `DSIM::dsim_create` — firma, vtables, lookup primitiva
- [x] simulador nativo: SPICESIM.DLL es fork SPICE3F5 (paths
  `.\SPICE3F5\SPARSE\SP*.C`, `?spice_initialize(SDF_NETLIST*,
  SIMLOG*, ISPICE...)`) — nuestro backend ngspice es su descendiente
directo, elección validada. SPICEINP.DLL parsea tarjetas
  (`.tran/.ic/.nodeset`, `unknown device type - error`).
- [x] packers de librería: PUTMDF.EXE/PUTSPICE.EXE
  (`-C[=n] create new library`, `-L=<libname>`, exigen `.ENDS`).
  GETSPICE: `-D` borra modelos del lib, `-A` extrae todo,
  `-F=<modname>` salida, `-L=<lib>` (default `PROSPICE.SML`);
  PUTSPICE: `-D` borra del disco, `-C[=n]` crea.
  TOKENIZER.DLL = tokenizador BASIC (`WHILE/UNTIL`, `Nothing to
  tokenize`) para FLOWCHART/DIYSTAMP.
- [x] `createdsimmodel` 8155H.dll — factory NULL-si-no-match.
- [x] firmware/mecánico: LOADERS.DLL carga HEX/ELF/COFF + list files
  (VSM CPUs); `BIN/citf/` importa 3D STEP/IGES (OpenCASCADE TK*).
  DIYSTAMP.EXE es compilador BASIC de terceros (no Proteus).
- [x] cadena de conversión: DSNCVT40 = ISIS IV + `ISIS SECTION V3.5` vía `ISIS_3X4X_DSNCVT.DLL` (3.x/4.x); IDSCVT40 migra los 4 kinds (DEVICE/PACKAGE/MODEL/SYMBOL) con chequeo de versión.
- [x] misc: EASYHDL.DLL compila programas MCU (`HDLMODEL`, CRC);
  IDSCVT40.DLL convierte librerías viejas; FLOWCHART/VIEWDATA son UI Qt.
- [x] modelos VSM (212 DLLs en MODELS/): 162 exponen `createdsimmodel`
  (primitiva DSIM) y 34 `createactivemodel`/`deleteactivemodel`
  (animados: BLDCanim, VGPS expone `IACTIVEMODEL`/`IDSIMMODEL`).
  Nuestro `vsm-host` carga ambas familias por nombre.
- [x] VSM activos (VTURTLE/DIPSW/CTIMER/COLORLED): `IACTIVEMODEL` +
  Qt `QPainter`/`QPaintDevice` para animación; SPICE expone `ISPICEMODEL`.
  Nuestro `vsm`/`nullmodel` espejan la interfaz sin Qt.
- [x] misc2: APPFRAME.DLL = marco app Qt (`APPLNERRORHANDLER`, page dims); INTERNET.DLL = updater (`QNetworkReply`, SSL); LIBMMD.DLL = catálogos i18n; VSMSTUDIO.DLL = IDE (`%PROTEUS_DIR%`, help XML). EXPREP/IMPORTER/PRIMS/PROJNOTES/STARTUP/TEXTEDIT/WINCORE/WINDIALOG/XLIB no existen en disco (nombres históricos).
- [x] VGDVC.DLL = viewport OpenGL (`SGDVC/GLDVC`, `tesserror`, `getopenglversion`).
- [x] SPICEINP catálogo de errores vendor (referencia, no espejado: ngspice emite los suyos): `.ic/.nodeset syntax error`, `unknown option/parameter on .tran - ignored`, `no unlabeled parameter permitted on mosfet`, `unable to find definition of model %s - default assumed`, `unimplemented control card`, `unknown device type - error`.
- [x] GVIEW.dll = visor Gerber/drill + salida CADCAM (`VIPKGVIEWER`, `DRILL_SYMBOL/SPAN`, `$DRILL%02d`, chequeo footprint sin asignar).
- [x] ARES verificación/salida: DRC (`RST: ARES\DRC`, `Checking...`,
  `%d DRC errors`, `DRC disabled`, columnas `Design Rule/Violation
  Type/Layer(s)/Spec'd/Actual Clearance`); `Design Rule Manager`
  (`ATB_LAYOUT:8`); `Pre-Production Check` (chequeos de manufactura +
  ignorar/resetear errores); conectividad (`Connectivity
  Errors` + `Connectivity Report`, tabla `Error Type/1st Pin/2nd Pin/Missing/Extra`);
  Gerber X2 default (+selector RS274X, exige stackup válido) + netlist
  `P  VER IPC-D-356A`; drill Excellon (`NC Drill Setup`,
  `Drill, %s, %u-%u` + flag `Unplated`, `$DRILL%02d`); stackup
  `Surface|Signal|Plane|Core|Prepreg`; zonas (`Zone: Net/Layer/Style`).
- [x] DSIM.DLL ABI completa (29 exports): ciclo `create/initialize/setup/bootstep/timestep/suspend/resume/terminate`, red `linknet/mergenets/connect(SDF_PINTYPE)`, pines `peek/poke`, conversores `createadcs/createdacs/linkadc/linkdac/postadc/polldac`, `getbuspin`, `linkprobe/linktape/writedata`, `signon(SIMLOG)`, `getdelay/getinitval`. Nuestro `vsm` cubre create-level; stepping completo = roadmap.
- [x] DSIM.DLL: interfaces `IDSIMCKT`/`IDSIMMODEL`/`IDSIMPIN1`/`IDSIMPIN2`
  (mixed-mode sim kernel, `DSIM creating net`, breakpoints). `asim` cubre
  el flujo batch; kernel interactivo = roadmap.
- [x] ELECTRA API C (6 exports): `electra_init/load/command/dofile/abort/sethost` — router scripteable por archivo (vía futura: generar dofile desde nuestro LYT). ISIS.DLL/ARES.DLL solo exponen `createmoduleinstance/deletemoduleinstance`.
- [x] ELECTRA.DLL: autorouter Qt (`Adaptive Shape-Based AutoRouter`,
  `AutoRouteRun/PauseAction`) — router propio = roadmap, formato LYT ya
  parseado (pads/sheets/CLASSES/SIGNAL).
- [x] CDBCORE constructores (`@0x1003860/0x1003800`): `addelement`
  (`getelement(sheet,uid)` si no `newelement`; UID en `+0xc`,
  `PINTEXTDATA`→`+0x80/84/88`, registro en `+0x304`) y `addmodule`
  (`newmodule`, nombre `strnzcpy 0x32` en `+0x8`, `+0x78`=param,
  `+0x7c`=UID, registro en `+0x31c`) — confirman al byte nuestro
  layout `pproj::cdb` (nombre+0x08, u32s+0x78/+0x7c, pines+0x80).
- [x] `CDBCORE` API + pin roles (strings ARES)
- [x] `NETLIST::read_models` (private, @0x10018a80) + `read_props`
  (@0x10018810) + `sortnets` (public, @0x100177f0) + `gettitle`
  (@0x1000e170) — decompilados Ghidra vía REA (ver notas abajo)
- [ ] `ISIS.DLL` factory loader — es GetProcAddress (sin función única);
  mecanismo documentado + espejado en `vsm-host` ✓
- [x] ISIS anotador: operación batch (`RST: ISIS\\Command` `Performing
  Annotation on Design...`); habilita back-annotation + pinswap
  automático; terminales de potencia anotados crean rails.
- [x] CONSOLE.EXE: verbos `simulate|dumptables` + `Usage: %s
  input_file output_file`, perfilado dinámico, constant propagation.
- [x] SPICEINP 25 tarjetas: `.model .width .print .plot .temp .op
  .nodeset .disto .noise .four/.fourier .ic .ac .pz .dc .tf .tran
  .subckt/.ends/.end .options/.option/.opt .sens .probe`
  (`.TEMP` obsoleta: usar `.options TEMP/TNOM).
- [x] editor de propiedades: `PROPDEFS.INI` = 23 defs con tipos
  (`INDEXED STRING`/`STRING`/`YESNO`/`FLOAT`/`PACKAGE,0`/
  `READONLY`/`HIDDEN`/`HIDDEN TRACEMODE`); diálogo `=PROPSTEXT`/`=PROPS`
  (ver `docs/uispecs.md`). .DTF = variante DSN (dsn_dump las abre, 0 wires: hojas vacías); .pjnt = XHTML.
- [x] PACKAGE (9) y SYMBOL (2) libs: mismo DIR stride-80/nombres-64 (CONNECTORS.LIB: 230 entradas, parse_lib_dir sirve sin cambios) + cuerpos 02-frames/braces; .STY = estilos pad ASCII; .VML = modelos 3D binarios.
- [x] cuerpos .LIB: mismo framing 02 xx + scripts {PREFIX=}/{PINDEFAULT}/{PROPDEFS}/{PROGRAM}/{MODDLL} (plib device_props, 8 KiB window) - link device->package->modelo; aengine lo expone en Pick Device.
- [x] pen block = constante por template (`34...00 02 22 00 00 00 00 60 4f 00 00 00 00 c0 00 01 ff ff ff 00 ff ff ff 00` idéntico en todos los COMPONENT) — el style id basta para render; strokes = orden de vértices (polilínea implícita, TBD verificar).
- [x] pen styles (siggen.DSN, `Symbol.style`): COMPONENT/GENERATOR=0x22, TERMINAL=0x21, MARKER/VPROBE=0x1f, PORT/TAPE=0x1d, WIRE grueso=0x23; style=0 en records con header bbox-first (pen default o en otro offset).
- [x] símbolos: directorio `02 7F/67/7B/7D/7E` (321 records: 298 refs +
  23 instancias, 12 templates incl. `2D GRAPHIC`, 1799 vértices) —
  `plib::symbols` generalizado a todos los templates; familias
  `02 22` (primitiva) vs `02 1B-23` (markers/pins/ports); ref `7F`
  vs instancia `67/7B/7D/7E`. Strokes/pen aún TBD.
- [x] pads LYT — records `02 7F ESTILO` + ASCII pin (Cpu.LYT @0x14e8);
  tail post-SIGNAL esparso (mayoría ceros, sin tracks ruteados en sample);
  trío 780/975mm = tamaño hoja. Tracks/vías: encoding TBD (probablemente
  sample sin rutear).
- [x] tracks `02 60 <grupo:3B> <x:i32> ...` + net ASCII `06 23 <num>`
  (PICDEM LCD 2_r: 40 verts en 29 grupos encadenados; Cpur: 1 segmento
  `$WR8`; `02 40` = vía + `02 44` pin-ref). Y siempre 0 en samples —
  encoding del eje Y TBD (experimento ARES pendiente). `plib::lyt_tracks`
  une vértices consecutivos del mismo grupo; `02 41` = nets del layout.
- [x] configs: `proteus.ini` (42 B, `[Settings] Program=...`);
  `STARTUP.ini` = HTML del update manager (anuncio Proteus 9).
- [x] DSO.DLL = osciloscopio Win32 (`DialogBoxParamA`, `DisplayColorChannel%i`,
  `createactivemodel`) — instrumento con UI nativa, no Qt.
- [x] instrumentos/comms: `SPEAKER.DLL` = `IACTIVEMODEL` + WinMM
  (`waveOutOpen/Write/...`) — playback real; `VGPS.dll` habla
  NMEA+MTK (`GPGGA/GPRMC/PMTK`, baud/fix interval);
  `COMPIM.dll` = puente COM físico/virtual (RS232).
- [x] RTC (14 DLLs, 7 familias): DS12887 paralelo, DS1302 3-wire,
  DS1307/1337/1338/1340 + DS3232/3234 + PCF2123/8523/8563/8583 +
  MCP7941X + MAX6902 (I2C/SPI, BCD, VBAT, alarmas); ABI DSIM.
- [x] displays (`DISPLAY.LML`): LCDALPHA=HD44780, LCDMPX, LCDPIXEL,
  COLORLED, ColorLcd (PRIMITIVE DIGITAL/ANALOG, no Qt).
- [x] memorias: 4 DLLs genéricas (`I2CMEM` 24LCxx, `SPIMEM` 25LC/M25P,
  `UNIOMEM` 11LC/93LC, `MMC` SD-SPI; +`IDT720X` FIFO, `PCF8570`, `ATA` IDE).
- [x] entrada: `KEYPAD.DLL` = active model que captura teclado PC
  (`VkKeyScanA`); matrices en `ACTIVE.LML`.
- [x] sensores (83 DLLs): BMP180/DHTXX/DS182X + HYT271/SHT2X/TH02
  (hum), DS1620/LM75/TC7X/MCP980X (temp), MAX31855/66745 (termopar),
  SRF04 (ultrasonido), INA219 (potencia); patrón DSIM.
- [x] ADC (20+ DLLs): `createmixedmodel` (ADC083X, LTC24XX) vs DSIM
  puro (MCP300X, TLC254X, MAX12X, AD1674); analógico vía `MODFILE` +
  `ITFMOD` (TTL/TLC1543ITF/ADS7824ITF...).
  BLDC*, DCFAN, DCMOTOR, SERVO, STEPPER...; sondas `RTVPROBE
  RPM/ANGLE/TORQUE/SPEED`, params `STEP/RPM/OMEGA/LCOIL/VNOM/KTORQUE`);
  `ROTOR.DLL`/`BLDCanim.dll` animación mecánica.
- [x] iconos UI reales: ISIS.DLL `.rsrc` trae 11 grupos RT_GROUP_ICON
  (33 imágenes RT_ICON 16–48px: chips, transistor, probes, R/C/D,
  gráficas con ejes, filtros); extraídos a `crates/aui/assets/`
  (parser PE propio: sección `.rsrc`, dir tipo 14→3). Corrige claim
  anterior: los iconos NO están dibujados solo en código.
- [x] UI declarativa: PDS.EXE/ISIS.DLL/ARES.DLL embeben specs UTF-16
  `=KEY,tipo,...` (534 ISIS + 459 ARES + 114 PDS: `=MENU`, `=PREVIEW`,
  `=ADDPIN`, `=ANGLE`, `=COLOUR`, `=LAYER`...) — ver `docs/uispecs.md`.
  Es el spec completo de diálogos/menús para clonar UX en `aui`.

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
- `NETLIST::addmodule` (`?addmodule@NETLIST@@QAEPAUNET_MODULE@@PADAAVPROPLIST@@PAUMODULE@@@Z`,
  public, @0x10013800): crea `NET_MODULE` (`new(0x68)` +
  `NRPROPLIST` en `+0x44`), copia nombre `MODULE`; solo si `+0x1c`.
- `NETLIST::propkwd` (`?propkwd@NETLIST@@QAEPBDH@Z`, public, @0x1000e1a0):
  `return keywords[this+0x74][i]` — tabla de vocabulario PROPDEFS
  (`PROPDEFS.INI`: PACKAGE, VALUE, MODFILE...).
  `propval` (`?propval@NETLIST@@QAEPBDH@Z`, @0x1000e1b0) devuelve el
  puntero PASADO el NUL: la tabla son pares `CLAVE\0valor\0`.
- `NETLIST::propsubst` (`?propsubst@NETLIST@@QAEXPADAAVPROPLIST@@@Z`,
  public, @0x10013ee0): sustitución de parámetros en valores con
  `EVALUATOR<double>` (buffers 1K) + límite de anidado
  (`Excessive nesting of parameter definitions for %s in %s`).
  Nuestro `normalize_value` cubre infijos (`9k1`), no fórmulas.
- `NETLIST::fmtdprop` (`?fmtdprop@NETLIST@@QAAXPBD0ZZ`, public,
  @0x100129d0): `vsprintf` + `PROPLIST::add(proplist+100)` — setter
  formateado de props `DSN_*` (lo usa `setdsnprops`).
- `format_propdefs` (`?format_propdefs@@YAXAAVPROPTEXT@@AAV?$GDIDX@VPROPDEF@@@@H@Z`,
  free, @0x1001f7e0): emite la sección `*PROPDEFS` desde
  `PROPTEXT` + `GDIDX<PROPDEF>` (menciona `COMPONENT`/`PROPDEFS`).
  Cierra el lado escritura: `write` → `net_gen` → `fmtdprop`.
- `NETLIST::parse_field` (`?parse_field@...`, public, @0x1001bcc0):
  parser de entradas `*PARTLIST`: por parte `CDBCORE::findpart` +
  `getpartelements` + `addelement`, con rama `TERMINAL`.
- `NETLIST::bom`/`dxp` (`?bom/dxp@NETLIST@@QAEHXZ`, @0x1000e050/
  0x1000e040): NO generan nada — son getters de bits de opción
  (`(flags+0x28>>5)&1` / `(>>4)&1`). La salida BOM va por `net_gen`.
- `NETLIST::setdsnstats` (`?setdsnstats@NETLIST@@QAEXIIIPAUCDBWALKQ@@PAVPROPLIST@@@Z`,
  public, @0x10012900): flags `+0x24/+0x28/+0x30` (+0x28 bit4/5 =
  `dxp()`/`bom()`), `CDBWALKQ` en `+0x1c`, `PROPLIST` en `+0x3c`.
- `NETLIST::setshtstats` (`?setshtstats@NETLIST@@QAEXPAUSHEET@@HH@Z`,
  public, @0x10012930): hoja vía tabla `+0xf0` (índice `*(sheet+8)`),
  módulo en `+0x19c`, flags `+0x34`, `+0x38`; vacía SLIST `+0x1a0`
  (staging de netprops).
- `NETLIST::getpwrnets` (`?getpwrnets@NETLIST@@QAEXAAV?$GDPA@UNET@@@@@Z`,
  public, @0x10015f00): recorre array de nets (`+0x184`, count
  `+0x180`); por net escanea pines (stride `0x48`) buscando
  `ELECTYPES==0xd` (OP) → `DPA::add`. Vendor define power-net por
  ROL (pines OP de terminales POWER), no por nombre — nuestro
  `validate` usa heurística de nombres (`PWRRAILS.INI`); equivalencia
  documentada, no identidad.
- `NETLIST::addmodel` (`?addmodel@NETLIST@@QAEXPAD0@Z`,
  @0x1000e0f0): tail-call `PROPLIST::add(this+0x9c)` — lista MODELDEFS.
- `NETLIST::addmodelinstance` (`?addmodelinstance@...`, @0x10012d90):
  crea `NET_MODULE` desde `NET_PART` (link `+0x3c`, copia nombre).
- `NETLIST::addmodfile` (`?addmodfile@NETLIST@@QAEXPAD@Z`, @0x1000e100):
  `strdup` + `DIDX::add(+0xc0)` — lista de MODFILE paths.
- `NETLIST::applymapping` (`?applymapping@NETLIST@@AAEXPAUNET_PMT@@PAUNET_MODULE@@@Z`,
  private, @0x10015980): runtime de `*MAPPINGS` — `getselector` +
  `PROPLIST[]` con fallback `DEFAULT`.
- `NETLIST::nodegen` (`?nodegen@NETLIST@@QAEXXZ`, public, @0x10017690):
  numeración de nodos sobre array `+0x198/+0x194` (mismo que `sortnets`),
  contadores de pin `+0xcc/+0xd0`.
- `NETLIST::netmerge` (`?netmerge@NETLIST@@QAEXPAUNET@@0@Z`, public,
  @0x100154f0): fusiona dos NETs (`realloc`+`memcpy`: pines stride
  `0x48` en `+0x4`/count `+0x10`, segundo array stride `0x66` en
  `+0x8`/count `+0x14`) — confirma tamaño de pin 0x48 de `getpwrnets`.
- `NETLIST::getnet` (`?getnet@NETLIST@@QAEPAUNET@@PBD@Z`, public,
  @0x10015c50): NULL→`""`, `#N` numérico o corta en `=`; scan lineal
  del array `+0x198/+0x194`.
- `NETLIST::getnetprop` (`?getnetprop@NETLIST@@QAEPBDPAUNET@@PBD@Z`,
  public, @0x10015f60): escanea props de net (array stride `0x66`
  en `+0x8`, count `+0x14`, `_stricmp`); valor en `+0x33`. Confirma
  que el 2º array de `netmerge` son props `CLAVE\0...valor`.
- `NETLIST::addmaptable` (`?addmaptable@NETLIST@@QAEXXZ`, public,
  @0x10012ba0): vuelca `NET_PMT` pendiente (`+0x1a8`): módulo raíz
  → `DPA::add(+0x88)`, si no `applymapping(módulo)` + libera.
  Cierra `parse_map` → `addmaptable` → `applymapping`.
- `*bounds` (`netbounds` @0x1000e2b0 y familia `part/prop/script/trace`):
  `0<=i<count` → 1/0 (verificado el patrón en `netbounds` vs count
  `+0x194`; el resto es la misma forma).
- `NETLIST::netcleanup` (`?netcleanup@NETLIST@@QAEXPAUNET@@HHH@Z`,
  public, @0x10015690): compacta/dedup array de pines (`+0x4`, count
  `+0x10`), re-enlaza pin→net, `NETLOG::logerror` si hay.
- `NETLIST::pathgen` (`?pathgen@NETLIST@@QAEPADPAUNET_MODULE@@H@Z`,
  public, @0x10012e40): recorre cadena de módulos (`+0x38`), genera
  path jerárquico (llama `setcdbroot/link/addmodule/addmodelinstance`).
- `NETLIST::addmapitem` (`?addmapitem@NETLIST@@QAEXPAD0@Z`, public,
  @0x10012b40): agrega a la tabla MAP pendiente (`+0x1a8+0x40`);
  duplicado → `Duplicate entry '%s' found in MAP ON %s table`.
- `CDBCORE::getmodule` (`?getmodule@CDBCORE@@QAEPAUMODULE@@K@Z`, @0x10001030):
  índice 1-based a array de `MODULE*` (`+0x250`, count `+0x24c`).
- `NETLIST::numberpins` (`?numberpins@NETLIST@@QAEHXZ`, public,
  @0x10016050): cuenta pines con nombre (stride `0x48` en `+0xc`,
  count `+0x18`, nombre no vacío) — segundo array de pines además
  del de `getpwrnets`/`netmerge`.
- Flags `+0x28` mapeados: bit2=`graph()` (@0x1000e020),
  bit3=`realtime()` (@0x1000e030), bit4=`dxp()` (@0x1000e040),
  bit5=`bom()` (@0x1000e050) — seteados por `setdsnstats`.
- `NETLIST::getscript` (`?getscript@NETLIST@@QAEPBDPBD@Z`, public,
  @0x1000e220): `DIDX[+0x140]` por nombre → texto en `+0x6c`
  (indirecto si tamaño `+0x80`>0xf) — tabla de scripts.
- `NETLIST::setmodstats` (`?setmodstats@NETLIST@@QAEXPAUNET_MODULE@@HH@Z`,
  public, @0x100129a0): módulo actual `+0x19c`, flags `+0x34`,
  vacía SLIST `+0x1a0`.
- Familia d/s-prop (`DSN_*` vs netprops): `adddprop`
  (`?adddprop@...`, @0x1000e0b0) = `PROPLIST::add(this+0x64)`;
  `repdprop` (@0x1000e0c0) = `remove`+`add` (reemplazo);
  `addsprop` (`?addsprop@...`, @0x10012a40) = nodo `new(0x66)`
  clave+0/valor+0x33 → SLIST `+0x1a0` (igual que `parse_netprop`).
- `addproductkey`/`addsamplekey` (`?add*@NETLIST@@QAEXK@Z`, @0x1001a700/
  0x1001a6e0): agregan u32 a tablas fijas (PRODUCT `+0x244`/count
  `+0x284`, SAMPLE `+0x200`/count `+0x240`, máx 0x10) — las que
  llena `parse_requires`.
- `NETLIST::addpwritf` (`?addpwritf@...`, @0x10013910): lee props
  `ITFMOD/MODDATA/MODEL/MODFILE/POWER/PRIMITIVE/TRACE/VALUE`;
  gated por flag `+0x24&4` y NO-`dxp` (`+0x28&0x10`) — es el worker
  del path DXP.
- `NETLIST::partssize` (`?partssize@NETLIST@@QAEHXZ`, @0x1000e290):
  `return count(+0x10c)` — tamaño del array de partes (`nodegen`
  itera `+0x10c/+0x110`).
- PSM (power-state pass, privados): `psm_crc` (@0x1001afd0) = CRC32
  sobre la tabla PROPDEFS (`+0x74`, count `+0x70`); `psm_netprop`
  (@0x1001aa60) busca roles OP(0xd)/IP(0xe) en pines y asigna estado
  de net; `psm_partprop` (@0x1001a8a0) transiciona estado de parte
  (`+0xdc/+0xe8`).
- `NETLIST::addmoddata` (`?addmoddata@NETLIST@@QAEHPAUNET_PART@@PAUMODDATA@@@Z`,
  public, @0x10013bd0): worker MODDATA con el mismo gate que `addpwritf`
  (`+0x24&4`, no-`dxp`) — path DXP por parte.
- `NETLIST::getselector` (`?getselector@NETLIST@@AAEHQADAAVPROPLIST@@0@Z`,
  private, @0x10015ad0): parsea nombres separados por `+`
  (`getnzstr(0x3f)`); error `Selection property name(s) expected in MAP
  statement` — el selector de `MAP ON` (`parse_map`).
- `NETLIST::getsheetid` (`?getsheetid@NETLIST@@QAEKXZ`, public,
  @0x1000e310): `**(SHEET**)this ?: 0` — id de la hoja actual
  (la que fija `setshtstats` en `+0x0`).
- `NETLIST::parse_map` (`?parse_map@NETLIST@@QAEXAAV?$basic_istream...`,
  public, @0x1001b9b0): `MAP ON <nombre>` → `newmaptable`; líneas
  hasta `*` (`getpropname(':')`); error `Property name(s) expected
  after MAP ON keyword`. Nuestro `Section.extra` guarda el nombre
  (`*MAPPINGS,2,COMPARATOR`).
- `NETLIST::parse_netprop` (`?parse_netprop@...`, public, @0x1001bf50):
  líneas `CLAVE=VALOR`; nodo `new(0x66)` + `strnzcpy` → SLIST en
  `this+0x1a0`; `seekg` atrás al topar `*`.
- `NETLIST::parse_requires` (`?parse_requires@...`, public, @0x1001c0c0):
  `chktok SAMPLE=`/`PRODUCT=` + `skippast('=')` + `gethex` → tabla
  en `this+0x284` (máx 0x10 entradas).
- `NETLIST::addelement` (`?addelement@NETLIST@@QAEPAUNET_PART@@PAD0AAVPROPLIST@@PAVOBJECT@@K@Z`,
  public, @0x10012f70): crea `NET_PART` (`new(0x124)`), `PARTLIST::find`,
  resuelve `PRIMITIVE`/`TERMINAL`, valida vs CDB por UID.
  Errores: `Duplicate part reference`, `Conflicting library parts /
  property values ... in multi-element device`, `No CDB element/part
  record ... UID=%08X`.
- `NETLIST::addpin` ×2: string (`?addpin@NETLIST@@QAEXPAD0PAY0BP@DW4ELECTYPES@@H@Z`,
  @0x100147b0) copia ref, corta sufijo `:xxx` vía `strrchr(':')`
  (si flag `+0x24` o `ELECTYPES` 9..13), `PARTLIST::find(módulo)` y
  delega a `addpin(NET_PART*,...)` (@0x10014860). Confirma enum
  `ELECTYPES` + roles con `:` de `read_nets`.
  Interna (@0x10014860): `getbusrange()` (rangos `D[0..7]`), clamp
  `ELECTYPES` 0..0x10 (17 roles = `sdf_etstrings`), sub-partes
  `REF#n` vía PINSWAP (`getnzstr(",;")` + `_stricmp`), SLIST `+0x1ac`.
- `NETLIST::addlabel` (`?addlabel@NETLIST@@QAEXPAUNET_MODULE@@PAVOBJECT@@PBDW4ELECTYPES@@@Z`,
  public, @0x10014480): label vacío → return; `ELECTYPES 0x11`
  con flag; `8` → `mergebusrange_lbc(+0x1e0/+0x1e4)`, resto →
  `mergebusrange_bar(+0x1e8/+0x1ec)`; sucio `+0x1dc`; `!x` = ref de
  módulo, `&x` = submódulo.
- `NETLIST::newnet` (`?newnet@NETLIST@@QAEXXZ`, public, @0x10014440):
  resetea 9 dwords de staging (`+0x1cc..+0x1ec`: contador `+0x1d8`,
  sucio `+0x1dc` — mismo bloque que `addwire`).
- `NETLIST::addwire` (`?addwire@NETLIST@@QAEXPAVWIRE@@H@Z`, public,
  @0x10014b40): `WIRE*` → SLIST en `this+0x1c4`, contador `+0x1d8`;
  con flag!=0 solo cuenta + marca sucio `+0x1dc` (modo merge).
- `NETLIST::parse_define` (`?parse_define@NETLIST@@QAEXAAV?$basic_istream...`,
  public, @0x1001b850): loop `getline` hasta `*`; `getpropname('=')` +
  `skipspaces` → `PROPLIST[this+100]` si `atrootlevel()` + además
  `PROPLIST[modulo_actual+0x44]`; al topar `*` hace `seekg` atrás para
  que el dispatcher vea el terminador. (En SDF de texto no hay
  `*DEFINE` — vive en DSN binario.)
- `NETLIST::read_models` (`?read_models@NETLIST@@AAEXAAV?$basic_istream...`,
  @0x10018a80): `skippast(',')` + `getint(count)`; loop `getline(0x2000)`
  + `getpropname(':')` + `skipspaces` + `PROPLIST::add(this+0x9c)`.
  Confirma MODELDEFS = count-first + `Clave: valor` (nuestro `pmodel`).
- `NETLIST::sortnets` (`?sortnets@NETLIST@@QAEXXZ`, @0x100177f0):
  `qsort(net_ptrs[this+0x198], count[this+0x194], 4, cmp@0x10017810)`.
  Vendor ordena nets; nuestro `emit` preserva orden de archivo
  (diferencia documentada, sin impacto funcional).
- `NETLIST::gettitle` (`?gettitle@NETLIST@@QAEPBDXZ`, @0x1000e170):
  `PROPLIST[this+0x64]["DSN_TITLE"]` — el título vive en las props
  `DSN_*` que `write()` excluye del SDF.
- `NETLIST::setdsnprops` (`?setdsnprops@NETLIST@@QAEXPBD00000@Z`, public,
  @0x10012860): 6 args NULL→`"<NONE>"` → `PROPLIST[this+0x64]` con
  `DSN_TITLE/DOCNO/AUTHOR/REV/MDATE/CDATE`. Cierra el círculo:
  `gettitle` lee `DSN_TITLE` de ahí y `write()` las excluye del SDF.
- `NETLIST::parse_models` (`?parse_models@...`, 0x1bb90, 368 B,
  Ghidra verifica nombre+body): lee líneas hasta EOF o línea que empiece
  con `*`; `getpropname(ptr,buf,':')` + `skipspaces` →
  `PROPLIST::add(lista+0x9c, nombre, valor)`. Confirma el diseño de
  `pmodel` (preámbulo `Clave: valor`, secciones `*`).
- `NETLIST::write` (`?write@...`, decompilado por nombre via REA/Ghidra:
  `void __thiscall NETLIST::write(NETLIST*, ostream*, int)`)
  `*PROPERTIES,XXXXX` → `*MAPPINGS,` (SIN conteo) → `*MODELDEFS[,XXXXX]` →
  `*SCRIPT ` … `*ENDSCRIPT` → `*PARTLIST[,XXXXX]` → `*NETLIST[,XXXXX]`
  (backpatch `XXXXX`, `#%05d`); excluye props `DSN_*` (strncmp); separadores
  ` : ` y `, `. `pmodel::emit` replica todo (MAPPINGS bare, ENDSCRIPT,
  filtro DSN_*).
- `read_parts` (Ghidra por nombre): getline loop; `getpropname('=')` +
  `getstr` → `PROPLIST::add`; `VALUE` defaulteado si falta.
  `pmodel::partlist` replica la tolerancia (campos faltantes → "").
- `read_nets` (Ghidra por nombre): por línea `getpropname('=')` +
  `getnzstr("\\n,")`, luego `addpin(net, ref, extra, 0, EVar3-enum, 0)` y
  `addnet()` al final. `pmodel::netlist` preserva los roles como strings
  (`PS/GT/Q1/1/...`), equivalente sin tipar el enum.
- Tabla `sdf_etstrings` (NETLIST.dll @0x22f28, slots NUL + pad a 4B):
  `[NC MOD PR BT OT IT GT LBL PP PD PU TS IO OP IP PS DEFNET …]`
  (índices 0–16). Rol `0xe`=IP se remapea a `0xd`=OP y va a `addlabel`
  en vez de `addpin`. Roles <8 → pin, <0xf → net.
- `NETLIST::validate` (`?validate@NETLIST@@QAEXXZ`, `void __thiscall`,
  @0x100175b0): solo 2× `NETLOG::logerror` — hide-common-pins +
  `Power rails '%s' and '%s' interconnected`. `asim::validate` lo espeja.
- `NETLIST::getpwrnets` (`?getpwrnets@NETLIST@@QAEXAAV?$GDPA@UNET@@@@@Z`)
  enumera power-nets (base del `validate` de rails + default `POWER`).
- `NETLIST::{zap,reset}` (`?zap/reset@NETLIST@@QAEXXZ`): clear/reset.
- `mdf_id` (`?mdf_id@@3QBDB`): es DATO exportado (buffer char),
  no función — por eso no hay `parse`/decompile.
- Accessors (firmas mangled verificadas, semántica por nombre):
  `netptr(NET*,int)`/`scriptptr`/`partptr` (indexados),
  `addptr(BASE*)` (@0x1000e130: envuelve en nodo SLINK → SLIST `+0x1c4`,
  misma lista staging que `addwire`), `addnprop(char*,char*)`,
  `addmoddata(NET_PART*,MODDATA*)`, `setmodstats`,
  `getnet/getmodule/getscript/getnetprop/getpart/getselector/getsheetid`,
  `netbounds/partbounds/propbounds/scriptbounds/tracebounds` (bboxes),
  `traceptr/realtime/iscontext/iscurrentsheet/ismode` (estado sim).
  `iscontext(mask,mode)` (@0x1000dfe0): `(flags+0x24 & mask) &&
  (flags+0x28 == mode)` — predicado que combina ambas palabras.
  `traceptr` (@0x1000e300): indexa array `TRACEPROP*` en `+0x170`
  (DPA de `addtraceprop` en `+0x160`).
- `addscript` (`?addscript@...`, @0x10012c10: texto + posiciones `fpos`).
- `addtraceprop` (`?addtraceprop@...`, @0x10013e50: nodo `new(0x48)`,
  `PROPLIST::format`, `DPA::add` en `+0x160` — props de trace para
  VPROBE/IPROBE).
- `PINOUT::netgen` único método PINOUT en IAT:
  `?netgen@PINOUT@@QAEXAAVNETLIST@@PADPAUNET_PART@@AAVPROPTEXT@@PAUNET_MODULE@@@Z`.
- `net_gen` path: `tfngen` importado de `LXLCORE.DLL` (ptr `0x10022174`,
  calls `0x1000fa8a/0x100fab8`); `fopen` vía `MSVCR90` (ptr `0x10022398`);
  fallo → `PUSH 0xa002` en `0x1000fae3`.
- CDBCORE I/O: `loadsheet 0x10004670` / `savesheet 0x100047c0` /
  `loadbindings 0x10008dc0` / `savebindings 0x10009340` (+ `save/load`
  ya documentados). `MODULE`: `+0x08` nombre, `+0x78/+0x7c u32`,
  `+0x80` ptr nullable contado.
  lee props `MODEL`/`MODFILE` y resuelve contra la DB de modelos;
  usa `CDBCORE` en stack para el walk. Nuestra `asim` mapea `D`/`Q`/`M`
  a defaults ngspice con aviso (elementos `R/C/L/V/I/D/Q/M` simulan;
  `U`/`X` pendientes por macromodelos).
- Export table (515 exports; RVAs de parse manual NO verificadas —
  el parseo crudo desalinea `AddressOfFunctions`: Ghidra-por-nombre es la
  fuente de verdad). Nombres confirmados por string: `parse_field`,
  `parse_map`, `parse_netprop`, `parse_define`, `parse_requires`,
  `parse_script`, `read_mappings/models/nets/parts/props/script`,
  `resolve`, `net_erc`, `net_gen`, `bom`, `dxp`, `addscript`,
  `writeheader`, `propkwd/propsubst/propval`, `validate`, `sortnets`,
  `gettitle`, `write`, `setcdbroot`, `setdsnprops`.
- `net_gen` ×2: ostream `?net_gen@@YAXAAVNETLIST@@AAV?$basic_ostream@DU?$char_traits@D@std@@@std@@HPAD2@Z`
  @0x1000fa00 y path `?net_gen@@YAXAAVNETLIST@@PADH11@Z`
  (`void net_gen(NETLIST*,char*,int,char*,char*)`) @0x1000fc50: abre con
  `fopen(..,"w")` + `addext`, despacha por índice vía tabla `net_formats`
  + punteros `PTR_FUN_10030390[i]`, `error(0xa002)` si falla.
  Formatos extra: `FORMAT,CROCODILENETLIST,1` + `REALPCB.ADI`
  (familia Crocodile/RealPCB fuera de los 6).
- `writeheader(ostream*, "ELECTRICAL RULES CHECK", ...)` abre los reports
  (visto llamarse desde `net_erc` en 0x1000f46a).
- Mapping to native crates: `PROPDEF` ↔ `*PROPDEFS` vocabulary
  (`PROPDEFS.INI`); `NETLIST::write` ↔ `*PARTLIST`/`*NETLIST` in `pmodel`;
  `setcdbroot` ↔ `ROOT.CDB` in `pproj`.
- `CDBCORE::save/load` (`NETLIST.dll`, 0x100088a0/0x100083f0, 442/304 B)
  descompilados: I/O por `VFILE` virtual (`vfgetstr`/`vfputstr` para
  strings Pascal), conteos y luego módulos/hojas (`newmodule`). `pproj::cdb`
  v1: records = `u32 seq | PString name | u32 pin-count | 2N PStrings |
  footer` — el `u32` previo es **secuencia** (0x28,0x29,0x2A… por record),
  no longitud; tablas de pines son pares planos (U1 = 7 pares).

## VSM + PROSPICE (from extracted CHM docs + community + ISIS strings)

- ISIS loads models itself (`ISIS.DLL` strings): `LoadLibrary(%s)` →
  signature check → `GetProcAddress(createactivemodel)` → create →
  `authorize(ILICENCESERVER)` (`GLE=0x%08lX` = `GetLastError`);
  missing `deleteactivemodel` is only a warning. `vsm-host` mirrors
  exactly this sequence with `libloading`.
- `createdsimmodel` (`MODELS/8155H.dll`, 0x4520, Ghidra via REA):
  `undefined4* __cdecl createdsimmodel(u32, host_cb*)`: llama al host
  `(cb(0x80808080, 0x8f))`, compara el nombre pedido con `"8155H"`
  (memcmp 6 chars); si coincide hace `operator_new(0x3b8)` + constructor,
  si no devuelve NULL. `deletedsimmodel` co-exportado. Patrón exacto que
  espeja `vsm-host::Plugin::load` (versión + create + destroy), con ABI
  propia limpia en vez de C++ vendor.
- `dsim_create` (`DSIM.DLL`, decompilado por nombre via REA/Ghidra):
  `int __cdecl dsim_create(IINSTANCE*, char* prim, PROPLIST&,
  IDSIMMODEL*, void** out)`. Sin modelo: lookup por nombre de primitiva
  (`Unknown DSIM Primitive Model [%s]` si falta); con modelo: alloca
  `INSTANCE` (0x9c B), instala vtables (`INSTANCE`, `GDIDX<PIN>`,
  `GDIDX<BUSPIN>`, `GDPA<PINHANDLER>`, `EXTMODEL`), devuelve instancia
  en `*out`. Log `DSIM creating %s`. Nuestra ABI nativa
  (`atenea_model_create/step/destroy`) es recepción limpia, no clon binario.
- `MODDLL`, `GETSPICE`/`GETMDF`/`SDFGEN` son `BIN/*.EXE` externos, no imports
  de ISIS — ISIS recibe un path ya resuelto.
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
- SDFGEN.EXE (`Labcenter SDF netlist converter`):
  `SDFGEN <filename> <format>` (ej. `design.net MULTIWIRE`),
  convierte netlists ajenas a SDF (`*NETLIST,%d`, FORMAT 2.0→3.0).
  Solo 2 formatos de entrada: MULTIWIRE + TANGO.
- Red VSM: NICS.DLL (ENC28J60 SPI + RTL8019AS NE2000, primitiva
  DIGITAL; props NET_CARD/MAC_ADDR/CHECKSUM); no hay USB VSM
  (solo HELP + mecánico 3D).
- LYTCVT.DLL: `CONVERTER:` old-layout (`File version is %1.2f -
  cannot load!`, `Cannot rename existing .LYT`) — LYT es versionado;
  DSNCVT40.DLL: `ISIS IV Design Converter` (legacy DSN→v8).
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
| `.LYT/.LTF` | `PCB LAYOUT FILE\x1A`, bloques `{*VIEW3D}`, padstacks, capas | mixed | `plib::dsn` (Cpu.LYT: 43 bloques); pads = `02 7f ESTILO\\0` + payload i32 (misma familia que los records `COMPONENT` de DSN; `Cpu.LYT` `CONN-DIL16` @0x14e8 con `DEFAULT`/`S60` y pines ASCII); escala 2.54M verificada (pitch 0.1\"=254000 y row 0.3\"=762000 i32 LE junto a estilos) |
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
