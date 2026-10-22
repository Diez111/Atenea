# Simular y plugins nativos

## Batch ngspice (`asim`)

`pmodel::parse` → `asim::elements` → netlist → `ngspice -b` → `Trace`.

- Normalizaciones: `9k1`→`9.1k`, `4M7`→`4.7MEG` (**M** mayúscula = mega,
  como Proteus; `m` minúscula = mili), `GND`→nodo `0`.
- Soporta R/C/L/V/I hoy; Q/D/U devuelven `UnsupportedPart` tipado.
- `Trace` parsea tablas `index time v1 ... vn`, ignora headers/footers.

Receta LM123 reproducible (verificada: `v(out)=5.0998` V, dropout ≈6.7 V):

```bash
# extraer deck (lógica de pmodel::sml) a /tmp/p3/lm123.mod, luego:
ngspice -b -o out.log docs/ngspice-lm123.cir
```

La UI (pestaña **Simulate**) hace esto por vos: cargá un `.MDF`/`.SDF`,
elegí `vars` (ej. `v(OUT)`) y `tstop`, **Run** → plot propio + valor final.
`tstop` vacío se rechaza sin lanzar proceso; cada corrida usa su propio
`tempdir` (corridas paralelas seguras).

## Plugins VSM nativos (`vsm-host`, `nullmodel`, `ne555`, `adc0808`)

ABI propia `extern "C"` (limpia, sin NDA):

- `atenea_model_api_version() -> u32` (debe ser 1)
- `atenea_model_create() -> *mut c_void`
- `atenea_model_step(inst, dt_ps: u64) -> i32`
- `atenea_model_destroy(inst)`

`vsm-host::Plugin::load()` verifica versión, crea instancia, `step()`,
`Drop` destruye. Modelos incluidos:

- **`nullmodel`**: contador de prueba (`cdylib` de referencia).
- **`ne555`**: astable `f=1.44/((RA+2·RB)·C)` verificado a 4.8 kHz (duty
  2/3), monoestable `t_w=1.1·R·C`, flancos exactos por test.
- **`adc0808`**: SAR con máquina IDLE→CONVERTING→DONE, `code =
  round(VIN/VREF·255)`, `EOC`, `OE` con alta-Z (`Floating`).

El ABI vendor original (`createactivemodel`, `deleteactivemodel`, ...,
verificados como imports en `ISIS.DLL`) es ABI C++ bajo NDA: Atenea no la
reimplementa ni carga DLLs PE32 (imposible desde x86_64 Linux sin
emulación — ver roadmap). Los modelos nativos son recepción limpia.

## Qué NO hay (todavía)

Stepping interactivo con sondas en vivo, modelos de componentes reales
ejecutables, editor/router, geometría de pads/trazos, empaquetado
distro.
