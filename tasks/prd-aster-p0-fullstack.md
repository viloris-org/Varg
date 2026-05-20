# PRD: Aster P0 — 骨架到可玩游戏的全栈贯通

版本 1.0 / 2026-05-21

## 1. Introduction / Overview

Aster 目前拥有清晰的 crate 边界、ECS 生命周期、资产数据库 schema、渲染抽象层、编辑器 UI 壳和物理/音频 trait。但核心运行链路全部停在占位或 headless 层——Scene View 和 Game View 画的是 placeholder 色块，Hierarchy 不绑定场景数据，Inspector 无法编辑组件，运行时没有真实窗口/输入/物理/渲染闭环。

**本 PRD 的目标**：完成 P0 级端到端贯通。编辑器能打开项目、显示真实 3D 场景、编辑组件属性、进入 Play Mode 运行带物理和脚本的游戏循环，CLI 能一键构建运行。

**总体策略：编辑器优先（Editor-First）**。Game View 承载运行时，编辑器 Shell 提供调试和编辑能力。当 Play Mode 跑通后，CLI `run` 作为独立窗口运行模式自然派生。

## 2. Goals

- 编辑器从 Hub 打开 `examples/project` 后，Hierarchy 显示场景对象树，Inspector 可编辑 Transform 等组件属性，Scene View 显示 wgpu 渲染的 3D 画面
- Game View 的 Play Mode 运行完整游戏循环：输入采集 → fixed update（物理 stepping）→ update → late update → 渲染提交
- 支持 WASD 输入移动、Rapier 物理碰撞、Rhai 脚本驱动简单 Player Controller
- PNG/glTF 资产可导入并在场景中渲染，文件修改后支持热重载
- `cargo run -p engine-cli -- run examples/project` 可独立窗口运行
- 所有 P0 链路均有自动化测试覆盖

## 3. User Stories

### Area A: 渲染后端打通（wgpu）

#### US-A01: wgpu 渲染设备初始化和 swapchain
**Description:** As a developer，我需要 `engine-render-wgpu` 提供真实的 `WgpuRenderDevice`，能创建 wgpu 实例、适配器、设备和 swapchain，替换当前的 `HeadlessRenderDevice`。

**Acceptance Criteria:**
- [ ] `WgpuRenderDevice` 实现 `RenderDevice` trait，完成 instance/adapter/device/surface/swapchain 初始化
- [ ] 在 `runtime-game` feature 下，`RuntimeServices` 默认使用 `WgpuRenderDevice`
- [ ] 窗口 resize 时重建 swapchain，不崩溃，不泄漏
- [ ] `cargo test -p engine-render-wgpu` 通过
- [ ] `cargo check --workspace --all-features` 通过

#### US-A02: 最小渲染管线 — debug mesh 和默认 shader
**Description:** As a developer，我需要一条从 Scene 中提取 `MeshRenderer` + `Camera` 组件到屏幕像素的渲染管线。

**Acceptance Criteria:**
- [ ] 内置 debug mesh（cube、sphere、plane）在无资产时可用
- [ ] 内置默认 PBR shader（WGSL），支持基础 diffuse + ambient
- [ ] `RenderWorld` 从 Scene 中提取 `Camera`、`MeshRenderer`、`Light`、`MaterialRef` 组件构建 `RenderObject` 列表
- [ ] `WgpuRenderDevice::render(RenderFrame)` 执行 clear → depth pass → opaque pass → present
- [ ] Scene View 和 Game View 显示真实渲染结果，不再显示 placeholder 色块

#### US-A03: 编辑器 Viewport 集成
**Description:** As a developer，我需要编辑器 Shell 将 Scene View/Game View 的 egui 区域映射为 wgpu render target，在 egui frame 中嵌入渲染纹理。

**Acceptance Criteria:**
- [ ] `EditorShell` 持有 `WgpuRenderDevice` 实例（通过 `RenderService`）
- [ ] Scene View 和 Game View 各自创建 offscreen `RenderTargetDesc`，渲染到 texture
- [ ] egui 通过 `egui::Image` 或 `Callback` 在对应 panel 区域显示渲染纹理
- [ ] viewport resize 时不丢帧、不崩溃

