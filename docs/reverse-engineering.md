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

## Dependencias (mapa vía `rizin ii`, guardado en `docs/imports/`)
- `ARES.DLL` (2081 imports): `LXLCORE` 392, `APPFRAME` 375,
  `QtGui4` 329, `PRIMS` 179, `WINDIALOG` 160, `VGDVC` 142,
  `NETLIST` 37, `electra` 6.
- `ISIS.DLL` (1776 imports): `APPFRAME` 343, `WINDIALOG` 269,
  `LXLCORE` 264, `PRIMS` 182, `NETLIST` 158, `VGDVC` 130.
- Conclusión: todo Proteus = `LXLCORE` (núcleo) + `PRIMS`
  (primitivas) + `VGDVC` (render) + `APPFRAME`/`WINDIALOG` (shell)
  + `NETLIST` (motor netlist, compartido ISIS+ARES) + `DSIM`
  (sim) + `ELECTRA` (router). Nuestro RE de NETLIST cubre el
  netlisting de ambas apps.
- `varianttomask(c)` (@0x1000a710): `'b'→0`, si no
  `1<<((c-0x41)&0x1f)` (letra de variante → bitmask).
- `GPAVIEWER::autosize` (@0x1000af30): sizing Win32 del viewer
  (`GetWindowRect/SetWindowPos`).
- Data (sin código): `net_nc`, `sdf_pintypes`, `library_locale`,
  `library_paths/module_paths/template_paths`, enums
  `CDBSYNCMODES/TRACEMODEFLAGS` — documentados por uso.
  Validador de imports ISIS→NETLIST: 0 gaps de código.
- `CDBCORE::newdesign` (@0x100030c0): resetea barriendo hojas del
  módulo raíz (`+0x300`, tabla `+0x270`).
- `CDBCORE::cleartags` (@0x10006280): pone a 0 los tag-fields en
  tabla módulos (`+0x290`, campo `+0x98`) y tabla `+0x2d0`
  (campo `+0x138`) — cierra los gaps del validador de imports.
- `NETLIST::addnet` (@0x10014b90): crea el NET en curso
  (3 `operator_new` + append SLIST + `NETLOG`).
- `NETLIST::netptr(i)/scriptptr(i)` (@0x1000e2d0/@0x1000e210):
  `tabla_nets+0x198[i]` / `tabla_scripts+0x150[i]`.
- `getcdb()` (@0x10002d00): singleton `DAT_10033dec`.
- Validador de cobertura: los 44 imports NETLIST que usa ISIS
  → gaps reales `addnet/netptr/reset/scriptptr/zap/getcdb/
  newdesign/cleartags` (+ data `net_nc/sdf_pintypes/..._paths`)
  en decompilación.

## Firmware loaders (LOADERS.DLL, 13 + `setcodelimit`)
- `load_hex(path,...)` (@0x10002c00): Intel HEX con validación
  de checksum (`CR or CR/LF expected after checksum`), log vía
  `LOADERS.HLP`, records vía `FUN_10003720/36b0` (+ فرمato
  `OMF51/S19/UBROF/COD/CDB/BIN/ELF/COFF/OBJ/EXE/BASIC`).
  Nuestro parser HEX ya cubre el formato (validación cruzada).

## Runtime interactivo (PROSPICE.DLL, 9 exports)
- API: `rts_initialize/rts_execute/rts_pausetest/rts_suspend/
  rts_terminate/rts_event`, `simulate`, `signalabort`,
  `dumptables`.
- `rts_initialize(char*,?,HWND)` (@0x100417ec): registra
  callback `rts_pausetest` + factory `Ordinal_1()`.
- `rts_execute(int,double*,?)` (@0x100379c0): drive por
  punteros `DAT_1006c760/750` — el loop interactivo que los
  botones play/pause/step alimentan.
  Spec para kernel interactivo propio (fase 2+).

## Scripting de modelos (VSMLUA.DLL, 11 exports)
- 4 flavors completos (SETPOINT/DUALSET/COLORLED exponen los 6):
  `active` (analógico), `dsim` (primitiva digital),
  `spice` (wrapper subckt), `mixed` (MCU). DSO = active+spice.
  Taxonomía cerrada para `vsm-host`.
- `createmixedmodel/deletemixedmodel` + `hostif_*`:
  `open/read/write/lseek/close`, `malloc/realloc/free`,
  `debug` — sandbox POSIX para modelos Lua.
  Spec directa para scripting propio (rhai/mlua) en fase 2+:
  exponer el mismo `hostif_*` a scripts de usuario.

## Modelos VSM (interfaz por flavor, vía `rizin iE`)
- Fábricas DSO (`createspicemodel` @0x10017f0c,
  `createactivemodel` @0x10018038): negocian capacidades vía
  vtable `(*param_2)(id, 0xa3)` — op `0xa3` = query-interface,
  ids `0x80808081`, `0x10000500`, `0xe7d34571` (tipos de modelo).
  Spec del ABI VSM para `vsm-host` (fase 2: implementar el
  dispatch `0xa3` en nuestros modelos).
- MCU/mixtos (`PIC16EX`, `AVR`): `createmixedmodel` /
  `deletemixedmodel` — ya cubierto en `vsm`.
- Analógicos activos (`SPEAKER`): `createactivemodel` /
  `deleteactivemodel`.
- Instrumentos mixtos (`DSO.DLL`): AMBOS +
  `createspicemodel/deletespicemodel` (canal analógico +
  lógica digital) — el modelo a imitar para plot/probe.
- Mayoría de DLLs: 4-6 exports (create/delete + variantes).

## Render DVC (VGDVC.DLL)
- `VGDVC::drawbox(BOX,CTS&,GSTYLE&,ulong)` (@0x1003ebc0):
  box → 4 POINTs + transform CTS — el path de primitivas.
- `VGDVC::drawline(LINE,GSTYLE&,ulong)` (@0x100263bc):
  convierte a POINTs y forwardea — firma del pipeline.
- `SGDVC::beginlayer(u8,int)` (@0x10036f2c): dispatch
  `D2DVC` vs `GLDVC` según flag `+0x2ac` (software vs OpenGL).
  Nuestro SVG = el path software.

## Render backends (GDF layer, VGDVC.DLL)
- `LISTVIEW::hittest/gettext/...` (WINDIALOG): internals del
  widget lista — cubiertos por `Slint ListView` (TouchArea por
  fila ya implementado en el panel).
- Set GDF completo (26 ops, vocabulario del backend): `arc`,
  `atrack`, `bezier`, `bitmap`, `box`, `drawseg`, `ellipse`,
  `epad`, `fillbox`, `hole`, `line`, `path`, `pline`, `polygon`,
  `polyline`, `postpadstack`, `ppad`, `prepadstack`, `rpad`,
  `slot`, `tjoin`, `tline`, `tristrips`, `truetype`, `vector`.
  Los `*pad*`/`padstack`/`slot`/`hole`/`tjoin` = render PCB
  (pads/vías) — spec para modo ARES futuro.
- `SGDVC::drawcache/gdf_atrack/...` (@0x1001350c/@0x100227fc):
  dispatch virtual a 3 backends: GL (`+0x188`), D2D (`+0x2ac`),
  software (fallthrough). GDF = set de ops vectoriales
  (`gdf_arc/bezier/bitmap/atrack/box/circle/line/...`).
  Nuestro `sheet_svg` es el 4º backend (SVG) del mismo set.

