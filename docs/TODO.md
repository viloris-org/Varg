## 我的理解

我在 `feat/ai-native-engine-loop` 分支上，工作区干净。这是一个长期双轨工程：**主轨**是 AI-native Quest/Editor 方向——SceneCommand 已暴露给 AI 模型、有测试但端到端未验证，StubProvider 存在但无独立测试，AttachAsset 是空存根，前端 QuestPage/EditorPage 仍是大文件；**副轨**是 Commander/KeyPool 方向——文档已就绪但代码为零。当前主轨最紧迫的不是堆新功能，而是验证、补强、查缺补漏：补 StubProvider 测试、补 run_quest_execution 端到端测试、补 scene_command 工具调用解析测试、修复 engine-ecs 测试警告。做完一轮再切副轨。

## 第一轮：主轨 — 补测试和修复

选择最高价值切片：**StubProvider 单元测试 + 端到端测试 + 修复 AttachAsset + 修复编译警告**。