### Area B: 编辑器项目工作流

#### US-B01: ProjectContext — 从 Hub 打开项目
**Description:** As a game developer，我想从 Hub 选择项目并进入编辑器，看到场景对象树和资产列表。

**Acceptance Criteria:**
- [ ] `ProjectContext` 结构体持有 `ProjectManifest`、`Scene`、`AssetDatabase`、项目根路径
- [ ] Hub 的 "Open" 按钮加载 `aster.project.toml` → 解析 `default_scene` → 加载 Scene JSON → 扫描 `asset_root` → 构造 `ProjectContext`
- [ ] 加载失败时 Console 显示错误，保持在 Hub 界面，不崩溃
- [ ] `EditorShell` 持有 `Option<ProjectContext>`，加载成功后切换到 Editor screen

#### US-B02: Hierarchy 绑定场景对象树
**Description:** As a game developer，我想在 Hierarchy 面板中看到场景里所有 GameObject 的树形结构。

**Acceptance Criteria:**
- [ ] Hierarchy 以 parent-child 树显示 `Scene::objects`（`GameObject`），每行显示对象名和 icon（Camera、Light、Mesh、Empty 等）
- [ ] 点击行更新 `SelectionService` 为 `Selection::Entity(entity_id)`
- [ ] 支持单击重命名（inline edit）、右键上下文菜单（Delete、Duplicate、Create Child）
- [ ] 拖拽排序改变 sibling index
- [ ] 空场景显示 "No objects in scene" 提示

#### US-B03: Inspector 编辑组件属性
**Description:** As a game developer，我想选中对象后在 Inspector 中看到并编辑其 Transform 和所有 Component 属性。

**Acceptance Criteria:**
- [ ] 选中 Entity 后，Inspector 显示对象名（可编辑）、Active 开关、Tag/Layer 下拉
- [ ] Transform 区域显示 Position/Rotation/Scale 的 DragValue 控件，修改后实时反映到 Scene View
- [ ] 每个 `ComponentData`（Camera、MeshRenderer、Rigidbody、Collider、AudioSource、Light、Script）根据 `ComponentSchema` 渲染对应编辑控件
- [ ] 组件标题栏右侧有 "Remove Component" 按钮
- [ ] Inspector 底部有 "Add Component" 下拉搜索框，可添加新组件

#### US-B04: Scene 保存和加载
**Description:** As a game developer，我想 Ctrl+S 保存场景并在重新打开项目时恢复所有修改。

**Acceptance Criteria:**
- [ ] Ctrl+S / File > Save Scene 将当前 Scene 序列化为 JSON 写入 `default_scene` 路径
- [ ] 文件存在时先备份（`.bak`），写入成功后删除备份
- [ ] 重新打开项目后 Hierarchy 和 Inspector 恢复所有修改
- [ ] 首次 "Save As" 弹出文件选择对话框
- [ ] 关闭编辑器时有未保存修改时弹出确认对话框

#### US-B05: Project 面板显示资产树
**Description:** As a game developer，我想在 Project 面板中看到 `asset_root` 下的文件树，区分 texture/model/material/audio/shader/scene/prefab 类型。

**Acceptance Criteria:**
- [ ] Project 面板以树形显示 `asset_root` 目录结构
- [ ] 每个文件按扩展名显示对应的资源类型 icon
- [ ] 单击选中资产，双击打开（场景文件→加载场景，纹理→预览窗口）
- [ ] 拖拽资产到 Scene View 可创建对应 GameObject（model→MeshRenderer、texture→Sprite 等）
- [ ] 支持右键菜单（Delete、Rename、Reimport、Show in Files）

### Area C: 运行时游戏循环