- `STATUSBAR::setdeftext/sethintcolour/minwidth`
  (@0x10002750/@0x10001be0/@0x10002e80, APPFRAME): shell de la
  barra de estado (nuestra status bar ya la implementa).
- `TREEVIEW::showpath` y resto de widgets lista/árbol: internals
  cubiertos por widgets Slint.

## Data-binding de diálogos (WINDIALOG.dll)
- `FPTEDIT::parseargs(FORM&,char*)` (@0x10017940): parsea
  args float/double del FORM — binding campo↔variable.
- `SPINFPTEDIT::setfpt(double,char*)` (@0x10004c50): setter
  vía `vtable+0x44` (spinner + unidades).
- `TEXTEDIT::setlimits(min,max)` (@0x10016230): guarda en
  `+0x3c/+0x40` + `EM_SETLIMITTEXT` — validación de campos.
  Patrón para validar nuestros LineEdit (longitud/unidades).

## Modelos (212 DLLs, `docs/models-audit.txt`, 189 C-ABI + casos)
- Mayoría: 2 exports C puros (`create*/delete*`) — ABI mínima.
- 4-6 exports: instrumentos digitales/analógicos (DLA, DSO,
  KEYPAD, SCOPE, SIGGEN, LOGAN, I2CDEBUG, SPIDEBUG...).
- `SETPOINT.DLL`: 46 mangled, clases `SPDOUT/SPVOUT`
  (salidas setpoint analógicas) — estímulo programable.
- `NETSHIELD.DLL`: 95 mangled, `QTcpServer/QAbstractSocket`
  (TCP real) — el modelo de red.

## Sistema de propiedades (PROPTEXT, LXLCORE.dll)
- `getprops(nombre, PROPLIST&, a, b)` (@0x1002d6cc): herencia
  de props a PROPLIST (STL-heavy) — el mecanismo que resuelve
  `{PREFIX/MODDLL/PROGRAM}` y `device_props`.
- `getblock/removeblock(nombre)` (@0x1002a5ac/@0x10026544):
  bloques de texto (struct TEXT) — split/join de props.
- `setprops` vive en LXLCORE (externo a NETLIST) — par de
  `getprops`.

## Estilos + settings (VGDVC/APPFRAME)
- `APPLNVARS::load_applnvars(path)` (@0x10029cf0): simétrico
  al save, vía accesor `FUN_1003ce90`. Spec para settings
  propios (TOML en `~/.config/atenea`).
- `APPLNVARS::save_applnvars(path)` (@0x10029f10): itera
  registro DPA y vuelca settings (persistencia Proteus).
- `GSTYLE::setpencolour/setpenwidth` (@0x10035a60/@0x10035a94):
  flag dirty bit `0x200` en `+0x16`, cache en `+0x1a`/`+0x26`.
  Nuestros estilos SVG usan la misma semántica (color+ancho).

## Wiring menú+teclas (APPFRAME.DLL)
- `replicatebranch(MODULE*,MODULE*,IDMAP*)` (@0x10005e70):
  replica jerárquica con remapeo de PARTs.
- `cachemodel(nombre,istream&)` (@0x100174a0, privado): cachea
  modelos parseados.
- `PARTITION::writefrequency(f,i,complex)` (@0x1000ca70,
  virtual): vuelca dato complejo de frecuencia.
- `PINOUT::getnetinfo(...)` (@0x100162c0): resuelve POPIN →
  net + `ELECTYPES` + arrays `char[31]`.
- `NETLIST::getpropdefs(NET_PART*,GDIDX<PROPDEF>&)`
  (@0x10015d60): llena defs de props por parte.
- `ANALOG_ITERATOR::readdata()` (@0x1000d700, privado,
  virtual): avanza muestra (`+0x44→+0x3c` + refill vía
  `vtable+0xc`) — el stepping analógico.
- `CHANNEL_ITERATOR::dval(t)` (@0x1000d690, virtual): base
  retorna constante `2` (STATE por defecto; `ANALOG_/
  DIGITAL_ITERATOR` la overridean).
- PSM = análisis de power-nets (`PCS` structs): `psmmulti/
  psmsingle/psmtidy/psm_analyse` (@0x10019e70/@0x1001a4a0/
  @0x1001a690/@0x1001ab50, `psm_analyse` privado) — rails
  VCC/VDD/GND/VSS/VEE (conecta con `PWRRAILS.INI`).
- `getsiblings(SHEET*,UID,GDPA<ELEMENT>&)` (@0x10005820):
  instancias hermanas multi-hoja.
- Familia `get*(GDPA&)` (@0x100067d0/@0x10006b20/@0x10006950/
  @0x10006cf0): colectores GDPA — `childmodules(SHEET*)`,
  `deviceinstances(ICDBDEVICE*)`, `isiscomponents()`,
  `packageinstances(ICDBPACKAGE*)` — el API de query del CDB.
- `SELECTION::checkplacement` (@0x1000c780, APPFRAME): valida
  placements vía `DPA::inbounds` sobre lista `+0x38`.
- `APPLNVARS::register_applnvar(nombre,APPLNVARTYPES,ptr,size)`
  (@0x10029c20): registro tipado de settings (el schema).
  `restore_applnvars` (@0x1002a0e0): aplica valores guardados
  barriendo el DPA. Spec para settings propios.
- `VGDVC::drawpolygon(POINT*,n,GSTYLE&,ulong)` (@0x100256b0):
  polígono por array de vértices (path de zones/pours).
- `CMDINFOTABLE::setuistate(HMENU,cmd,UISTATE&)` (@0x100349a0):
  `EnableMenuItem` + `CheckMenuItem` desde `UISTATE{enabled,
  checked}` (vía `APPLNMODULE::setuistate` +`0x68`, @0x1001d9c0).
  Wiring menú completo: find/append/bitmaps/state/accel.
- `CMDINFOTABLE::setmenubitmaps(HMENU,idx,mask)` (@0x10034740):
  `SetMenuItemBitmaps` vía `MENUITEMINFOA` — iconos del menú
  desde los strips BMP (cierra menú→icono).
- `CMDINFOTABLE::appendmenuitem(HMENU,cmd,sub)` (@0x10034700):
  `find()` → `AppendMenuA` con `CMDINFO+2` (id) y `+8` (texto).
- `translate_and_dispatch(HWND,key,msg)` (@0x10036f10):
  `TranslateAccelerator` + dispatch vía `find_cmd` (+ helpers
  `FUN_10038dd0/38e10`).
- `KEYTRANSLATOR::find_cmd(key,shift)` (@0x10037640):
  `bsearch` sobre tabla `KEYCMD` ordenada (máscara `0xfff0`) —
  atajo → comando.
- Kit Win32 propio detrás de TODOS los diálogos: `FORM`
  (`arg_bool/cptr/double/int/ptr`, `msghlr`, `takefocus`),
  `LISTVIEW`, `TREEVIEW`, `SELECTOR`, `CONTROL`, `COMBOBOX`,
  `BUTTON`, `EDITBOX` (`parsevalidchar`), `TABCONTROL`,
  `STATICWIN`, `PTEXT`, `TEXTVIEWER`, `FILESELECTOR`
  (`getloadname/getsavename`), `GRAPHICBTN` (`bmpname`,
  `islatched`, `paintbutton` — botones con icono BMP).
