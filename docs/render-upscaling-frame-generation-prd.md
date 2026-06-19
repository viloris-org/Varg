# Aster 超分与插帧渲染能力 PRD

状态：Draft  
目标版本：分阶段交付  
最后更新：2026-06-20

## Problem Statement

Aster 已经有 `engine-render` 抽象、`engine-render-wgpu` 后端、动态分辨率配置和 4K/120 Hz 运行时性能目标，但当前渲染输出仍主要假设“内部渲染分辨率接近最终输出分辨率”。这对桌面高刷、掌机、移动端和未来 XR/云游戏都不够：

- 高分辨率和高刷新率会快速吃掉 GPU、带宽、功耗和热预算。
- 移动端设备通常更受限于 tile memory、带宽、温控、驱动能力和电池。
- 桌面端玩家已经预期 DLSS、FSR、XeSS、DirectSR、MetalFX 等主流超分能力。
- 新一代厂商方案正在把超分、反走样、插帧、多帧生成、低延迟和神经渲染绑定在一起，但硬件、平台、SDK 和授权差异很大。
- 插帧能提高显示流畅度，但会放大输入延迟、UI 合成、present pacing、motion vector、遮挡和后处理顺序问题。
- 如果直接把某一家 SDK 接进 `engine-render-wgpu`，Aster 会过早绑定到单一平台和厂商，移动端也会被边缘化。

Aster 需要先定义平台无关的超分与插帧能力模型，让游戏内容和编辑器设置面向“能力、质量档位和预算”，而不是面向某个厂商 SDK。移动端必须从第一阶段开始作为核心目标，不能等桌面实现完成后再补。

## Solution

Aster 将建立一个跨平台的 Render Scaling and Frame Generation 能力层。该能力层负责：

- 将内部渲染分辨率、显示输出分辨率、UI 分辨率和截图/录制分辨率解耦。
- 定义统一的超分输入数据：color、depth、motion vectors、exposure、jitter、near/far、render size、display size、reactive/transparency masks、frame history。
- 定义统一的插帧输入数据：连续帧 color、depth、motion vectors、UI composition policy、present timing、latency markers、frame generation multiplier。
- 提供 runtime capability negotiation，按平台、后端、GPU、驱动、SDK、功耗和温度选择可用方案。
- 提供稳定的 editor/project settings，让游戏可以声明目标质量和移动端策略。
- 提供保守 fallback：native、dynamic resolution、built-in spatial scaler、built-in temporal upscaler。

技术支持分为三层：

1. **Aster 内置通用层**
   - Native render scale。
   - Dynamic Resolution Scaling。
   - 简单 spatial upscaler。
   - Temporal upscaler/TAA-ready path。
   - 可选的轻量移动端 upscaler。

2. **开放或较易跨平台的集成**
   - AMD FSR 1/2/3。
   - AMD FSR Upscaling / FSR 4 / Redstone，按 SDK 和硬件能力启用。
   - Intel XeSS SR/FG/MFG。
   - Apple MetalFX Upscaling / Frame Interpolation，用于 iOS、iPadOS、macOS、tvOS、visionOS。
   - Qualcomm Snapdragon Game Super Resolution / GSR，作为 Android/Windows on Arm 移动与掌机方向的候选。
   - Arm 或 GPU 厂商提供的移动端超分方案，作为 backend-specific adapter，不污染公共 API。

3. **平台或厂商专用集成**
   - NVIDIA DLSS Super Resolution、DLAA、Frame Generation、Multi Frame Generation、Ray Reconstruction。
   - NVIDIA Streamline，作为 Windows 桌面聚合入口候选。
   - Microsoft DirectSR，作为 D3D12/Windows 多厂商超分入口候选。
   - 平台驱动级 Auto SR / driver override，仅作为检测和兼容性信息，不作为 Aster 的核心渲染路径。

## Goals

- 让 Aster 在桌面、掌机和移动端都能以更低内部渲染成本输出高质量画面。
- 将移动端作为 P0 设计约束，覆盖 Android、iOS/iPadOS、Windows on Arm 和移动 GPU 热预算。
- 为 DLSS、FSR、XeSS、MetalFX、GSR、DirectSR 和 Streamline 预留稳定集成边界。
- 先交付可靠超分，再交付插帧；插帧不得破坏输入响应、UI 可读性或 frame pacing。
- 让项目作者可以按平台设置质量档位、目标 FPS、最低 render scale、功耗策略和 fallback。
- 让编辑器能显示当前 active upscaler、render scale、输出分辨率、GPU 时间、插帧倍率和延迟状态。
- 保持 headless 测试和无 GPU CI 可验证公共配置、能力选择和 fallback 行为。