#### US-C01: RuntimeServices 游戏循环
**Description:** As a developer，我需要 `RuntimeServices` 在 feature `runtime-game` 下运行完整的游戏循环：输入 → 固定步长物理 → 场景生命周期 → 渲染 → 延迟销毁。

**Acceptance Criteria:**
- [ ] 帧顺序：`input.collect()` → `fixed_timestep_accumulate()` → while accumulated: `physics.step()` + `scene.fixed_update()` → `scene.update()` → `scene.late_update()` → `render.submit()` → `scene.deferred_destroy()`
- [ ] 固定步长默认 60 Hz（`fixed_dt = 1.0/60.0`），可配置
- [ ] `max_dt` 防止螺旋式死亡（spiral of death），默认 0.1s
- [ ] Play Mode 进入时深拷贝编辑态 Scene，退出时丢弃运行态拷贝
- [ ] 暂停/单步/恢复通过 `RuntimeServices::paused` / `step_frame()` 控制
- [ ] Escape 键退出 Play Mode（在 Game View focused 时）

#### US-C02: 输入系统
**Description:** As a game developer，我需要完整的输入状态管理和动作映射。

**Acceptance Criteria:**
- [ ] `InputState` 维护：`pressed`（本帧刚按下）、`released`（本帧刚释放）、`held`（持续按住）、mouse delta、scroll delta、cursor position
- [ ] 每帧开始 reset transient 状态（pressed → held、released → up、delta 清零）
- [ ] winit `WindowEvent::KeyboardInput`/`MouseInput`/`CursorMoved`/`MouseWheel` 转换为 `InputEvent`
- [ ] `ActionMap` 支持键盘按键 → 动作名绑定（如 `MoveForward = KeyW | ArrowUp`）
- [ ] `InputState::action_pressed("MoveForward")` 供脚本和组件查询
- [ ] 手柄支持推迟到 P1

#### US-C03: Play/Pause/Stop 工具栏集成
**Description:** As a game developer，我想通过 Toolbar 按钮控制 Play Mode 并在 Game View 中看到运行结果。

**Acceptance Criteria:**
- [ ] Toolbar Play 按钮进入 Play Mode：`Scene::enter_play_mode()` + `RuntimeServices::start()`
- [ ] Play 状态下按钮切换为 Stop，点击回到编辑态
- [ ] Pause 按钮在 Play 状态下可用，暂停/恢复 fixed time accumulator
- [ ] Game View 在 Play Mode 下显示运行时渲染画面
- [ ] Scene View 在 Play Mode 下不可编辑，显示 "Play Mode Active" overlay

### Area D: 物理集成

#### US-D01: Rapier 物理后端
**Description:** As a developer，我需要 `engine-physics` 接入 Rapier 作为首个真实物理后端。

**Acceptance Criteria:**
- [ ] `RapierPhysicsBackend` 实现 `PhysicsBackend` trait
- [ ] 初始化时创建 Rapier `RigidBodySet`、`ColliderSet`、`ImpulseJointSet`、`IntegrationParameters`、`PhysicsPipeline`、`BroadPhase`、`NarrowPhase`、`CCDSolver`
- [ ] `create_body`/`destroy_body`、`create_collider`/`remove_collider` 操作 Rapier 集合
- [ ] `step(dt)` 调用 Rapier pipeline
- [ ] `raycast`/`overlap_sphere` 查询实现
- [ ] `drain_contacts` 收集碰撞/触发事件
- [ ] `LayerMatrix` 过滤碰撞层

#### US-D02: PhysicsSync — ECS ↔ PhysicsWorld 双向同步
**Description:** As a developer，我需要物理系统在每个 fixed update 中同步场景和物理世界。

**Acceptance Criteria:**
- [ ] 实体添加 `RigidbodyComponent` 时，`PhysicsSync` 在 `PhysicsWorld` 中创建 body 并回写 `BodyHandle`
- [ ] 实体添加 `ColliderComponent` 时（需有 Rigidbody），`PhysicsSync` 创建 collider 并 attach 到 body
- [ ] 实体销毁时 `PhysicsSync` 清理对应的 body/collider
- [ ] 物理 step 后，`PhysicsSync` 从 `BodyHandle` 读回 world transform 写入 `TransformHierarchy`
- [ ] 接触事件通过 `drain_contacts` → `RuntimeServices` event queue 分发给脚本/组件

