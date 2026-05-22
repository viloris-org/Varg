# Aster 游戏制作关键需求文档

更新时间：2026-05-19

## 目标

把当前 Aster 从“引擎模块骨架”推进到“可以制作、运行、调试一个小型可玩游戏”的状态。本文只整理当前项目里最影响做游戏的事项，并按阻塞程度排序。

## 当前判断

项目已经具备比较清晰的 crate 边界、场景/Prefab 数据格式、ECS 生命周期、资源注册表、渲染抽象、编辑器 UI 壳、物理/音频抽象和 CLI smoke 路径。但核心运行链路仍停在占位或 headless 层：

- `runtime-min` 只 tick headless renderer，没有真实窗口、输入、资源加载、脚本、物理、音频或游戏循环集成。
- `engine-render` 有 RenderDevice/RenderGraph 抽象，但默认是 `HeadlessRenderDevice`，Vulkan crate 默认是 stub，不能把场景真正画出来。
- `engine-editor-ui` 已有 Hub/Shell UI，但 Scene View/Game View 是 placeholder，Hierarchy/Project 也还没有绑定真实项目和场景。
- `engine-assets` 有数据库、manifest、导入队列、CPU/GPU 缓存概念，但缺少真实导入器、文件监听、运行时加载和渲染/音频/场景引用闭环。
- `engine-physics` 和 `engine-audio` 目前是完整 trait + null backend，接口方向对，但游戏功能不可用。
- 示例项目只有 manifest、scene、prefab、preferences 和 build 配置，没有可玩的资源、脚本入口或构建产物。

## P0：不做就无法“做游戏”

### 1. 可运行的游戏 Runtime

**问题**

当前 `runtime-min::RuntimeServices::tick` 只执行 render graph 和 frame counter，场景生命周期、输入、固定步长、资源加载、物理、音频、窗口事件都没有被组合成一个游戏循环。

**需求**

- 提供 `runtime-game` 运行器，能加载项目配置、默认场景并进入持续帧循环。
- 每帧顺序明确：输入采集 -> fixed update 累积 -> physics fixed update -> scene fixed lifecycle -> scene update/late update -> audio update -> render submit -> deferred destroy。
- 支持暂停、单步、退出、窗口 resize 和基础错误上报。
- `aster` 增加类似 `run <project>` 的命令，能直接运行 `examples/project`。

**验收**

- `cargo run -p aster -- run examples/project` 可以打开窗口并持续运行默认场景。
- Player 对象上的组件/脚本能收到 start/update/fixed_update。
- 关闭窗口或按 Escape 能稳定退出。

### 2. 真实渲染路径和 Game View

**问题**

渲染层目前主要是抽象和 headless/stub。没有从 Scene 到可见画面的实体提取、相机、mesh/material/texture 绑定、swapchain 呈现或编辑器 viewport texture。

**需求**

- 先选定一个首发后端。建议优先 `wgpu`，因为 `aster` 编辑器路径已经引入 `egui_wgpu::wgpu`；如果坚持 Vulkan，需要完成 `engine-render-vulkan` 的真实初始化、swapchain、pipeline 和资源上传。
- 定义最小渲染组件：Camera、MeshRenderer、Light、MaterialRef。
- 打通 Scene -> RenderWorld/RenderQueue -> RenderDevice 的提交流程。
- Game View 和 Scene View 使用真实 offscreen target，而不是 placeholder。
- 内置 debug material、基础 mesh、默认 shader，保证没有资产时也能渲染测试对象。

**验收**

- 示例场景打开后能看到相机视角中的一个几何体。
- Scene View/Game View 都显示真实渲染结果。
- resize 不崩溃，资源销毁延迟队列可验证。

### 3. 输入系统和玩家控制

**问题**

`engine-platform::input` 只有少量事件枚举，没有输入状态、轴映射、鼠标按钮、滚轮、手柄，也没有接入 runtime update。

**需求**

- 建立 `InputState`：pressed/released/down、mouse delta、wheel、cursor position。
- 支持动作映射：例如 `MoveForward = W/Up/GamepadLeftY`。
- winit 事件转换到引擎输入事件，并在每帧 reset transient 状态。
- Runtime 和脚本/组件能查询输入。

**验收**

- 示例 Player 可用 WASD 或方向键移动。
- 输入状态在帧边界正确更新，按下/释放只触发一帧。

### 4. 场景组件序列化和 Inspector 编辑

**问题**

Scene 文件目前保存 GameObject 元数据、Transform、脚本 proxy，但普通组件、渲染组件、物理组件、音频组件没有统一序列化/反序列化机制。编辑器 Inspector 也没有真实属性编辑。

**需求**

- 定义组件 schema/registry：组件类型 ID、字段元数据、默认值、序列化格式、迁移策略。
- Scene/Prefab 文件支持组件列表，并能实例化到 ECS。
- Inspector 根据 schema 显示和编辑 Transform、Camera、MeshRenderer、Rigidbody、Collider、AudioSource、Script。
- Play Mode 使用编辑态拷贝，退出后不污染编辑态。

**验收**

- 在 JSON 场景里声明 MeshRenderer/Camera/Rigidbody 后，运行时能恢复组件。
- Inspector 修改 Transform 后，Scene View 立即反映。
- Prefab 实例化保留组件数据。

### 5. 资源导入、加载和热更新闭环

**问题**