- Mapeo a Slint para diálogos nativos futuros: FORM→Window,
  LISTVIEW/TREEVIEW/SELECTOR→ListView, COMBOBOX→ComboBox,
  EDITBOX→LineEdit, TABCONTROL→páginas manuales (ya hecho),
  GRAPHICBTN→Button+icon, FILESELECTOR→rfd (ya hecho).

## Router (ELECTRA, shape-based)
- `ELECTRA.DLL` (4.4 MB, ABI C): `Adaptive Shape-based
  AutoRouter` (linaje Specctra). Vocabulario DO/costos extraído
  de strings: `autoroute/autorouteSetup`, `cost`, `fanout` +
  `fanout_dir/dist/passes`, `recorner(+diagonal)`, `route`,
  `protect/unprotect`, `via_*` (`at_smd/fit/grid`, `keepout`,
  `number`, `on_pin`, `share`, `testpoint`, `via_same_net`),
  `limit_bends/crossings/vias`, `pair`, `bus diagonal`,
  `Bus Router (colinear pins)`.
  Spec para router propio: DO-file con estos comandos + tabla
  de costos + matriz de capas (ver `LYTCVT`/stackup docs).

## Auditoría total de binarios (`docs/binarios-audit.txt`, 82 DLL/EXE)
- Por mangled (sin Qt): APPFRAME 1703, VSMSTUDIO 1467, FLOWCHART 1161,
  ARES 1184, ISIS 988, GVIEW 905, PDS 893, WINDIALOG 825, VGDVC 732,
  DXP 719, VSMDEBUG 546, NETLIST 427 (0 gaps), VIEWDATA 392,
  IMPORTER 375, PRIMS 339, LIBMAN 205, XLIB 121, LOADERS 14.
- Cero mangled (ABI C o stripped): ELECTRA (API `dofile`, ya doc),
  CONSOLE, SPICEINP, GETSPICE/PUTSPICE, PROSPICE, EASYHDL, SDFGEN.
- `SPICESIM.DLL` (1 MB): 17 exports C — el fork SPICE3F5 al
  desnudo: `spice_initialize(SDF_NETLIST*,SIMLOG*,ISPICECKT*)`,
  `spice_terminate`, `CKTdltMod/dltNNum/fndDev/fndNode/free/
  mkCur/mkVolt/setBreak/terr`, `DCtran`, `NIintegrate`,
  `SMPmakeElt`, `simerrmsg/simerrrtn/siminit`.
  Confirma linaje Berkeley SPICE3f5 (nuestro backend ngspice
  es el reemplazo nativo correcto).

- Satélites: `DXP.dll` = shell puro (BOM vive en SDF/`*PARTLIST*`,
  ya parseado); `FLOWCHART.DLL` = 7 métodos lógicos (solo
  `ColourCombo` — la lógica de gráficas está en ISIS/NETLIST);
  `GVIEW.dll` reusa el stack `VGDVC/PRIMITIVE/LXLCORE` (mismo
  render que ARES) + `UNZIPARCHIVE/APPLNVARS/SELECTION`;
  `VSMSTUDIO` = editor QScintilla + shell.
- Resto (~300 variantes `VGDVC::draw*` + shells Qt + diálogos
  `WINDIALOG`) = bajo valor para Atenea (render propio vía SVG,
  diálogos nativos Slint) — documentado, no decompilado 1×1.

## FUSIÓN SimulIDE (fuente última versión, `/tmp/simuide-src`)
- Arquitectura SimulIDE: `Circuit` (documento: componentes +
  conectores + nodos, XML `.simu`, undo/redo) + `CircuitView`
  (canvas) + `ItemLibrary`/`ComponentSelector` (paleta con
  búsqueda) + `Component` (id+tipo+props+pines+visual) +
  `MainWindow` (paleta izq, props der, canvas centro, consola abajo).
- Mapeo a Atenea (100% Rust/Slint, sin Qt): `DocSnapshot` =
  `Circuit`; `sheet_svg` = `CircuitView`; panel izq + filtro =
  `ComponentSelector`; `PartView`+`device_props` = `Component`;
  tabs Sim/ERC/consola = docks inferiores.
- Fase 1 hecha (`f984927`): autoload `ArduinoTEP.IDX` al arrancar
  (paleta nunca vacía), click en fila = select (highlight azul),
  `Place` = append a schematic en memoria (ref auto `XX+n`,
  cursor grilla 25.4mm) → re-render hoja + Parts + ERC live.
- Fase 2 (roadmap): props editables a la derecha, drag en hoja,
  conectores+nodos (wire entre placements), sim del schematic
  en memoria vía `to_cir_with_subs`.

## Iconos/toolbars extraídos del PE (verificado en pixels)
- Tabla comando→icono CERRADA: `docs/icons/` con los 63 gifs del
  `Icon_Reference_Chart` del CHM + `CHART.txt` (gif => comando):
  New/Open/Save/Import/Export/Print/Refresh/Grid/FalseOrigin/Pan/
  Zoom×4/Undo/Redo/Cut/Copy/Paste/Block×4/Pick/Make/Packaging/
  Decompose/WireAutorouter/SearchTag/PAT/Explorer/NewSheet/Parent/
  BOM/ERC/ARES/Select/Component/Junction/WireLabel/Script/Bus/
  Subcircuit/Pins/Graphs/Tape/Generator/VProbe/IProbe/VInstruments/
  2D×7/Rotate×2/Mirror/Flip.
  Cruce con strips: cursor/play/LBL/gates/sine/cap/línea/caja/
  círculo/arco/A/S/+ verificados en pixels (`aui/assets`).
- Strips RT_BITMAP (`type 2`) en `ISIS.DLL` (98) y `ARES.DLL` (102):
  toolbars horizontales 24/32px — celdas de 24/32px, fondo gris.
- ISIS: modo (cursor/play/LBL/gates/sine/cap/curva/probe/línea/caja/
  círculo/arco/A/S/+), edición (undo/redo/tijera/copiar/flechas/X/
  mano/wrench/martillo), sim (play/pause/step), 2D, zoom, carpetas.
- ARES: track/vía/zona/pour/DRC/ratsnest/layers/3D + edición común.
- Iconos RT_GROUP_ICON (33 en `PDS.EXE`): chip/gráfica/filtro/X/
  sonda/componente/R/LED/cap/tijera/A — NO hay casa/guardar (esos
  viven en strips BMP, no en iconos).
- Splash `PROTEUS DESIGN SUITE / ISIS SCHEMATIC CAPTURE` en strip
  `ISIS.DLL` (banner rojo/negro, auto + cerebro).
- Nombres de iconos en uispecs (`=BMP`: `ALIGNBOTTOM`, `PKG_ACW`…)
  aún sin mapear a IDs de grupo — pendiente tabla comando→icono.
- Assets en `crates/aui/assets/`: `tool_select/sim_play/lbl/sim_graph/
  wire/box2d/circle2d/text/chip/folder/terminal/wrench/bug.png`
  (ISIS) + 50 `ares_*.png` (modo PCB futuro).

## Cobertura (números honestos, `rizin aa;aflq`)
- `NETLIST.dll`: 501 funciones totales (incluye CRT/MFC/thunks).
  API vendor (29 clases): 0 símbolos sin cubrir — cada método con
  dirección Ghidra, pseudocódigo REA y/o patrón verificado.
  264 decompilados individuales archivados (`/tmp/nl*.md`).