### Area E: 脚本扩展

#### US-E01: Rhai 脚本后端
**Description:** As a game developer，我想用 Rhai 脚本驱动 GameObject 行为。

**Acceptance Criteria:**
- [ ] `RhaiScriptBackend` 在 `script-rhai` feature 下编译
- [ ] Scene 加载时，所有 `ScriptComponentProxy(backend="rhai")` 的脚本文件从 asset root 加载并编译
- [ ] 脚本暴露 `on_start()`、`on_update(dt)`、`on_fixed_update(fixed_dt)` 生命周期钩子
- [ ] `ScriptContext` 向 Rhai 注入 API：
  - `input::is_pressed(key)` / `input::axis(name)` / `input::mouse_delta()`
  - `transform::get_position()` / `transform::set_position(x, y, z)` 等
  - `spawn(name)` / `destroy(id)`
  - `physics::raycast(ox, oy, oz, dx, dy, dz, max_dist)`
  - `get_resource(path)`
- [ ] 脚本运行时错误推送 `ConsoleEntry`，包含文件路径和行号
- [ ] 脚本文件不存在时标记 `pending_recovery = true`，跳过生命周期调用，不崩溃
- [ ] `examples/project/assets/scripts/player_controller.rhai` 作为示例脚本

### Area F: 资产管线

#### US-F01: 最小资产导入器（PNG、glTF）
**Description:** As a game developer，我想把 PNG 贴图和 glTF 模型放入项目 assets 目录后能在场景中使用。

**Acceptance Criteria:**
- [ ] `PngImporter`：读取 PNG 文件 → 解压像素 → 生成 mip chain → 产出 `CpuTextureResource` → 排队 GPU upload
- [ ] `GltfImporter`：读取 glTF JSON + binary buffer → 提取 mesh（positions/normals/uvs/indices）→ 提取 material（base color/metallic/roughness）→ 产出 `CpuMeshResource` + `CpuMaterialResource`
- [ ] `ImportQueue` 在工作线程执行 CPU import，完成后回调 GPU upload
- [ ] 导入错误推送 `ConsoleEntry`，不 panic
- [ ] 有效 fixture 测试：valid PNG → 无诊断错误 / valid glTF → 无诊断错误
- [ ] 无效 fixture 测试：损坏 PNG → 至少一条 `AssetDiagnostic` / 不 panic

#### US-F02: 资产热重载
**Description:** As a game developer，我想修改贴图或模型文件后无需重启编辑器即可看到更新。

**Acceptance Criteria:**
- [ ] `FileWatcher`（基于 `notify` crate）监听 `asset_root` 的文件变更事件
- [ ] 文件修改后标记对应资产为 `Stale`，触发 `ImportQueue` 重新导入
- [ ] 重新导入完成后，GPU 资源自动替换（旧的入 deferred destroy 队列）
- [ ] 热重载在编辑器和 Play Mode 下均工作（Play Mode 下由开发者自行决定何时刷新）

### Area G: CLI 和构建

#### US-G01: CLI `run` 命令
**Description:** As a game developer，我想用 `cargo run -p engine-cli -- run examples/project` 在独立窗口中运行游戏。

**Acceptance Criteria:**
- [ ] `engine-cli run <project_path>` 子命令
- [ ] 加载项目配置 → 构造 `RuntimeServices`（wgpu 后端）→ 创建 winit 窗口 → 启动游戏循环
- [ ] 窗口 title 显示 `ProjectManifest.name`
- [ ] Escape 或关闭窗口正常退出，无 panic

#### US-G02: 构建打包命令
**Description:** As a game developer，我想用 CLI 一键构建可分发的游戏包。