## Non-Goals

- 第一阶段不承诺接入所有厂商 SDK。
- 第一阶段不实现 DLSS/FSR/XeSS/MetalFX/GSR 的完整生产集成。
- 第一阶段不承诺多帧生成或插帧质量。
- 不把 Aster 场景、材质或 UI 绑定到某个厂商 SDK。
- 不把驱动控制面板里的外部覆盖功能视为引擎内置支持。
- 不在移动端强行启用高成本 ML upscaler；移动端必须尊重功耗和温度。

## Supported Technology Matrix

| 技术 | 分类 | 主要平台 | 支持策略 |
| --- | --- | --- | --- |
| Native + Dynamic Resolution | 内置基础能力 | 全平台 | P0 |
| Built-in Spatial Upscaler | 内置 fallback | 全平台 | P0 |
| Built-in Temporal Upscaler / TAA path | 内置时域能力 | 全平台 | P1 |
| AMD FSR 1 | 空间超分 | PC、掌机、部分移动/主机类环境 | P1 候选，作为低门槛 fallback |
| AMD FSR 2/3 | 时域超分/插帧 | PC、掌机、支持的 Vulkan/DX 环境 | P1/P2 候选 |
| AMD FSR Upscaling / FSR 4 / Redstone | ML 超分/神经渲染套件 | 主要 PC/掌机，RDNA 4 最优 | P2+，硬件和 SDK 能力检测 |
| NVIDIA DLSS SR/DLAA | AI 超分/反走样 | RTX 桌面/笔记本 | P2+，Windows 优先 |
| NVIDIA DLSS FG/MFG | 插帧/多帧生成 | RTX，MFG 依赖新硬件 | P3+，需低延迟和 present pacing 成熟 |
| NVIDIA Streamline | 多厂商集成框架 | Windows 桌面 | P2+ 候选 |
| Intel XeSS SR | AI/DP4a 超分 | Intel Arc/iGPU，部分跨厂商 GPU | P2 候选 |
| Intel XeSS FG/MFG | 插帧/多帧生成 | 主要 Windows/DX12，能力依 SDK | P3+ 候选 |
| Microsoft DirectSR | D3D12 多厂商 SR API | Windows/D3D12 | P2+，仅 D3D12 backend 可用 |
| Apple MetalFX Upscaling | 平台超分 | iOS、iPadOS、macOS、tvOS、visionOS | Mobile P1 |
| Apple MetalFX Frame Interpolation | 平台插帧 | Apple Metal 4 支持设备 | Mobile P3 |
| Qualcomm Snapdragon GSR | 移动/Arm 超分 | Snapdragon Android、Windows on Arm | Mobile P1/P2 候选 |
| Arm/MediaTek/Samsung GPU vendor SR | 移动厂商能力 | Android SoC | Mobile P2+ 调研和适配 |
| OS/Driver Auto SR | 外部覆盖 | Windows/驱动支持设备 | 检测/说明，不作为核心路径 |

## Mobile-First Requirements

- Android 和 iOS/iPadOS 必须拥有明确的 render scale、dynamic resolution、thermal policy 和 battery policy。
- 移动端默认优先稳定帧时间和温控，不追求短时间峰值画质。
- 移动端必须支持 30/40/45/60/90/120 FPS 目标档位，具体可用值按设备刷新率协商。
- 移动端必须支持更激进的 internal resolution lower bound，例如 50% 以下是否可用由项目配置决定。
- 移动端 UI、文本、HUD 和触控反馈不得被低质量插帧或超分破坏。
- 移动端应优先使用平台原生能力：MetalFX on Apple、Snapdragon GSR on Qualcomm、Vulkan/厂商扩展 on Android。
- Android 后端必须假设 GPU、驱动和扩展碎片化，不允许公共 API 依赖单一 SoC。
- iOS/iPadOS 后端必须利用 Metal 的平台能力，同时保持与 `wgpu` 公共渲染抽象隔离。
- 移动端必须暴露 thermal throttling、GPU time、render scale、upscaler mode 和 dropped frame telemetry。
- 插帧在移动端默认关闭，只有在延迟、UI 合成和功耗策略满足条件时才允许启用。

## User Stories