- `DSIM.DLL`: 46 funciones; ABI 29/29 exports con prototipos exactos
  + 10 core decompilados (create/initialize/setup/accept/timestep/
  connect×2/linknet/mergenets). Resto = helpers internos `FUN_*`.
- Formatos 100%, corpus 0 unknowns nativos, 33 iconos + 200 strips
  en `aui/assets`, 823 clases AV*, 171 command IDs, 1107 specs UI.
- Pendiente (build, no RE): kernel interactivo `vsm-host` con los
  slots vtable documentados, router propio, tabla comando→icono,
  U/X con macromodelos, verificación visual `aui` con usuario.

## ARES/PCB — arquitectura real (vía REA cross-DLL)
- `ARES.DLL` es cáscara fina: la lógica vive en DLLs compartidas:
  `LXLCORE.DLL` = kernel geométrico (`POINT::rotate`,
  `POLYGON::isclockwise`, `GPC_POLYGON::touches`,
  `OCTAGON::collidesclosed`, `LINE::parallel`);
  `PRIMS.DLL` = primitivas PCB (`PRIMITIVE::normalise/makesym`,
  `PR_BOX::makepath`, `LABEL::drc` — el `drc` reportado como
  `PR_SYM::drc`/`PR_PATH::drc` resuelve a `LABEL::drc`);
  `VGDVC.DLL` = render (`VGDVC::drawcpad/drawhole`,
  `SGDVC::setlayercolour`);
  `APPFRAME.DLL` = shell (`MENU::bmp_name`,
  `INPUTHANDLER::snapfunc`).
- `ORNTBAR::setornt(CTS)` (@0x10054f90): barra de orientación
  desde `CTS::isorthogonal` (estado de rotación).
- `TOOLBAR::addbuttons(n0,n1,n)` (@0x10053630): arma array
  `TBBUTTON` (stride `0x14`) — creación Win32 del toolbar.
- `TOOLBAR::setactvcmds(cmds[],n)` (@0x100537b0): recorre
  botones vía `FUN_10055a30/55a50` (count/por-botón) y setea
  comandos activos — el wiring comando→botón.
- `TOOLBAR::getbitmap(i)` (@0x10006220, APPFRAME):
  `SendMessageA(HWND, TB_GETBITMAP=0x42c, i, 0)` — API Win32
  estándar: los botones referencian índices del strip BMP.
  Cierra el mecanismo toolbar→icono.
- `MENU::bmp_name/tbar_bmpheight/tbar_bmpwidth` confirman el
  mecanismo toolbar-BMP (strips `type 2` ya extraídos).

## Kernel PCB (LXLCORE/PRIMS, vía REA)
- `MLIB` (XLIB.DLL): `rdlibnames`=`+0x40`, `wrlibnames`=`+0x54`
  (listas de librerías read/write; `num*` adyacentes).
  Nuestro `libdir` ya parsea el stride-80 — MLIB es el lado
  templates/footprints (ver `LML`/`MDF` en formatos).
- `SYMBOL::preview(CTS&,SGDVC*,LIBBROWSEINFO*,char*)`
  (@0x1000d420, PRIMS): render del browser de librerías.
- `PRIMITIVE::gethandles/resize/setunit` (@0x10007fb0): stubs
  base (thunk a 0).
- `PRIMITIVE::usedlist/PR_SYM::usedlist` (@0x100081d0/
  @0x1000c9a0): colección DIDX de dependencias (vía `this+0x10`
  y `SYMBOL+0x30` — path BOM/pack).
- `occa` resuelve a impls compartidas: `VGDVC::textocca` +
  `LXLCORE::BEZIER::occa`. `MLIB` vive en `XLIB.DLL`
  (`MLIB::numrdlibs`) — formato lib en búsqueda.
- `PRIMITIVE::importrgn(RGNFILE*)` (@0x10008070, PRIMS): formato
  RGN = texto ASCII (`fscanf %d`, primitiva en `this+0x24`).
- `LABEL::drc(IDRCLIST*)` (@0x10006790, PRIMS): occupancy de
  texto vía `VGDVC::textocca` + callback `IDRCLIST[4]` (gate
  `this+0x76&1`).
- `OCTAGON::vertex(i)` (@0x100480c0): vértice del octágono
  eje-alineado (centro+radio) — primitiva de clearance DRC.
- `POINT::snapup(grid)` (@0x10056050): `ceil()` (snap a grilla).
- `LINE::nearest_point(POINT)` (@0x10045f58): proyección en
  doble precisión (snap de ruteo).
- `POLYGON::splitconcave(GSLIST<POLYGON*>&)` (@0x100527cc,
  LXLCORE): triangulación (SUBPOLYGON) — base del pour/DRC.

## Sync ISIS→ARES (validador lado ARES, 37 imports)
- `createcdb/releasecdb` (@0x10002d10/@0x10002db0): singleton
  con refcount (`DAT_10033df0`, instancia `DAT_10033dec` =
  `getcdb()`).
- `firstname(NET*)` (@0x1001c530): nombre del net (head o tabla).
- `NETLOG::logerror(fmt,...)` (@0x10019990): varargs →
  `writelog(tipo 0x45=ERROR)`.
- `NETCACHE::setsource` (@0x1000efe0): cambia `INETSERVER`
  (`+0x29c`) con `invalidate` — cierra validador lado ARES.
- `CDBCORE::savesheet` (@0x100047c0): writer VFILE de SHEET
  (el `load` es `LXLCORE::adi_load` — persistencia compartida).
- `getpart(UID)` (@0x10001120): lookup en tabla parts
  `+0x2d0/+0x2cc` (quinta tabla UID confirmada).
- `getcommonparent(GDPA)` (@0x10005b40): hoja común subiendo
  la cadena (usa `isancestor`).
- `updatefrompart` (@0x10004ec0): copia ELEMENT (52B) — aplica
  cambios del PART al elemento.
- `getpartelements/getswapgroups` (@0x10006710/@0x10006dc0):
  colectores GDPA sobre tabla de elementos (part→elements,
  pinout→swapgroups para gateswap en PCB).
- `beginsyncboard/endsyncboard` (@0x10004ff0/@0x10005110):
  bracketean el sync de BOARD (flag `+0x340`, barren tabla `+0x2cc`).
- `NETLIST::getelements` (@0x10016030): delega a
  `CDBCORE::getpartelements` por link `+0x18`.

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
- [x] convertidores decompilados (`convert` en los 3): mismo esqueleto
  table-driven (init once, walk `tabla+1` con callback de progreso
  `Loading %s...`): LYTCVT @0x1000303b (578 procs), IDSCVT40
  @0x100030d4, DSNCVT40 @0x1000e950.
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
- `FUN_10002cdc(t_lo, t_hi)` (@0x10002cdc): dispatcher de la cola
  de callbacks del kernel (lo llama `timestep`).
- `FUN_1001f194()` (@0x1001f194): init global previo (lo llama
  `initialize`).
- `FUN_10018900(SDF_NETLIST*)` (@0x10018900, 470 líneas, STL):
  walker que construye la base de simulación (4 `operator_new`)
  — lo llama `initialize` con el netlist.
- `FUN_10003fe8` (@0x10003fe8, fastcall): interning de nets —
  cache `DAT_10035900`, si no `FUN_100215f8(&DAT_10035f80, nombre)`
  (pool `35f80`). Lo usan linknet/connect/mergenets.