**Acceptance Criteria:**
- [ ] `engine-cli build <project_path>` 子命令，读取 `build.runtime-min.toml`
- [ ] 调用 `cargo build` 编译 runtime binary，复制到 `<output>/bin/`
- [ ] 扫描资产 → 生成 `assets_manifest.json` → 写入 `<output>/`
- [ ] 复制默认场景到 `<output>/scenes/`
- [ ] 写入 `build_info.json`（timestamp、target、release flag、version）
- [ ] 构建失败时打印 stderr 并返回非零 exit code

### Area H: 自动化验证

#### US-H01: Windowless 场景模拟测试
**Description:** As a developer，我需要不依赖 GPU 的自动化测试来验证游戏循环。

**Acceptance Criteria:**
- [ ] `RuntimeServices` 在 headless 模式下运行 60 tick，验证实体 Transform 非初始值
- [ ] 场景中包含 `RigidbodyComponent` + `ColliderComponent` 的实体受重力下落
- [ ] headless smoke 测试保持：`smoke_runtime_min` 返回 `frame_index == 1`

#### US-H02: Scene 序列化 Golden 测试
**Description:** As a developer，我需要验证 Scene JSON 的 round-trip 一致性。

**Acceptance Criteria:**
- [ ] 包含 Camera/MeshRenderer/Rigidbody/Collider/AudioSource 的 Scene → JSON → 反序列化 → 等于原始
- [ ] 二次序列化产生 byte-identical JSON
- [ ] 所有合法 `Scene::to_json()` 输出满足 `from_json` → `to_json` round-trip

#### US-H03: 编辑器状态测试
**Description:** As a developer，我需要验证编辑器核心流程。

**Acceptance Criteria:**
- [ ] 构造 `EditorShell` → 打开 `examples/project` → Hierarchy 包含 "Main Camera" 和 "Player"
- [ ] 修改对象名 → 保存 Scene → 重新加载 → 名称保留
- [ ] Play Mode 进入 → `playing == true` → 退出 → 编辑态 Scene 未变

## 4. Functional Requirements

### FR1: 渲染
- FR1.1: `WgpuRenderDevice` 必须在 `runtime-game` 和 `editor` feature 下可用
- FR1.2: 默认 shader 必须是 WGSL，内嵌在 crate 中（不依赖外部文件）
- FR1.3: Scene View 和 Game View 必须使用独立 `RenderTargetDesc`，支持不同分辨率
- FR1.4: 渲染资源销毁必须走 deferred destroy 队列，确保 GPU 完成使用后再释放

### FR2: 编辑器
- FR2.1: `ProjectContext` 必须在主线程加载，加载期间显示进度条
- FR2.2: Hierarchy 必须支持至少 10,000 个 GameObject 不卡顿（虚拟滚动或分帧加载）
- FR2.3: Inspector 的 DragValue 修改必须通过 `UndoStack` 记录，支持 Ctrl+Z/Ctrl+Y
- FR2.4: 所有编辑器面板的显隐状态必须持久化到 `EditorPreferences.layout`
- FR2.5: Console 必须在收到新条目且用户在底部时自动滚动

### FR3: 运行时
- FR3.1: 游戏循环 fixed timestep 必须是 60 Hz，update 不限帧率
- FR3.2: 输入 transient 状态（pressed/released）生命周期精确为一帧
- FR3.3: Play Mode 的 Scene 拷贝必须是深拷贝，编辑态和运行态互不干扰
- FR3.4: Runtime 崩溃时不得让编辑器崩溃——错误必须通过 Console 上报

### FR4: 物理
- FR4.1: Rapier backend 必须在 `physics-rapier` feature 下编译
- FR4.2: 物理 stepping 必须在 fixed update 中执行，不在 render update 中
- FR4.3: 碰撞事件必须在 fixed update 后、scene update 前分发
- FR4.4: `TransformHierarchy` 必须被 PhysicsSync 更新且不覆盖非物理驱动的 Transform 修改