资产层的数据结构已存在，但缺少实际导入器和运行时加载流程。游戏制作必须能把图片、模型、材质、音频、shader 从磁盘变成可用资源。

**需求**

- 最小支持：PNG/JPEG texture、glTF model、material JSON/TOML、WGSL 或 GLSL shader、WAV/OGG audio。
- 项目扫描生成/维护 `.meta` 和 manifest。
- ImportQueue 实际读取文件，产出 CPU resource，并排队 GPU upload。
- 编辑器 Project 面板显示资产树、导入状态、错误诊断和缩略图。
- 文件修改后标记 stale，并触发重导入/重上传。

**验收**

- 把一张图片放进项目 assets 后，Project 面板能显示并预览。
- 材质引用 texture 后，示例 mesh 能使用该材质渲染。
- 修改图片文件后无需重启即可刷新。

## P1：决定游戏是否可用、可调试

### 6. 物理后端和 ECS 同步

**问题**

物理已有 trait、Collider/Rigidbody 描述和 null backend，但不能模拟、碰撞或查询。

**需求**

- 接入一个真实后端，建议 Rapier 作为首个实现。
- Rigidbody/Collider 组件和 PhysicsWorld 之间建立创建、销毁、同步、事件分发。
- 支持 raycast、overlap、trigger enter/exit、collision enter/exit。

**验收**

- Player 可在地面上移动并被碰撞体阻挡。
- trigger 区域能触发事件。

### 7. 脚本或游戏逻辑扩展

**问题**

Scene 中已有 `ScriptComponentProxy`，但没有 backend。只靠 Rust 原生组件对使用者门槛较高，也不利于编辑器工作流。

**需求**

- 明确首个脚本方案：Rust hot-reload、Lua、Python 或 Rhai 只能先选一个。
- 脚本组件能接收 lifecycle、输入、Transform、spawn/destroy、资源访问和物理查询。
- 脚本错误进入 Console，并能定位到文件/行。

**验收**

- 示例 `player_controller` 能驱动 Player。
- 脚本错误不会让编辑器崩溃，Console 能展示诊断。

### 8. 编辑器打开项目和保存场景

**问题**

Hub 和 Shell 已有状态/UI，但 `open` 目前只显示 Hub，LaunchEditor action 未真正切换/加载项目；Project/Hierarchy/Inspector 多数是空状态。

**需求**

- Hub 新建/打开项目后进入 Editor screen。
- EditorShell 持有当前 ProjectContext、Scene、AssetDatabase。
- Hierarchy 绑定 Scene 对象树；Project 绑定 assets；Console 绑定诊断；Toolbar 的 Play/Pause/Stop 调用 Scene play mode。
- 保存/另存场景，保存 editor preferences。

**验收**

- 从 Hub 打开 `examples/project` 后，Hierarchy 显示 Main Camera 和 Player。
- 修改对象名或 Transform 后保存，再打开仍保留。

### 9. 构建和打包最小游戏

**问题**

CLI 有 smoke/profiles，`xtask` 入口存在，但还没有面向项目的 build/package 流程。

**需求**

- `build.runtime-min.toml` 升级为可执行的 build config。
- 产出目标目录：runtime binary、assets manifest、import cache、默认场景。
- 支持 debug/release、目标平台、资源拷贝和基础版本信息。

**验收**

- 一条命令能把 `examples/project` 打成可运行目录。
- 在不依赖源码树路径的情况下启动成功。

## P2：提升制作效率和稳定性

### 10. 调试和诊断体验

- Frame time、draw call、resource count、entity count、physics step time。
- Console 支持来源、过滤、跳转、清空、复制。
- Render/asset/script/physics 错误统一进入 diagnostics。

### 11. 编辑器交互能力

- Scene picking、outline、transform gizmo 连接真实场景和渲染。
- Undo/Redo 命令栈。
- Prefab create/apply/revert。
- 多选、复制、删除、拖拽资产到场景。

### 12. 自动化验证

- Runtime smoke 从 headless 扩展到 windowless scene simulation。
- Scene save/load golden tests 覆盖组件。
- Importer fixture tests 覆盖 texture/material/model/audio。
- Editor state tests 覆盖 open project、save scene、play mode。

## 推荐里程碑

### M1：可看见、可输入、可退出

- `runtime-game` 持续帧循环。
- winit 窗口和输入状态。
- 一个真实渲染后端能画 debug mesh。
- CLI `run examples/project`。

### M2：可编辑、可保存、可重开

- Editor 打开项目。
- Hierarchy/Inspector/Project 绑定真实数据。
- Scene 组件序列化。
- 保存场景和偏好。

### M3：可导入资源、可做一个小 demo

- texture/material/model 导入。
- MeshRenderer/Camera/Light。
- Project 面板资源树和缩略图。
- 示例场景显示 textured mesh。

### M4：可玩性基础

- 输入动作映射。
- 脚本或游戏逻辑 backend。
- 物理后端和碰撞事件。
- 示例 Player controller。

### M5：可分发

- 项目 build/package。
- 资源 manifest 随包加载。
- 基础性能和诊断面板。

## 优先级结论

最影响做游戏的不是单个模块缺失，而是缺少端到端闭环。建议先集中做 M1：`runtime-game + 窗口输入 + 真实渲染 + CLI run`。只要这个闭环跑通，后续编辑器、资产、物理、脚本都能围绕同一条可验证路径迭代，避免继续堆抽象但没有可玩结果。