- `FUN_100213d0` (@0x100213d0): clasificador de modelos —
  `FUN_10021690(PROPLIST)` → índice → tabla de strings en
  `this+0x10` (entries de 2 chars, estilo ELECTYPES) → puntero
  al tipo. `dsim_create` lo llama con la tabla `0x1002dea0`.
- `dsim_create` (@0x100046b4): `FUN_100213d0(PROPLIST, str)`
  clasifica el modelo (SPICE vs VSM) y lo instancia en la lista
  `+0x35f70`.
- `dsim_initialize` (@0x1001fc14, 10 instr): `FUN_1001f194()` +
  `FUN_10018900(netlist)` + flags `35928/20/24/5c` + singleton
  `&DAT_100360ec`.
- `dsim_setup` (@0x10003d24): itera modelos llamando `vtable[1]`
  (setup); -1/0 según `DAT_100360c4+0x2c`.
- `dsim_timestep(t, &tref)` (@0x10002914): quantum en picosegundos
  (`t/1e-12`, `ROUND`, control FPU) — el kernel mixto avanza en ps.
  Nuestro `vsm-host` pisa en 1000 ps hoy (coherente).
- `dsim_accept(t, ctx, cb)` (@0x1000279c): `t/1e-12 → ROUND` a ps;
  registra callback de aceptación de paso.
- `dsim_timestep(t, &tnext)` (@0x10002914, kernel loop):
  1. `*(360c4+0x2c)=0`, `360e4=360e8`, estado `35928=0x10`;
  2. por modelo en `35f70[i]`: `vtable[2]` (=simulate) con
  `(t, estado)`, `vtable[8]` para delay;
  3. tiempo 64-bit ps en `3593c:35938`; cola de delays `360c8`;
  callbacks vía `FUN_10002cdc`; flag suspend `35934`;
  4. `*tnext = tiempo*1e-12 - local` (próximo quantum).
  Slots vtable modelo: `[1]`=setup, `[2]`=simulate(t,state),
  `[8]`=delay — spec para `vsm-host` (roadmap kernel interactivo).
- `dsim_linknet(nombre, STATE**)` (@0x10004288): resuelve net por
  nombre → `STATE*` en `+0x3c`, 1/0.
- `dsim_bootstep` (@0x10003938): `boot init...` por callback SIMLOG;
  itera lista de modelos (`+0x35f70`, count `+0x35f6c`) con init
  por modelo.
- `dsim_resume()` (@0x10003c98): drena cola de callbacks (`+0x35954`
  lista linkada), limpia flags `+0x35934/+0x35930`, retorna 2/0/-1.
- `dsim_suspend()` (@0x1000261c): flag `DAT_10035934=1` (resume lo
  limpia).
- `dsim_initialize(SDF_NETLIST*, IDSIMCKT**)` (@0x1001fc14):
  construye el `IDSIMCKT` singleton (`DAT_100360ec`) desde el
  SDF netlist + resetea estado (`DAT_10035928/20/24/5c`).
- [x] DSIM.DLL ABI completa (29 exports, prototipos exactos vía
  `rizin iE`): `dsim_create(IINSTANCE*,char*,PROPLIST&,IDSIMMODEL*,
  void**)` (@0x100046b4) — factory de modelos; `dsim_accept(double,
  void*,cb)` (@0x1000279c); `dsim_setup()` (@0x10003d24);
  `dsim_connect(char*,SDF_PINTYPE,char*)` (@0x100045a0) y
  `dsim_connect(void*,char*,SDF_PINTYPE,char*,IDSIMPIN2**)`
  (@0x10004630); `dsim_linkadc(char*,void**)` (@0x10004310),
  `dsim_linkdac(char*,double*,void**)` (@0x100042b8),
  `dsim_polldac(void*,STATE*,double*)`/`dsim_postadc(void*,STATE)`;
  `dsim_peekpin/pokepin`, `createadcs/createdacs`, `getbuspin`,
  `linkprobe/linktape/writedata`, `signon(void*,SIMLOG*,IWATCHPOPUP*)`,
  `getdelay(IINSTANCE*,char*,i64)`/`getinitval(IINSTANCE*,char*,long)`.
  Nuestro `vsm` cubre create-level; stepping completo = roadmap.
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
- [x] DAC (`NATDAC.LML`, 25 decks): DAC081S101/082S085/084S085/101S101...
  + ADC080X (2950 B); reguladores 7805-7815 en `ANALOG.LML`
  (FORMAT 3.0/8.0, decks `Design: 7812`).
- [x] CPUs VSM = `createmixedmodel` TODAS (PIC16EX/PIC18/pic16/AVR2/msp430/dsPIC33/PIC24/HC11/ARM7TDMI/AVR/cm0/cm3 + genéricos .LML):
  PIC16EX (14MB: USART/USB/SPI/I2C/ADC/DAC/PWM/CCP/LCD/TMR/WDT/BOR/
  Comparator/PLL/VREF/SR-latch), AVR2 (4.5MB, GDI propio). Firmware
  vía HEX (LOADERS.DLL).
- [x] opto/triac: `OPTO.LIB/LML` (4N2x, MOC3xxx); TRIAC tb. en
  `ANALOG.LIB`/`DEVICE.LIB`; cristales en `DEVICE.LIB` + `ANALOG.LML`.
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
- [x] misc3: VSM Studio = XMLs de toolchains (`compilers/`, `programmers/`,
  `simulator/`, `SYNTAX/` editing) + devboards `.pdsprj`; `Translations/`
  Qt `.qm` (incluye `proteus_es.qm` — la UI vendor existe en español);
  `SampleSearchCache.ini` = QSettings con QVariants.
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
- Mapa objeto `NETLIST` (offsets verificados en decompilados):
  `+0x0` SHEET* actual, `+0x1c` CDBWALKQ*, `+0x20` módulo raíz,
  `+0x24/+0x28/+0x30` flags (bits: 2 graph, 3 realtime, 4 dxp, 5 bom),
  `+0x34` flags hoja, `+0x38` OR-mask, `+0x3c` PROPLIST* DSN,
  `+0x64` PROPLIST DSN_* (+100 dec), `+0x70`/`+0x74` tabla PROPDEFS
  (`KEY\0valor`), `+0x88` DPA maptables, `+0x9c` PROPLIST MODELDEFS,
  `+0x100` PARTLIST, `+0x10c` count partes, `+0x110` array partes,
  `+0x140` DIDX scripts, `+0x160` DPA traces, `+0x170` array TRACEPROP,
  `+0x180` count nets, `+0x184` array nets, `+0x1a0` SLIST netprops,
  `+0x1a8` NET_PMT pendiente, `+0x1ac` SLIST sub-partes,
  `+0x19c` módulo actual, `+0x19c+0x44` PROPLIST módulo,
  `+0x1c4` SLIST staging (addwire/addptr), `+0x1cc..0x1ec` staging
  (count `+0x1d8`, sucio `+0x1dc`, merge `+0x1e0/0x1e4/0x1e8/0x1ec`),
  `+0x200/count+0x240` sample keys, `+0x244/count+0x284` product keys,
  `+0xc0` DIDX modfiles.