1. As a mobile player, I want the game to maintain stable frame pacing under thermal pressure, so that the game remains playable during long sessions.
2. As a mobile player, I want the game to lower internal resolution instead of stuttering, so that controls remain responsive.
3. As a mobile player, I want UI and text to stay sharp, so that touch controls and HUD information remain readable.
4. As an iPhone player, I want the game to use MetalFX when available, so that Apple GPUs can render efficiently.
5. As an Android player, I want the game to use Snapdragon GSR or another available mobile upscaler when supported, so that my phone can trade resolution for performance.
6. As a handheld player, I want balanced upscaling presets, so that battery life and image quality can be tuned.
7. As a PC player, I want DLSS, FSR, or XeSS options when my GPU supports them, so that I can choose the best image/performance tradeoff.
8. As a PC player, I want DirectSR or Streamline-backed options to appear only when valid, so that settings do not expose broken choices.
9. As a competitive player, I want frame generation to be optional and clearly separated from super resolution, so that I can prioritize input latency.
10. As a casual player, I want frame generation to improve perceived smoothness when appropriate, so that high-refresh displays look better.
11. As a graphics programmer, I want one capability negotiation layer, so that vendor SDK checks do not spread through the renderer.
12. As a graphics programmer, I want the renderer to produce motion vectors and depth consistently, so that temporal upscalers receive valid inputs.
13. As a graphics programmer, I want UI composition policy to be explicit, so that generated frames do not smear HUD or editor overlays.
14. As a graphics programmer, I want per-platform adapters, so that MetalFX, DirectSR, Streamline and GSR can coexist without contaminating the core API.
15. As an engine developer, I want headless tests for capability selection, so that CI can verify fallback behavior without GPU SDKs.
16. As an engine developer, I want render targets to know internal and output size separately, so that dynamic resolution and upscalers are first-class.
17. As an engine developer, I want quality presets expressed in normalized project settings, so that platforms can map them to vendor-specific modes.
18. As an engine developer, I want latency telemetry around frame generation, so that performance gains do not hide responsiveness regressions.
19. As an editor user, I want the viewport to show current render scale and upscaler, so that visual changes are understandable.
20. As a project owner, I want per-platform defaults, so that mobile builds can use conservative settings while desktop builds use high-end features.
21. As a QA engineer, I want golden scenes for motion, particles, alpha, UI and disocclusion, so that upscaler artifacts can be compared.
22. As a release engineer, I want proprietary SDKs behind feature flags and platform gates, so that licensing and package size remain controlled.

## Implementation Decisions

- Add a render scaling capability model to `engine-render`, independent from `wgpu`, D3D12, Metal or Vulkan.
- Represent super resolution and frame generation as separate capabilities. A backend may support one without the other.
- Keep render resolution, display resolution and UI composition resolution separate in public configuration.
- Add explicit frame data contracts for temporal rendering: camera jitter, previous view/projection, motion vectors, depth, exposure and history invalidation.
- Add render graph concepts for pre-upscale, upscale, post-upscale and UI composition stages.
- Add an `UpscalerBackend`-style deep module interface that can be implemented by built-in, FSR, DLSS, XeSS, DirectSR, Streamline, MetalFX and mobile vendor adapters.
- Add a `FrameGenerationBackend`-style interface later, after upscaling and frame pacing are stable.
- Store user-facing quality modes as engine modes: Native, UltraQuality, Quality, Balanced, Performance, UltraPerformance and Auto.
- Map engine modes to vendor modes inside adapters, not in project content.
- Treat mobile thermal and battery policy as inputs to automatic mode selection.
- Require every adapter to provide capability reason strings, so editor UI can explain unavailable options.
- Require all proprietary SDK integrations to be optional features and excluded from default open-source builds unless licensing permits bundling.

## Required Render Data Contract

Super resolution backends must be able to request:

- Low-resolution color input.
- Output color target.
- Depth buffer.
- Per-pixel motion vectors.
- Exposure or luminance metadata.
- Camera jitter and frame index.
- Previous frame matrices.
- Reactive/transparency mask where supported.
- Reset/history invalidation flag.
- Render size and output size.

Frame generation backends must additionally request:

- Current and previous resolved frames.
- Optical-flow or generated motion input where required by the vendor.
- UI/HUD composition policy.
- Present timing and display refresh metadata.
- Low-latency markers where supported.
- Generated-frame multiplier.
- Screenshot/recording policy for generated frames.

## Platform Strategy

### Desktop Windows

- `wgpu` remains the current practical backend for Aster.
- DirectSR requires a D3D12 path or native handle integration; do not assume it works through portable `wgpu` APIs.
- Streamline/DLSS/XeSS/FSR integrations must be isolated behind backend-specific adapters.
- Frame generation should not ship until the runtime can measure latency, queue depth and present pacing.

### macOS and iOS/iPadOS

