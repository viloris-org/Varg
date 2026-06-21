# LLM-Optimized Scripting Proposal

> Current language-family decisions, file roles, and canonical extensions are tracked in
> [`docs/aster-script-family-spec.md`](./aster-script-family-spec.md).

## 问题
如果 Aster 引擎的主要用户是 LLM 而非人类开发者，当前的 Rhai 命令式脚本可能不是最优选择。

## LLM 代码生成的特点

### LLM 的优势
1. **结构化数据生成**：擅长生成 JSON、树形结构
2. **模式识别**：容易复制和组合已知模式
3. **声明式语法**：比命令式更少出错
4. **高级抽象**：宁可用"PatrolPath"也不要手写寻路循环

### LLM 的弱点
1. **状态管理**：容易在复杂状态转换中出错
2. **边界条件**：off-by-one、空指针等低级错误
3. **循环不变量**：难以保证复杂循环的正确性
4. **底层操作**：手动内存管理、指针运算等

## 推荐方案：分层设计

### 方案 A：保留 Rhai + 添加声明式层（渐进式）

**优势**：
- 向后兼容
- 人类开发者仍可用 Rhai 做复杂逻辑
- LLM 使用高级声明式 API

**实现**：
```rust
// 在 engine-ai 或新的 engine-script-declarative crate 中
pub struct DeclarativeBehavior {
    entity_id: String,
    behavior_tree: BehaviorNode,
}

pub enum BehaviorNode {
    Sequence(Vec<BehaviorNode>),
    Selector(Vec<BehaviorNode>),
    Condition(ConditionExpr),
    Action(ActionExpr),
    Parallel(Vec<BehaviorNode>),
}
```

**LLM 生成 JSON，引擎编译为行为树**：
```json
{
  "type": "Sequence",
  "children": [
    {
      "type": "Condition",
      "check": {"input": "keyPressed", "key": "W"}
    },
    {
      "type": "Action",
      "do": {"transform": "translate", "offset": [0, 0, 1]}
    }
  ]
}
```

### 方案 B：新增 DSL（推荐用于 LLM）

创建一个简单的领域特定语言，专门为 LLM 优化：

```javascript
// aster_ai.script - 新语法，专为 LLM 设计
entity Player {
  // 声明式状态
  state {
    health = 100
    speed = 5.0
  }
  
  // 行为树（最适合 LLM）
  behavior {
    parallel {
      // 移动控制
      sequence {
        when input.pressed("W")
        then transform.move_forward(speed * time.delta)
      }
      
      // 跳跃控制
      sequence {
        when input.pressed("Space")
        when not state.is_jumping
        then physics.apply_impulse(y: 10.0)
      }
      
      // 生命值监控
      watch state.health {
        when below(20) then audio.play("warning")
        when equals(0) then game.game_over()
      }
    }
  }
}
```

**为什么这对 LLM 更好？**

1. **模式化**：每个块都遵循 `when-then` 模式
2. **可组合**：`sequence/parallel/selector` 清晰的语义
3. **类型安全**：`input.pressed()` 明确返回 bool
4. **声明式**：描述"要什么"而非"怎么做"
5. **易验证**：结构简单，容易静态分析

### 方案 C：纯配置 + 预定义行为库（最安全）

LLM 只生成配置，不写逻辑代码：

```yaml
# LLM 只需填充参数，不写代码逻辑
entity:
  name: Enemy
  components:
    - type: health
      max: 100
      
    - type: ai_behavior
      preset: "patrol_and_chase"
      config:
        patrol_points:
          - [0, 0, 0]
          - [10, 0, 0]
          - [10, 0, 10]
        patrol_speed: 2.0
        chase_distance: 8.0
        chase_speed: 4.0
        attack_distance: 1.5
        
    - type: combat
      preset: "melee_attacker"
      damage: 15
      cooldown: 1.5
```

所有复杂逻辑都在 Rust 中预实现，LLM 只负责配置参数。

## 实施建议

### 阶段 1：验证假设（2-4 周）
1. 用 Claude/GPT 测试生成当前 Rhai 脚本
2. 用 Claude/GPT 测试生成声明式配置
3. 对比错误率、可用性、复杂度

### 阶段 2：原型（4-6 周）
1. 在 `crates/engine-script-declarative/` 实现方案 A 或 B
2. 创建 JSON Schema 用于验证 LLM 输出
3. 实现 Rust 编译器：JSON → 行为树 → 执行