- Layouts secundarios (offsets verificados): NET `+0x4` array pines
  (stride `0x48`, rol `ELECTYPES` en `+0x38`), `+0x8` array props (stride `0x66`: clave `+0x0`, valor `+0x33`), count pines `+0x10`,
  count props `+0x14`, estado `+0x20`; NET_PART `+0x4` nombre?, `+0xdc`
  estado PSM, `+0xe8` link, `+0xf8` PINSWAP list, `+0x118` extra;
  NET_MODULE `+0x38` parent, `+0x3c` NET_PART link, `+0x44` NRPROPLIST;
  MODULE `+0x8` nombre (0x32), `+0x78` param, `+0x7c` UID;
  ELEMENT `+0xc` UID, `+0x80/84/88` PINTEXT; TRACEPROP en DPA `+0x160`;
  SHEET `+0x8` índice; PROPLIST=`+0x64` DSN (KEY\0valor).
- `dsim_setup()` (@0x10003d24): invoca setup de cada modelo
  (vtable+4) sobre el singleton `IDSIMCKT`; `terminate()`
  (@0x1000e8fc) libera listas `+0x35f60/+0x35f80`.
- `dsim_connect` ×2 (@0x100045a0/@0x10004630): string parte por
  `_` + `bsearch` (solo OP 0xd/IP 0xe); instancia crea
  `IDSIMPIN2`, linkea y loguea `connecting pin %s:%s`.
- `dsim_connect` ×2 (@0x100045a0/@0x10004630): string parte por
  `SDF_PINTYPE` (`0xd/0xe` = bus → expande bits con `FUN_10021c6c`
  sobre `'_'`); la 2ª usa factory `FUN_10003fe8` + `printf
  "connecting..."` debug.
- `dsim_linknet` (@0x10004288): `FUN_10003fe8(ecx,0)` lookup →
  `STATE*` en `+0x3c` (confirma doc previo).
- `dsim_mergenets` (@0x10003ed8): lookup de ambas nets +
  resolución por matriz 32x32 `DAT_10035960` indexada por
  `state&0x1f` (tabla de drive-strength) — spec para `aengine`.
- `dsim_peekpin(modelo, idx, STATE*)` (@0x10003d90): forward con
  guarda NULL a `FUN_10003da8` (poke simétrico en curso).
- `dsim_pokepin(modelo, idx, t_segundos, STATE)` (@0x10003de4):
  maneja analógico con tiempo en ps (`ROUND(t/1e-12)` 64-bit).
  Confirma timebase ps en todo el kernel.
- `dsim_signon(modelo, SIMLOG*, IWATCHPOPUP*)` (@0x1000e928):
  registra log global (`DAT_100360c4`) + watch popup (`DAT_100360d4`).
- `dsim_mergenets(n1, n2)` (@0x10003ed8): fusiona nets con tabla
  de combinación de estados 32×32 (`DAT_10035960`, índices
  `&0x1f`); re-apunta pines (`+0x54/+0x58`).
- `dsim_createadcs`/`createdacs` (@0x100043bc/@0x10004348): registran
  conversores en lista `+0x36040` (flag ADC `+6`, DAC `+0xb`).
- `dsim_linkprobe(modelo, PARTITION*, ch)` (@0x10004490): bindea
  probe (`+0x9c` partición, `+0xa4` canal, label truncado 0x20 en
  `+0xa8`); log `DSIM Linked probe %s`.
- `dsim_linktape(modelo, CHANNEL_ITERATOR*)` (@0x1000442c): espejo
  de linkprobe para cintas (`+0x9c` iterador, label `+0xa0`);
  log `DSIM Linked tape %s`.
- `dsim_writedata(t, modelo)` (@0x10002628): inserta ordenado por
  tiempo en lista `DAT_100360e8` (corte en `DAT_10036098`).
- `dsim_getbuspin(IINSTANCE*, nombre, ...)` (@0x10004500): busca
  modelo en lista (`+8` == instancia), `FUN_10007594` → `IBUSPIN*`.
- `getdelay(IINSTANCE*, pin, __int64)` (@0x10007264): devuelve
  `__int64` (escala ps).
- `getinitval(IINSTANCE*, pin, long)` (@0x1001fc54): lee prop del
  pin vía vtable+8; valor `RANDOM` → `srand/rand` con estado
  `DAT_1003259c` (xorshift) — iniciales aleatorios deterministas.
- `proptypenames` (`?proptypenames@PROPDEF@@2PAPBDA`): array static
  de `char const*` (dato, como `mdf_id`) — vocabulario de tipos.
  `PROPTEXT`/`PROPLIST` (`putblock`/`addpair`) viven en `LXLCORE.DLL`
  (imports, no código NETLIST).
- `PINOUT::checkpinswap` (`?checkpinswap@PINOUT@@QAEHPBD@Z`, public,
  @0x1001dc00): valida swaps parseando listas `;` + `, `
  (`getnzstr`, buffer 8K, `_stricmp`) — formato PINSWAP.
- `CDBCORE::addrootsheet` (@0x100036c0): `addsheet` sobre módulo
  raíz (`+0x300`).
- `CDBCORE::addsheet` (@0x100036e0): `SHEET=new(0x114)`, `+0x8`=MODULE,
  `+0x4`=UID, nombre `strnzstr 0x32` en `+0xc`, conteos `+0x10c/+0x110`.
  Estructura hoja confirmada (nuestro `pproj::cdb` sheets ROOT).
- `PARTITION::getiterator` (@0x1000cee0): `CHANNEL_ITERATOR`
  polimórfico (ANALOG/DIGITAL/FREQUENCY según `simtype`) — el que
  consume `dsim_linktape`.
- `PINOUT::gethiddenpins` (@0x1001d920): recorre tabla `+0x70`
  (count `+0x6c`); pines con flag `+0x3c` y rol != `0x11` → DIDX.
  (`0x11` = rol power oculto: liga con `addlabel` y `validate`
  hide-common-pins.) PINOUT cerrado: netgen/checkpinswap/gethiddenpins.
- Roles por clase (inventario completo de métodos, semántica por
  nombre + miembros decompilados): `CDBCORE` = DB de diseño
  (sheets/snippets/módulos/elementos, variants, sync, bind);
  `PARTITION` = corrida sim batch (`begindump/endrun/flush`,
  `CHANNEL_ITERATOR` con `ANALOG/DIGITAL/FREQUENCY`);
  `PINOUT` = mapeo pines↔package (netgen/checkpinswap/gethiddenpins,
  `getpin*/getnetinfo` pendientes); `NETLOG`/`SIMLOG` = logging con
  contexto (push/pop, `logerror/logwarning/loginfo`);
  `NETCACHE` = caché de compilación (`autorecompile/rebuild/invalidate`);
  `PROPDEF`/`PROPTEXT`/`PROPLIST` = vocabulario+texto de props
  (viven en `LXLCORE.DLL`); `DPA`/`GDPA`/`SLIST` = arrays dinámicos;
  `VFILE` = I/O virtual (vfgetstr/vfputstr, Pascal strings).
- `ANNOTATOR::globalannotate/localannotate` (@0x10001940/@0x10001bb0):
  global por MODULE, local por SHEET (vía `+0x8`); `reset` +
  `buildglobaladb` + `CDBCORE::annotate`.
- `PINOUT::getpinname` (@0x1001d610): scan tabla `+0x70`
  (`_stricmp` en nombres `+0x4c`, count `+0x54`).
- `PINOUT::getswappedpins` (@0x1001d860): lista nombres de pines
  swap por id (`+0x50`, `stpcpy` con chequeo, -1 si desborda).