### FR5: 脚本
- FR5.1: Rhai backend 必须在 `script-rhai` feature 下编译
- FR5.2: 脚本文件加载失败时不得阻止其他脚本或系统运行
- FR5.3: 脚本 API 必须通过 `ScriptContext` 注入，不直接暴露 engine 内部类型
- FR5.4: 脚本中的 `spawn`/`destroy` 必须是 deferred 语义，在实际帧结束时生效

### FR6: 资产
- FR6.1: ImportQueue 必须在独立线程上执行 CPU import，避免阻塞主线程
- FR6.2: GPU upload 必须在 render thread 上执行
- FR6.3: 文件监听必须 debounce（默认 200ms），避免编辑器保存时触发重复导入
- FR6.4: 导入失败时资源标记为 `Error` 状态，不阻止其他资源加载

## 5. Architecture / Data Flow

### 5.1 系统架构图

```
┌──────────────────────────────────────────────────────────────────┐
│                      engine-cli (entry point)                    │
│  ┌─────────┐  ┌─────────────────────────┐  ┌────────────────┐  │
│  │   run   │  │        build            │  │   hub (editor)  │  │
│  └────┬────┘  └───────────┬─────────────┘  └───────┬────────┘  │
│       │                   │                        │            │
│       ▼                   ▼                        ▼            │
│  ┌────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │RuntimeServ.│  │BuildPipeline │  │   engine-editor-ui     │  │
│  │(runtime-g)│  │  (xtask)     │  │  ┌─────┐ ┌───────────┐ │  │
│  └─────┬─────┘  └──────────────┘  │  │ Hub │ │EditorShell│ │  │
│        │                          │  └──┬──┘ └─────┬─────┘ │  │
│        │                          └─────┼─────────┼───────┘  │
│        │                                │         │           │
│  ┌─────┴────────────────────────────┐   │         │           │
│  │        engine-platform           │   │  ┌──────┴─────────┐│
│  │  Window │ Input │ FS │ Library   │◄──┘  │ engine-editor  ││
│  └──────────────────────────────────┘      │ Services       ││
│                                             └────────────────┘│
│  ┌──────────────────────────────────────┐                     │
│  │          engine-ecs                  │                     │
│  │  World │ Scene │ Transform │ Schema  │                     │
│  └──────────────────────────────────────┘                     │
│  ┌──────────────────────────────────────┐                     │
│  │         engine-render                │                     │
│  │  RenderDevice │ RenderWorld │ Graph  │                     │
│  └────────────────┬─────────────────────┘                     │
│                   │                                            │
│  ┌────────────────┴─────────────────────┐                     │
│  │      engine-render-wgpu              │                     │
│  │  WgpuRenderDevice (real impl)        │                     │
│  └──────────────────────────────────────┘                     │
│  ┌──────────────┐ ┌────────────────────┐                      │
│  │engine-physics│ │   engine-audio     │                      │
│  │  + Rapier    │ │   (null backend)   │                      │
│  └──────────────┘ └────────────────────┘                      │
│  ┌──────────────────────────────────────┐                     │
│  │         engine-assets                │                     │
│  │  DB │ Manifest │ ImportQueue │ Cache │                     │
│  └──────────────────────────────────────┘                     │
│  ┌──────────────────────────────────────┐                     │
│  │    engine-script-rhai (new)          │                     │
│  │  RhaiBackend │ ScriptContext │ API   │                     │
│  └──────────────────────────────────────┘                     │
└────────────────────────────────────────────────────────────────┘
```

### 5.2 数据流：从启动到画面

