# Varg

[![CI](https://github.com/viloris-org/Varg/actions/workflows/core.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/core.yml)
[![Nightly](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml/badge.svg)](https://github.com/viloris-org/Varg/actions/workflows/nightly.yml)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.96+-orange.svg)

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | [日本語](README.ja.md) | 한국어 | [Español](README.es.md)

Varg는 Rust 런타임, Tauri/React 데스크톱 에디터, AI 지원 제작 워크플로를 중심으로 만든 실험적 게임 엔진 및 에디터입니다. 현재 코드베이스는 안전한 ECS/런타임 기반, 네이티브 에디터 셸, Varg 제작 언어, 프로젝트 패키징, Quest/Copilot 스타일의 에디터 자동화에 집중하고 있습니다.

이 프로젝트는 아직 pre-1.0 단계입니다. 일부 문서는 목표 설계를 설명하며, 이 README는 현재 저장소에 실제로 반영된 내용을 기준으로 합니다.

![Varg 에디터](docs/screenshots/editor.png)

## 빠른 시작

필수 조건:

- [Rust](https://rustup.rs/) 1.96 이상
- 에디터 프런트엔드를 위한 [Bun](https://bun.sh/)
- [Tauri v2 시스템 의존성](https://v2.tauri.app/start/prerequisites/)

Debian/Ubuntu 계열 Linux에서는 보통 다음 Tauri 의존성이 필요합니다:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

저장소를 클론하고 에디터를 실행합니다:

```sh
git clone https://github.com/viloris-org/Varg
cd Varg

cd editor
bun install
bun run dev:tauri
```

Rust workspace를 빌드합니다:

```sh
cargo build --workspace
```

## 현재 기능

- **Rust 런타임 기반**: ECS, 프로젝트 매니페스트, 에셋, 플랫폼 입력, 렌더링 trait, WGPU 통합, 물리, 오디오, UI, 애니메이션, 스켈레톤, shader, policy, AI, 패키징 crate.
- **Tauri 에디터**: Hub/프로젝트 워크플로, viewport hosting, Copilot, Quest, 패키징, 다이얼로그, 네이티브 창/패널을 Rust 명령으로 지원하는 React/TypeScript 데스크톱 앱.
- **Varg 제작 언어**: `.varg`, `.vscene`, `.vasset` 파싱, 진단, MVP 스크립트 런타임, behavior 선언, `varg-lsp` 바이너리.
- **선언형 스크립팅 실험**: `engine-script-declarative` 아래의 JSON behavior, scene, UI, system, project, asset 구조.
- **패키징 파이프라인**: `cargo xtask package`는 데스크톱 프로젝트용 런타임 폴더를 만들고 몇 가지 향후 target/format 조합을 검증합니다.
- **안전한 Rust 정책**: 엔진 crate는 `#![forbid(unsafe_code)]`를 사용합니다.

## 프로젝트 구조

```text
Varg/
├── crates/                         # 엔진 및 런타임 crate
│   ├── engine-core/                # ID, 오류, 수학, 설정
│   ├── engine-ecs/                 # 씬, 엔티티, transform, 컴포넌트
│   ├── engine-assets/              # 에셋 DB, importer, manifest
│   ├── engine-render/              # 렌더링 trait와 공유 렌더 모델
│   ├── engine-render-wgpu/         # WGPU backend와 viewport 실험
│   ├── engine-platform/            # 창, 입력, 파일 시스템 추상화
│   ├── engine-script-varg/         # Varg parser, 진단, 런타임, LSP
│   ├── engine-script-declarative/  # 선언형 JSON 제작 실험
│   ├── engine-editor/              # 에디터 서비스와 AI/도구 지원
│   ├── engine-packager/            # 프로젝트 패키징 파이프라인
│   └── runtime-min/                # 런타임 구성 루트
├── editor/                         # Tauri/React 데스크톱 에디터
├── examples/                       # 예제 프로젝트, behavior, script
├── docs/                           # 설계 노트, PRD, ADR
├── schema/                         # JSON schema
├── scripts/                        # 유틸리티 스크립트와 테스트
└── xtask/                          # workspace 자동화 명령
```

## 에디터 개발

```sh
cd editor
bun install

bun run dev:tauri
bun run build
bun run tauri build
```

자주 쓰는 경로:

- Renderer UI: `editor/src/renderer/`
- Tauri 명령과 host 서비스: `editor/src-tauri/src/`
- Tauri 권한: `editor/src-tauri/capabilities/`

## 런타임 Feature

`runtime-min`은 구성 crate입니다. Feature 세트는 루트 `Cargo.toml`의 workspace metadata에도 정리되어 있습니다.

| Feature | 용도 |
|---|---|
| `runtime-min` | 최소 headless 런타임 경로 |
| `runtime-game` | 에셋 import와 window 지원을 포함한 런타임 경로 |
| `wgpu` | WGPU 렌더링 backend |
| `physics` | 물리 서브시스템 |
| `audio` | 오디오 서브시스템 |
| `editor` | 에디터용 서비스 |
| `agent-tools` | AI/에디터 도구 지원 |
| `dev-full` | 런타임, 에디터, Agent, 물리, 오디오, shader, 2D/UI, 애니메이션, 스켈레톤을 포함한 넓은 개발 빌드 |

예:

```sh
cargo build -p runtime-min --no-default-features --features runtime-min
cargo build -p runtime-min --no-default-features --features runtime-game,wgpu,physics,audio
cargo xtask runtime-min
cargo xtask build-editor
```

## Varg 언어 도구

언어 서버 실행:

```sh
cargo xtask varg-lsp
```

`.vscene` 파일을 scene JSON으로 컴파일:

```sh
cargo xtask vscene compile path/to/input.vscene --out path/to/output.scene.json
```

목표 언어 방향은 [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md)에 있습니다. 이 문서에는 MVP 런타임을 넘어서는 예정 문법도 포함되어 있습니다.

## 프로젝트 패키징

현재 데스크톱 host용 기본 예제 프로젝트를 패키징합니다:

```sh
cargo xtask package --project examples/project/fps_arena --target native --format folder --debug
```

폴더 패키지는 프로젝트 아래에 작성됩니다. 예:

```text
examples/project/fps_arena/exports/<project>/<target>/<channel>/
```

런타임 바이너리, 런처 스크립트, 복사된 프로젝트 payload, 에셋 manifest, `package-manifest.json`을 포함합니다.

현재 패키징 상태:

| Target | 현재 지원 |
|---|---|
| `native`, `linux-x64`, `windows-x64`, `macos-universal` | 일치하는 데스크톱 host에서 `folder` 패키지 생성 |
| `android-arm64` | 툴체인 검증 있음. 서명된 APK/AAB 생성은 아직 미구현 |
| `ios-universal` | 툴체인 검증 있음. 서명된 IPA 생성은 아직 미구현 |
| 데스크톱 installer(`appimage`, `deb`, `rpm`, `exe`, `msi`, `nsis`, `dmg`) | CLI는 인식하지만 Varg 프로젝트 패키징은 현재 unsupported capability를 반환합니다 |

## 테스트와 검사

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

## 문서

- [`docs/varg-language-family-spec.md`](docs/varg-language-family-spec.md): Varg 제작 언어 방향과 MVP subset 설명.
- [`docs/ai-agent-unified-spec.md`](docs/ai-agent-unified-spec.md): AI Agent 워크플로 방향.
- [`docs/quest-workflow-ui-reference.md`](docs/quest-workflow-ui-reference.md): Quest 워크플로 UI reference.
- [`docs/adr/`](docs/adr/): 아키텍처 결정 기록.

## 라이선스

Mozilla Public License 2.0. 자세한 내용은 [LICENSE](LICENSE)를 참고하세요.