- `PINOUT::getnetinfo` (@0x100162c0): resuelve pin vía `getpin`,
  tokeniza paths `{...}` (`strtok`), structs `POPIN` — PINOUT queda
  completo (netgen/checkpinswap/gethiddenpins/getpin*/getnetinfo).
- `CDBCORE::getpinout` (@0x10006d80): índice 1-based a tabla UID
  (`+0x2d0`, count `+0x2cc`), `PINOUT*` en `+0x128` (espejo de
  `getmodule` +0x250/+0x24c).
- Triviales verificados por muestra (`numelements`=`this+0x54`,
  `isempty`=`!(flags+0x24&3)` — getters de 1 instrucción): los ~150
  restantes (`is*/get*/set*/has*/clone/close/detach/flush/...`)
  son accessors/predicados de igual forma; cubiertos por firma +
  patrón, sin pseudocódigo individual (ruido, no señal).
- `CDBCORE::addsnippet` (@0x100038f0): `newsnippet(sheet)` +
  registro en lista `+0x2e0`.
- `CDBCORE::attachelement` (@0x10004fa0): link `ELEMENT+0x90` =
  `PART*` + refcount en `PART+0x12c`.
- `CHANNEL_ITERATOR::{aval,ival,mval,pval,rval}` (todos en
  @0x1000d6b0 = `FLDZ; RET`, return 0.0): base virtual, las
  subclases ANALOG/DIGITAL/FREQUENCY los overridean.
- `CDBCORE::bindmodule/bindpart` (@0x100041f0/@0x100050a0): bindean
  UID 1-based (tabla `+0x2d0`/count `+0x2cc`, igual que getmodule/
  getpinout) vía interfaces `ICDBMODULE`/`ICDBPART`.
- `CDBCORE::checkpartbinding` (@0x10004b70): valida binding
  `ELEMENT+0x90`→PART. `cleartags` (@0x10006280): cero en `+0x98`
  de tabla `+0x290`/count `+0x28c`. `NETLOG::clearctx` (@0x10002ab0):
  cero en `+0x4/+0x8`. `ANNOTATOR::compare` (@0x100026e0, static):
  comparador de strings con buffers locales.
- `CDBCORE::deleteelement/deletepart` (privados, @0x10007c20/0x10007d30):
  baja por UID validado en tablas `+0x290/+0x28c` y `+0x2d0/+0x2cc`.
- `CDBCORE::detachelement` (@0x10004fc0): inverso de attach
  (decrementa refcount `PART+300`, nullea `+0x90`).
- `PARTITION::enddump` (@0x1000cc40): cierra corrida de dump vía
  writer vtable `+0x414`, contadores `+0x760/+0x440/+0xa80`.
- `CDBCORE::endsyncsheet` (@0x10004370): máquina de sync de hoja
  (requiere estado `+0x334==4`, itera módulos).
- `CHANNEL_ITERATOR::eof` (@0x1000b740): flag `+0x24`.
- `CDBCORE::getboard` (@0x100010c0): tercer tabla UID (`+0x2b0`/
  count `+0x2ac`) — módulos +0x250, pinouts +0x2d0, boards +0x2b0.
- `CDBCORE::getcommonparent` (@0x10005b40): hoja común de una
  lista GDPA<ELEMENT>.
- `PARTITION::getdumprun/getdumpvalue` (@0x1000d1d0/@0x1000d0b0):
  lectura del dump vía `CHANNEL_ITERATOR` (`getiterator` + index
  por `simtype`) — cierra el circuito probe→tape→dump.
- `PROPDEF::getfnargs/getfplimits` (@0x1001ed40/@0x1001eb20):
  def-string en `+0x8`; args `TRUE/FALSE` + comas; límites
  `PNZ/POZ/NNZ` (pos/neg-no-cero). Vocabulario de props tipado.
- `PROPDEF::getkwdprops` (@0x1001ef20): parsea `N,keywords` →
  `PROPLIST::addkwdlist` — el mecanismo de keywords (`INDEXED STRING`).
- `PARTITION::getlabel` (@0x1000b3a0): `index()` + array `+0x43c`,
  label en `+10`.
- Triviales confirmados: `getmaxmoduleuid`=`+0x24c`, `getmaxruns`=
  max(`+0xa80`) — getters de 1 instrucción.
- `PARTITION::getpmd(i)` (@0x1000b380): `base+0x424 + i`.
  `CDBCORE::getrootboard` (@0x10001010): módulo raíz `+0x300` →
  `BOARD*` en `+0x80`.
- `PINOUT::hasgateswap` = flag `+0x58`. `CDBCORE::invariant(ELEMENT)`:
  recursivo vía link `+0x90` (PART).
- `PINOUT::iscommon` (@0x10001d800): `getpin` → flag `+0x40` del POPIN.
  `CDBCORE::isexactsibling` (@0x10005e10): identidad de hoja por walk.
- `PROPDEF` type enum en `+0x14`: 3=HIDDEN, 4=INDEXED (`ishidden`/
  `isindexed` @0x100123b0/@0x100123e0, `==const`).
- `PARTITION::isvalid` (@0x1000ce10, compartido NETCACHE): abre
  `DISKFILE+0x414` con path `+0x209`, valida magic vs
  `DAT_10030420`.
- `ANNOTATOR::mergelocaladb` (@0x10002440): merge de ADB local vía
  `buildbranchadb/buildglobaladb/merge` + `CDBCORE::annotate`.
- `CDBCORE::migratelegacyvariants` (@0x10007610): itera tabla UID
  (`+0x2d0/+0x2cc`) migrando variantes legacy.
- `FREQUENCY_ITERATOR::mval(t)` (@0x1000db60): interpolación lineal
  entre `(t0,v0)@+0x28` y `(t1,v1)@+0x30` tras `advance(t)` — así se
  leen los valores del trace.
- `CDBCORE::newdocument(path)` (@0x10003020): `zap` + set nombre +
  `newmodule(NULL)` como raíz (`+0x300`) — confirma ROOT.CDB
  (módulo raíz primero). `newdesign` similar.
- `CDBCORE::newpart` (private, @0x100078d0): `PART=new(0x148)`.
  `PARTITION::newrun` (@0x1000c710): nueva corrida vía writer vtable.
- `CHANNEL_ITERATOR::nextx` (@0x1000b760): `return *(double*)(this+0x30)`
  (t1 del par de interpolación de `mval`).
- `ANNOTATOR::process(ELEMENT*, ANN_MODES)` (@0x10001d00):
  anotación por elemento vía vtable `+0x24`.
- `NETLOG::pushelementctx` (@0x100197c0): apila nodo ctx `0x28`
  con ELEMENT (logging con contexto de elemento).
- `NETLOG::push*ctx` (@0x10019730/@0x10019850/@0x100197c0): nodos
  ctx `0x28` con tag (1=módulo, 2=elemento, 3=objeto) — pila de
  contexto para errores con ubicación.
- `PARTITION::readpmd` (@0x1000c5f0): dispatch por modo vía
  writer vtable `+0x414`. `CDBCORE::readsnippet` (@0x10009e70):
  carga snippets vía VFILE.
- `CDBCORE::removemodule/removesheet` (@0x10003920/@0x10003ab0):
  teardown con flag (inverso de addmodule/addsheet).
- `CDBCORE::removevariant(char)` (@0x10007430): máscara de bits
  por variante (`'b'` especial). `renamepart` (@0x10004a90): vía
  link PART `+0x90`, si no `updatefrompart`.