```
1. CLI / Editor Hub
   │
   ▼
2. 加载 ProjectManifest (aster.project.toml)
   │  - project_name, default_scene, asset_root, build_config
   ▼
3. 加载 Scene JSON (default_scene)
   │  - GameObject[] → 实例化到 ECS World
   │  - 每个 GameObject 创建 Entity + TransformHierarchy + Components
   ▼
4. 扫描 AssetDatabase (asset_root)
   │  - 遍历文件树 → 匹配扩展名 → 创建 ResourceMeta
   │  - *.png → Texture, *.gltf → Model, *.rhai → Script
   ▼
5. ImportQueue 处理未导入资产
   │  - CPU thread: 读取文件 → 解码 → 生成 CpuResource
   │  - GPU upload: 上传 texture/mesh/shader 到 GPU
   ▼
6. 编辑器：构建 UI 面板
   │  - Hierarchy ← GameObject tree
   │  - Inspector ← ComponentData of selected entity
   │  - Project ← AssetDatabase tree
   │  - Scene View ← RenderWorld from Scene
   ▼
7. Play Mode: 深拷贝 Scene → 启动游戏循环
   │
   ├── input.collect()          ← winit events → InputState
   ├── fixed_timestep_loop:
   │     ├── physics.step(1/60)
   │     │     ├── Rapier pipeline
   │     │     └── PhysicsSync: body transforms → TransformHierarchy
   │     ├── scene.fixed_update()
   │     │     └── Rhai: on_fixed_update(dt)
   │     └── scene.physics_events()  ← drain_contacts
   │
   ├── scene.update(dt)
   │     └── Rhai: on_update(dt)
   ├── scene.late_update(dt)
   │
   ├── render.submit(frame)
   │     ├── RenderWorld::extract(scene)
   │     │     ├── Camera → RenderCamera
   │     │     ├── MeshRenderer + Transform → RenderObject
   │     │     └── Light → RenderLight
   │     ├── WgpuRenderDevice::render(frame, render_world)
   │     │     ├── shadow pass (optional, P1)
   │     │     ├── opaque pass
   │     │     └── present
   │     └── Game View 显示 rendered texture
   │
   └── deferred_destroy()
         ├── 销毁标记的 Entity
         └── 清理 GPU deferred destroy 队列
```

### 5.3 关键类型关系

```
EditorShell
  ├── ProjectContext (new)
  │     ├── project_manifest: ProjectManifest
  │     ├── scene: Scene
  │     ├── asset_db: AssetDatabase
  │     └── project_root: PathBuf
  ├── selection_service: SelectionService
  │     └── Selection::Entity(EntityId) | Asset(PathBuf) | None
  ├── console_service: ConsoleService
  ├── undo_stack: UndoStack<UndoCommand>
  ├── gizmo_service: GizmoService (for Scene View)
  ├── picking_service: PickingService (for Scene View click)
  ├── outline_service: OutlineService (for selection highlight)
  └── render_service: RenderService (new, wraps WgpuRenderDevice)

RuntimeServices (when playing)
  ├── world: World
  ├── scene: Scene (play copy)
  ├── input_state: InputState
  ├── physics_world: PhysicsWorld<RapierPhysicsBackend>
  ├── physics_sync: PhysicsSync
  ├── script_backend: RhaiScriptBackend
  ├── render_device: Box<dyn RenderDevice>
  ├── asset_database: AssetDatabase
  └── time: TimeState
```

## 6. Non-Goals (Out of Scope)

本项目 P0 阶段**不做**的事情：

- **Vulkan 后端**：P0 只做 wgpu。Vulkan stub 保留但不动。
- **Audio 真实后端**：`engine-audio` 保持 null backend。P1 再做。
- **完整 PBR 管线**：默认 shader 只需 diffuse + ambient。PBR metallic/roughness、IBL、shadow maps 推到 P1。
- **FBX/Assimp 导入器**：P0 只做 PNG + glTF。`fbx-importer` feature gate 保持不启用。
- **Python/Lua 脚本**：P0 只做 Rhai。`script-python` feature gate 保留但不动。
- **手柄/触屏输入**：P0 只做键盘 + 鼠标。
- **Prefab 工作流**：create/apply/revert prefab 是 P1 编辑器交互功能。
- **Gizmo 交互**：P0 的 Scene View 仅显示渲染画面，picking/transform gizmo 是 P1。
- **多语言完善**：i18n 系统已有，翻译文本按需补，不系统化。
- **移动端/Web 平台**：仅桌面（Linux/Windows/macOS）。