- MetalFX is the primary Apple-platform candidate.
- Apple platforms need a Metal-capable backend boundary. If `wgpu` cannot expose required MetalFX integration points cleanly, the adapter should live in a native Metal backend.
- Mobile Apple support is not a desktop afterthought; iPhone and iPad quality presets must be designed alongside macOS.

### Android

- Vulkan is the likely long-term graphics path for advanced vendor upscalers.
- Snapdragon GSR is a first mobile candidate, but the public API must also support non-Qualcomm devices.
- Thermal policy, memory bandwidth and UI readability are P0 Android concerns.
- Built-in dynamic resolution and spatial/temporal upscaling must work even when no vendor SDK is available.

### Windows on Arm and Handhelds

- Treat Windows on Arm as both desktop and mobile-adjacent.
- Snapdragon X/G devices may expose driver or platform upscaling controls; Aster should detect and document interactions but not rely on driver overrides for correctness.
- Handheld presets should bias toward battery, thermals and stable frame pacing.

## Testing Decisions

- Test public configuration serialization and defaults without a GPU.
- Test capability negotiation with fake adapters for supported, unsupported and partially supported devices.
- Test quality-mode mapping without invoking vendor SDKs.
- Test render scale bounds, automatic mode selection and fallback behavior.
- Add golden/render fixture scenes for camera motion, skinned/animated objects, particles, alpha materials, UI overlays and disocclusion.
- Add benchmark scenes that report internal resolution, output resolution, GPU frame time and upscaler mode.
- Add mobile profile simulations for thermal throttling and battery saver mode.
- Add frame-generation-specific tests only after frame generation backend work begins.

## Rollout Plan

### Phase 0: PRD and Data Contract

- Agree on terminology, supported technology matrix and mobile-first requirements.
- Define public render scaling settings and capability structures.
- Document vendor SDK licensing and platform constraints.

### Phase 1: Built-In Scaling Foundation

- Split internal render size from output size across render targets and runtime metrics.
- Add built-in spatial upscale fallback.
- Extend dynamic resolution to mobile-oriented policies.
- Expose editor/runtime telemetry.

### Phase 2: Temporal Inputs

- Generate motion vectors and camera jitter.
- Add history invalidation and previous-frame metadata.
- Build or integrate a basic temporal upscaler/TAA path.

### Phase 3: Mobile Vendor Path

- Prototype MetalFX on Apple platforms where backend access permits.
- Prototype Snapdragon GSR or equivalent Android mobile upscaler when SDK access is confirmed.
- Validate thermal and power behavior on real devices.

### Phase 4: Desktop Vendor Path

- Prototype FSR 2/3 or FSR Upscaling depending on SDK maturity and backend compatibility.
- Investigate DirectSR for a D3D12 backend path.
- Investigate Streamline/DLSS and XeSS behind optional features.

### Phase 5: Frame Generation

- Add frame generation adapter boundary.
- Implement latency and frame pacing telemetry.
- Prototype FSR/XeSS/DLSS/MetalFX frame generation only after UI composition and input latency policies are stable.

## Out of Scope

- Shipping a production DLSS, FSR 4, XeSS, MetalFX or GSR implementation in the first PRD milestone.
- Making claims about vendor certification before legal/licensing review.
- Supporting frame generation in editor viewport before game runtime.
- Supporting generated frames in deterministic offline render tests.
- Replacing the current render backend choice solely to chase one vendor SDK.

## Further Notes

- NVIDIA documents DLSS as a neural rendering suite that includes Super Resolution, DLAA, Frame Generation and Multi Frame Generation, with Streamline positioned as a cross-IHV integration route.
- AMD documents current FSR Upscaling as the ML-powered successor to the former FSR 4 naming, with SDK support that also includes FSR 2/3 era paths and Redstone technologies.
- Microsoft DirectSR standardizes super resolution for D3D12 and exposes DLSS Super Resolution, FSR and XeSS through a common code path where drivers support it.
- Apple documents MetalFX Upscaling, Frame Interpolation and Denoising as Apple-platform performance technologies, with Metal 4 support on recent Apple devices.
- Intel's XeSS SDK releases include SR, FG and MFG capabilities, but platform/API support varies by version and GPU.
- Mobile support should be validated on physical devices early; desktop GPU benchmarks do not predict mobile thermals.

## References

- NVIDIA DLSS: https://developer.nvidia.com/rtx/dlss
- AMD FSR Upscaling: https://gpuopen.com/amd-fsr-upscaling/
- Microsoft DirectSR preview: https://devblogs.microsoft.com/directx/directsr-preview/
- Apple Metal: https://developer.apple.com/metal/
- Intel XeSS SDK releases: https://github.com/intel/xess/releases