- `CDBCORE::selectvariant` (@0x100073a0): fija variante activa
  (`+0x344`). `NETCACHE::setcontext` (@0x1000f010): compara flags
  y setea ctx `+0x2a0/+0x2a4` + `invalidate`.
- `CHANNEL_ITERATOR::setedgetimes` (@0x1000d460): stub base
  (`return 0`, override en subclases). `ANNOTATOR::setdvctype`
  (@0x10001ed0): setea tipo de device (detalle en cuerpo).
- `PROPDEF::setkwdargs` (@0x10001ee10): reconstruye def como
  `"N,keywords"`. `CDBCORE::setpartproperty` (@0x10006f60): setter
  de props de PART vía PROPTEXT.
- `PARTITION::setsimtype` (static, @0x1000b370): setea global
  `simtype` (`DAT_10030440`, el que usan `getiterator`/`getlabel`).
  `NETCACHE::setsource` (@0x1000efe0): cambia `INETSERVER +0x29c` +
  `invalidate`.
- `CDBCORE::tagelement(UID)` (@0x100064b0): lookup en tabla.
  `tidysnippets` (@0x100097b0): itera tabla snippets `+0x2ec`.
- `PROPDEF::typestring` (@0x10012360): `proptypenames[tipo+0x10]`
  (tabla global `0x1003025c`). `PINOUT::unpack` (@0x1001d470,
  virtual): `DPKB::get` + `compile` — path de compilación de package.
- `CDBCORE::walkfirstmodule/walkfirstsheet` (@0x10005190/@0x10005370):
  inician `CDBWALKQ` (flag `+0x18`, módulo `+0x10` default raíz `+0x300`,
  índice `+0x14`) — el walk que usa `setdsnstats`.
- `PARTITION::writeanalog` (virtual, @0x1000c820): writer de trace
  analógico (array `+0x43c`, estado `+0x2c`).
  `walknextsheet` = continuación del walker.
- `PARTITION::writedigital` (virtual, @0x1000c9b0): writer con
  array `STATE*`. `CDBCORE::writeelement` (@0x10009ad0): serializa
  ELEMENT vía VFILE (save path).
- `PARTITION::writepmd` (@0x1000c5b0): writer vtable `+0x414`,
  contador bytes `+0x428`. `CDBCORE::writesnippet` (@0x10009fd0):
  serializa SNIPPET vía VFILE (save path).
- `PARTITION::writestring` (@0x1000ccf0): `vsprintf` → writer.
  `writexval(double)` (@0x1000cb00): writer con tracking de corrida
  (`+0xa84/+0xa80/+0x760`).
- `tracemodenames`/`proptypenames`/`mdf_id`: arrays static de datos
  (`2PAPBDA...`, `3QBDB`), no código.
- `PROPDEF::modetypestring` (@0x1001e8f0): nombre de tipo vía
  `proptypenames[i]` + prefijo según kind (0=plano, 1=`READONLY %s`...).
  `PINOUT::getpinnum` (@0x1001d690): strings de número (tabla stride
  `0x1f` en `+0x4c`, count `+0x48`).
- `PARTITION::getdatastats/newchannel/newobject` (@0x1000d260/
  0x1000c610/@0x1000cc90): junta canales en DPA; crea canal
  (`new 0x60`, default `simtype`); crea objeto vía vtable.
- `NETCACHE::updateclients` (@0x1000f280, private): notifica
  lista clientes (`+0x298/+0x294`) vía vtable+8.
- `PROPDEF::getkwdargs/setfnargs/getintlimits` (@0x1001ec40/
  0x1001ee90/0x1001e9f0): parsean args/límites del def (`TRUE/FALSE`,
  `PNZ/NNZ`...). `CHANNEL_ITERATOR::bval` = stub base (NULL),
  `prevx` = t0 (`+0x28`).
- `ANNOTATOR::getnextname` (@0x100020d0): allocator de nombres
  (`+0x48/+0x4c/+0x50`).
- `ANNOTATOR::setcount` (@0x10001c80): camina cadena SHEET
  (`+0x8/+0x4`), lee contador `+0x110`.
- `CDBCORE::bindelement/findelement` (@0x10004100/@0x10006340):
  bind por UID vía `ICDBELEMENT` (gate `+0x334==4`); find por nombre
  en tabla `+0x290`.
- `CDBCORE::getelementproperty` (@0x10006ea0): `PROPTEXT::get`
  en `ELEMENT+0x44`. `getmastersheet` (@0x10003670): primera hoja
  sin módulo (tabla `+0x270`) — hoja raíz.
- `CDBCORE::newsheet/newboard/newlayout` (@0x100077c0/0x100077f0/
  0x10003190, privados los 2 primeros): SHEET `new(0x114)`;
  BOARD `new(8)` con backlink `MODULE+0x80`; layout crea BOARD
  del raíz si falta.
- `rebindelement` (@0x10004990): re-resuelve PART vía PINOUT.
  `setelementproperty` (@0x10006ec0): delega a `setpartproperty`
  por link `+0x90`.
- `PARTITION::runstart(i)` (@0x1000b3d0): `tabla+0x440[i]`.
  `setdatatype` (@0x1000c6a0, virtual): tipo de canal.
  `NETCACHE::showlog` (static, @0x1000f2c0): registry `VSMDEBUG`.
- `CDBCORE::readelement/readpart/writepart` (privados, @0x100098f0/
  @0x10009c50/@0x10009d80): trío de serialización VFILE (load/save
  de ELEMENT/PART) — cierra el I/O de CDBCORE.
- `CDBCORE::currentvariant` (@0x10001160): byte `+0x344`.
  `deleteboard/deletesheet/deletesnippet` (privados): bajas por UID
  en boards `+0x2b0/+0x2ac`, sheets `+0x270/+0x26c`, snippets
  `+0x2f0/+0x2ec` (cuarta tabla UID).
- `CDBCORE::isancestor` (@0x10005c50): sube cadena
  SHEET→MODULE(`+0x8`)→SHEET(`+0x4`) buscando el módulo.
  `killorphans` (@0x10003500): barre tabla de módulos.
  `removeelement/removesnippet` (@0x10003c40/@0x10003d30): bajas con flag.
- `PARTITION::runsize(i)` (@0x1000b3d0): `tabla+0x760[i]`.
  `pushmodulectx/pushobjectctx` confirman tags 1/3 del nodo ctx `0x28`.
- `NETLOG::setsheet` (@0x10002aa0): `*(NETLOG*) = SHEET*`.
- `NETLOG::writecontext/writelog` (@0x10019df0/@0x100199d0):
  vuelcan el stack ctx a ostream (MSGTYPES/NETCONTEXT) — cierra NETLOG.
- Resto verificado por patrón (getters/predicados de 1 instrucción,
  setters triviales, nodos ctx `0x28`): `PROPDEF::isadvanced/isnormal/
  isreadonly/isvalue`, `PINOUT::errjmp/errmsg/ncpins`,
  `CHANNEL_ITERATOR::getdatawidth/getmaxapoints/getmaxdpoints`,
  `DPA::grow` (realloc, en LXLCORE), `NETLOG::clearerrors/popctx/
  pushlabelctx`, `CDBCORE::beginsyncsheet/getexactsibling/
  getmoduledepth/getrootmodule/setunpopulated/walknextmodule`,
  `PARTITION::endobject/pdfver` — documentados por firma; sin
  pseudocódigo individual (idénticos en forma a los decompilados).
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