## 7. Technical Considerations

### 7.1 Feature Flag 规划

```toml
# P0 新增/修改的 feature flags
[features]
runtime-min = ["engine-core", "engine-platform", "engine-assets", "engine-ecs", "engine-render"]
runtime-game = ["runtime-min", "engine-physics/rapier", "engine-render-wgpu", "engine-script-rhai", "engine-audio"]
editor = ["runtime-game", "engine-editor", "engine-editor-ui", "engine-i18n"]
script-rhai = ["engine-script-rhai"]        # new crate
physics-rapier = ["engine-physics/rapier"]   # new feature on existing crate
dev-full = ["editor", "agent-tools", "script-python"]
```

### 7.2 新增 Crate

| Crate | 用途 | 依赖 |
|-------|------|------|
| `engine-script-rhai` | Rhai 脚本后端 | `rhai`, `engine-ecs`, `engine-platform`, `engine-assets`, `engine-physics` |

### 7.3 现有 Crate 重大改动

| Crate | 改动 |
|-------|------|
| `engine-render-wgpu` | 从骨架到完整 `WgpuRenderDevice` 实现 |
| `engine-physics` | 新增 `rapier` feature，新增 `RapierPhysicsBackend` |
| `engine-editor-ui` | `EditorShell` 新增 `ProjectContext`、`RenderService`、真实面板数据绑定 |
| `engine-editor` | `PhysicsSync` 新增、`ScriptService` 新增 |
| `runtime-min` | `RuntimeServices` 新增完整游戏循环、输入、物理、脚本集成 |
| `engine-cli` | 新增 `run`、`build` 子命令 |
| `xtask` | 新增 `BuildPipeline` |

### 7.4 依赖新增

```toml
# 新增 workspace dependencies
rhai = "1.20"              # Rhai scripting engine
notify = { version = "7.1", features = ["macos_kqueue"] }  # File watcher
```

### 7.5 性能目标

- Editor 启动到显示 Scene View：< 3 秒（含资产扫描，不含首次导入）
- 空场景帧率：> 120 fps（Release、NVIDIA GTX 1060 级别）
- 100 个 cube 场景帧率：> 60 fps
- Hierarchy 10,000 objects 滚动：60 fps（虚拟滚动）

## 8. Success Metrics

- `cargo run -p engine-cli -- run examples/project` 打开窗口并稳定运行 60 秒不崩溃
- 编辑器从 Hub 打开 `examples/project` → Hierarchy 展示对象树 → Inspector 可编辑 Transform → Scene View 显示 3D 画面（可走通）
- `cargo test --workspace` 全部通过，测试数 ≥ 30
- `examples/project` 包含可用 WASD 控制的 Player，能在地面上移动并被碰撞体阻挡
- 修改一张 PNG 贴图后，编辑器 Scene View 在 3 秒内自动刷新

## 9. Open Questions

1. **Rhai 模块系统**：脚本是否支持 `import` 其他 `.rhai` 文件？模块路径解析逻辑？
2. **Play Mode 下的资产热重载行为**：自动刷新还是手动触发？自动刷新可能导致 Play Mode 中间结果不一致。
3. **wgpu backend 选择**：优先 Vulkan/Metal/DX12 还是 DX11/OpenGL 兼容路径？建议 wgpu 默认 auto，可通过配置覆盖。
4. **Scene 文件格式**：继续保持 JSON 还是迁移到二进制/TOML？JSON 对编辑器友好但对大场景慢。P0 保持 JSON。
5. **物理重力方向和大小**：默认 (0, -9.81, 0) 还是 (0, 0, -9.81)？建议 Y-up 的 (0, -9.81, 0) 匹配大多数游戏习惯。

---

*本 PRD 基于 Aster 现有代码库的实际骨架状态编写，所有 User Story 和 Functional Requirement 都对应到已有的 struct/trait/module。实施时按 Area A→H 的顺序推进，每个 Area 完成后进行集成验证。*
