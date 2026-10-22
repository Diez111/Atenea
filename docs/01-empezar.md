# Empezar con Atenea (60 segundos)

Requisitos: Linux x86_64, Rust 1.85+, `ngspice` (solo para simular).

## Instalar

```bash
git clone https://github.com/Diez111/Atenea.git && cd Atenea
cargo run -p atenea -- --help          # debug
~/.local/bin/atenea --help            # release instalado (28-30 MB)
```

También: `install.sh` (prefijo `~/.local`), tarball
`atenea-linux-x86_64.tar.gz` (~9 MB), launcher `atenea.desktop`.

## Abrir

```bash
cargo run -p atenea -- Isistut.pdsprj [--tab Esquema]
```

- Sin archivo abre igual: bienvenida + botón **Abrir** + arrastrar y soltar.
- Doble-click en `.pdsprj/.DSN/.LYT/.SDF/.MDF` abre Atenea (MIME registrado).
- `--gl` fuerza OpenGL (VMs, Xvfb, máquinas sin Vulkan).
- Log de arranque: `/tmp/atenea.log`.

## Usar

- **Tables**: resumen, `*PARTLIST`, `*NETLIST`, bloques `{...}`.
- **Graph**: netlist como grafo (pan/zoom, click inspecciona, filtro).
- **Esquema**: hoja estilo ISIS — fondo claro, retícula 0.1"/1", borde azul,
  wires reales, etiquetas de componentes, capas conmutables, pan/zoom con
  autofit, status bar con cursor en mm y thou.
- **Simulate**: cargá un `.MDF`/`.SDF`, elegí `vars` (ej. `v(OUT)`) y `tstop`,
  **Run** → plot propio + valor final. Requiere `ngspice`.

## Fallos típicos

- Diálogo nativo sin backend → usá el botón **Abrir** o arrastrá el archivo.
- Sin Vulkan → `--gl`.
- Sin `ngspice` → Simulate muestra el error; el resto funciona igual.
- Archivo no soportado → mensaje en pantalla, sin crash.
