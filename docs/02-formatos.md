# Formatos que lee Atenea (con evidencia)

| Formato | Magic / contenedor | Parser | Prueba |
|---|---|---|---|
| `.IDX` | `DEVICE LIBRARY INDEX\x1A` + count u32 en offset 36 | `plib::libdir::parse_idx` | 7 entradas ArduinoTEP, join por key |
| `.LIB` (DEVICE/PACKAGE/MODEL/SYMBOL) | directorio offset 32, stride 80: `name[64]+seq+key+offset+flags` | `plib::libdir::parse_lib_dir` | 6/7 offsets vivos (NANO documentado como stale) |
| `.DSN` | `ISIS SCHEMATIC FILE\x1A` | `plib::dsn` | 23/23; `WIRE n pts i32 LE`, escala 2.54M uds/pulg (10 nm) |
| `.LYT` | `PCB LAYOUT FILE\x1A` | `plib::dsn` (reutilizado) | Cpu.LYT: 43 bloques `{*VIEW3D}` |
| `.pdsprj` | ZIP (`PROJECT.XML`, `ROOT.CDB`, `ROOT.DSN`) | `pproj` | 534/534, 2 sabores (esquemático / firmware sin TITLE) |
| `.CDB` | binario + strings `CAT/MFR/DESC` | `pproj::cdb` | 35 comps + 120 records en sample; validado vs `CDBCORE::save/load` decompilado |
| `.MDF`/`.SDF` | `... FORMAT x.y` + `*SECCIÓN,conteo` | `pmodel` | 145/145 MDF (+ `*RED,n` anidadas, variante sin título SPICELNK), Ppsu.SDF |
| `.SML`/`.LML` | `SPICE/MODEL LIBRARY\x1a`, TOC 32 B + chunks con longitud | `pmodel::sml/lml` | 6 decks SML **byte-idénticos** a GETSPICE oficial; 1097 LML parsean |
| `.VML`/`.3DS` | magics `MM`/`LX` | clasificados (sin geometría) | 33 archivos |
| `.qm`/`.chm`/firmware | varios | no parseados | 63+136 páginas HTML extraídas y leídas |

## Dumps de verificación (cada uno prueba un formato real)

```bash
cargo run -p plib --example dsn_dump -- siggen.DSN        # 153 segs, bbox 350.52x147.32 mm
cargo run -p plib --example corpus_check -- LIBRARY/      # 2374 archivos, 0.16 s, 15.6% con parser
# Árbol completo Proteus 8 (2026-10-22): 15 unknowns, todos toolchains de
# terceros (MPASM/ASM11/citf scripts/unins000.dat) — 0 archivos nativos
# sin parser (DSN/LYT/LIB/IDX/SDF/MDF/SML/LML/pdsprj/CDB/VML/STY).
cargo run -p pmodel --example sml_dump -- ANALOG.SML     # 4 decks (LM123...)
cargo run -p pmodel --example lml_dump -- ANALOG.LML     # 41+ decks LISA
cargo run -p pmodel --example lisa_dump -- 12ce51x.MDF   # 10 nets, 20 pins
cargo run -p pproj --example pproj_dump -- Isistut.pdsprj # título+TIMESTAMP, DSN validado
cargo run -p pproj --example cdb_dump -- ROOT.CDB        # 35 comps, 120 records
cargo run -p vsm-host --example host_demo -- libnullmodel.so  # dlopen + 10 pasos
```

## Reglas para agregar un formato

1. Example `*_dump` que lo abra y muestre contenido legible.
2. Test con fixture sintético + test con archivo real (skip si falta).
3. Entrada en `corpus_check` (magic bytes).
4. Fila en `docs/reverse-engineering.md` con evidencia.
