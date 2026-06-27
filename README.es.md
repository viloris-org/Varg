# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | [日本語](README.ja.md) | [한국어](README.ko.md) | Español

Varg es un motor de juegos y editor experimental construido alrededor de un runtime en Rust, un editor de escritorio Tauri/React y flujos de autoría asistidos por IA. El código actual se centra en una base segura de ECS/runtime, una shell nativa de editor, el lenguaje de autoría Varg, empaquetado de proyectos y automatización de editor estilo Quest/Copilot.

El proyecto sigue en fase pre-1.0. Algunos documentos describen diseños objetivo; este README sigue lo que está representado en el repositorio actual.

![Editor de Varg](docs/screenshots/editor.png)

## Primeros Pasos

Requisitos:

- [Rust](https://rustup.rs/) 1.96 o más reciente
- [Bun](https://bun.sh/) para el frontend del editor
- [Dependencias de sistema de Tauri v2](https://v2.tauri.app/start/prerequisites/)

En distribuciones tipo Debian/Ubuntu, las dependencias de Tauri suelen incluir:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Clona y ejecuta el editor:

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

Construye el workspace de Rust:

```sh
cargo build --workspace
```

## Capacidades Actuales

- **Base runtime en Rust**: ECS, manifiestos de proyecto, assets, entrada de plataforma, traits de render, integración WGPU, física, audio, UI, animación, skeleton, shader, policy, IA y crates de empaquetado.
- **Editor Tauri**: app de escritorio React/TypeScript respaldada por comandos Rust para flujos de hub/proyecto, hosting de viewport, Copilot, Quest, empaquetado, diálogos y ventanas/paneles nativos.
- **Lenguaje de autoría Varg**: parsing de `.varg`, `.vscene` y `.vasset`, diagnósticos, runtime MVP de scripts, declaraciones de behavior y binario `varg-lsp`.
- **Experimentos de scripting declarativo**: estructuras JSON de behavior, scene, UI, system, project y asset bajo `engine-script-declarative`.
- **Pipeline de empaquetado**: `cargo xtask package` crea una carpeta runtime para proyectos de escritorio y valida varias combinaciones futuras de target/format.
- **Política de Rust seguro**: los crates del motor usan `#![forbid(unsafe_code)]`.

## Estructura del Proyecto

```text
Varg/
├── crates/                         # Crates del motor y runtime
│   ├── engine-core/                # IDs, errores, matemáticas, configuración
│   ├── engine-ecs/                 # Escenas, entidades, transforms, componentes
│   ├── engine-assets/              # Base de assets, importadores, manifiestos
│   ├── engine-render/              # Traits de render y modelo compartido
│   ├── engine-render-wgpu/         # Backend WGPU y experimentos de viewport
│   ├── engine-platform/            # Abstracciones de ventana, entrada y filesystem
│   ├── engine-script-varg/         # Parser Varg, diagnósticos, runtime, LSP
│   ├── engine-script-declarative/  # Experimentos de autoría JSON declarativa
│   ├── engine-editor/              # Servicios de editor y soporte IA/herramientas
│   ├── engine-packager/            # Pipeline de empaquetado de proyectos
│   └── runtime-min/                # Raíz de composición runtime
├── editor/                         # Editor de escritorio Tauri/React
├── examples/                       # Proyectos, behaviors y scripts de ejemplo
├── docs/                           # Notas de diseño, PRDs y ADRs
├── schema/                         # JSON schemas
├── scripts/                        # Scripts de utilidad y tests
└── xtask/                          # Comandos de automatización del workspace
```

## Desarrollo del Editor

```sh
cd editor
bun install

bun run dev:tauri
bun run build
bun run tauri build
```

Rutas útiles:

- UI del renderer: `editor/src/renderer/`
- Comandos y servicios host de Tauri: `editor/src-tauri/src/`
- Permisos de Tauri: `editor/src-tauri/capabilities/`

## Features del Runtime

`runtime-min` es el crate de composición. Los conjuntos de features también están listados en la metadata del workspace del `Cargo.toml` raíz.

| Feature | Propósito |
|---|---|
| `runtime-min` | Ruta runtime headless mínima |
| `runtime-game` | Ruta runtime con importación de assets y soporte de ventanas |
| `wgpu` | Backend de render WGPU |
| `physics` | Subsistema de física |
| `audio` | Subsistema de audio |
| `editor` | Servicios orientados al editor |
| `agent-tools` | Soporte para herramientas de IA/editor |
| `dev-full` | Build de desarrollo amplio con runtime, editor, Agent, física, audio, shader, 2D/UI, animación y skeleton |

Ejemplos:

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Herramientas del Lenguaje Varg

Ejecuta el servidor de lenguaje:

```sh
cargo xtask varg-lsp
```

Compila un `.vscene` a JSON de escena:

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

La dirección del lenguaje objetivo está documentada en [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md). Ese documento incluye sintaxis planeada más allá del runtime MVP.

## Empaquetar un Proyecto

Empaqueta el proyecto de ejemplo predeterminado para el host de escritorio actual:

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

El paquete de carpeta se escribe bajo el proyecto, por ejemplo:

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

Incluye binario runtime, script lanzador, payload copiado del proyecto, manifiesto de assets y `package-manifest.json`.

Estado actual del empaquetado:

| Target | Soporte actual |
|---|---|
| `native`, `linux-x64`, `windows-x64`, `macos-universal` | Paquetes `folder` en hosts de escritorio compatibles |
| `android-arm64` | Existe validación de toolchain; generación de APK/AAB firmado aún no implementada |
| `ios-universal` | Existe validación de toolchain; generación de IPA firmado aún no implementada |
| Instaladores de escritorio (`appimage`, `deb`, `rpm`, `exe`, `msi`, `nsis`, `dmg`) | La CLI los reconoce, pero el empaquetado de proyectos Varg actualmente devuelve unsupported capability |

## Tests y Checks

```sh
cargo test --workspace
cargo xtask check
cargo fmt --check
cargo clippy --workspace

cargo test -p runtime-min --no-default-features --features runtime-min
cargo test -p engine-render-wgpu
cargo test -p engine-editor --no-default-features --features agent-tools

pytest scripts/tests
```

## Documentación

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md): dirección del lenguaje de autoría Varg y notas del subconjunto MVP.
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md): dirección del flujo de trabajo de AI Agent.
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md): referencia UI del flujo Quest.
- [`docs/adr/`](docs/adr/): registros de decisiones de arquitectura.

## Licencia

Mozilla Public License 2.0. Consulta [LICENSE](LICENSE).