### 阶段 3：集成（2-3 周）
1. 在编辑器 UI 中添加"AI Script Mode"切换
2. 提供 LLM prompt 模板和示例
3. 实现实时预览和错误检查

## 与现有架构的集成

```rust
// 在 engine-script-declarative/src/lib.rs
pub struct DeclarativeScriptBackend {
    behavior_trees: HashMap<PathBuf, CompiledBehaviorTree>,
    rhai_fallback: Option<RhaiScriptBackend>, // 复杂逻辑回退到 Rhai
}

impl DeclarativeScriptBackend {
    pub fn load_from_json(&mut self, path: &Path) -> EngineResult<()> {
        let json = std::fs::read_to_string(path)?;
        let schema: BehaviorSchema = serde_json::from_str(&json)?;
        let tree = self.compile_behavior_tree(schema)?;
        self.behavior_trees.insert(path.to_path_buf(), tree);
        Ok(())
    }
    
    pub fn execute(&mut self, entity_id: &str, dt: f32) -> EngineResult<()> {
        // 执行行为树
    }
}
```

## 成本-收益分析

| 方案 | 开发成本 | LLM 友好度 | 功能完整性 | 性能 |
|------|---------|-----------|-----------|------|
| 保持 Rhai | 低 | ⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| A: Rhai + 声明层 | 中 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| B: 新 DSL | 高 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| C: 纯配置 | 中 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |

## 推荐

**如果 LLM 是主要用户**：
- **短期**：方案 A（添加声明式层到现有 Rhai 系统）
- **长期**：方案 B（专用 DSL）+ 保留 Rhai 作为高级用户的"逃生舱"

**关键设计原则**：
1. **声明式优于命令式**：描述"什么"而非"如何"
2. **组合优于继承**：小块组合成复杂行为
3. **配置优于代码**：能用 JSON 配置的就不要写代码
4. **类型明确**：每个操作的输入输出类型都要清晰
5. **可验证性**：提供 JSON Schema 让 LLM 输出可验证

## 示例对比

### 当前 Rhai（命令式）
```rhai
let health = 100;
let is_patrolling = true;
let patrol_index = 0;
let patrol_points = [[0,0,0], [10,0,0], [10,0,10]];

fn on_update(dt) {
    let player_pos = get_player_position();
    let self_pos = get_position();
    let dist = distance(player_pos, self_pos);
    
    if dist < 5.0 {
        is_patrolling = false;
        // 追逐玩家
        let dir = normalize(player_pos - self_pos);
        translate(dir.x * 4.0 * dt, dir.y * 4.0 * dt, dir.z * 4.0 * dt);
        
        if dist < 1.5 {
            attack_player(10);
        }
    } else if is_patrolling {
        // 巡逻逻辑
        let target = patrol_points[patrol_index];
        let dir = normalize(target - self_pos);
        translate(dir.x * 2.0 * dt, dir.y * 2.0 * dt, dir.z * 2.0 * dt);
        
        if distance(target, self_pos) < 0.5 {
            patrol_index = (patrol_index + 1) % patrol_points.len();
        }
    }
}
```

### 声明式（LLM 优化）
```json
{
  "entity": "Enemy",
  "behaviors": [
    {
      "type": "Selector",
      "children": [
        {
          "type": "Sequence",
          "name": "combat",
          "children": [
            {
              "type": "Condition",
              "check": {"player_distance": {"less_than": 5.0}}
            },
            {
              "type": "Selector",
              "children": [
                {
                  "type": "Sequence",
                  "children": [
                    {"type": "Condition", "check": {"player_distance": {"less_than": 1.5}}},
                    {"type": "Action", "do": {"attack": {"target": "player", "damage": 10}}}
                  ]
                },
                {
                  "type": "Action",
                  "do": {"chase": {"target": "player", "speed": 4.0}}
                }
              ]
            }
          ]
        },
        {
          "type": "Action",
          "do": {
            "patrol": {
              "points": [[0,0,0], [10,0,0], [10,0,10]],
              "speed": 2.0,
              "loop": true
            }
          }
        }
      ]
    }
  ]
}
```

**差异**：
- ❌ Rhai：27 行代码，包含状态管理、数学计算、边界条件
- ✅ 声明式：清晰的层次结构，无手动状态，模式化

LLM 生成第二种的成功率会显著高于第一种。

## 下一步

需要我提供：
1. ✅ 详细的 JSON Schema 定义？
2. ✅ `engine-script-declarative` crate 的骨架代码？
3. ✅ 行为树执行引擎的实现？
4. ✅ LLM prompt 模板和最佳实践？
