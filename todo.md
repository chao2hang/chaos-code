# 未提交改动代码审查 — TODO

> 分支：`fix/issues-13-14`
> 审查日期：2026-07-25
> 涉及 issue：#14（#11/#12/#13 已提交，此处只审 #14 工作区改动 + 附带改动）

---

## 🔴 P0 — Bug（必须修）

### 1. 未校验的 reasoning effort 值写入 config，可导致配置加载失败 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/provider.rs`
- **位置**: `parse_reasoning_meta` (~line 1050) + `upsert_provider_model_entry` (~line 669)
- **问题**:
  `parse_reasoning_meta` 把上游 `reasoningEfforts` 原样收集为 `Vec<String>`，不做校验。
  `upsert_provider_model_entry` 直接写入 config.toml。
  但 config 加载时 `reasoning_efforts` 反序列化为 `Vec<ReasoningEffortOption>`，
  bare string 经 `s.parse::<ReasoningEffort>()` —— 非标准值（如 `"turbo"`）会导致
  **整个 model 条目加载报错**。
  `reasoning_effort`（单数）同理：存 `Option<String>` 写入 config，但 config 期望
  `Option<ReasoningEffort>`。
- **对比**: `parse_remote_model_value`（`remote/client.rs:886-892`）用
  `parse_reasoning_effort_options(arr)`，跳过无效项并 warn。新代码没有复用。
- **方案**: `parse_reasoning_meta` 复用 `xai_grok_sampling_types::parse_reasoning_effort_options`
  做校验，或在写入前过滤掉 parse 失败的值。`reasoning_effort` 同理用 `s.parse::<ReasoningEffort>().ok()` 过滤。
- **完成**: `parse_reasoning_meta` 改用 `parse_reasoning_effort_options` + `ReasoningEffort` parse；
  非法值 warn 并跳过；新增 `skips_invalid_reasoning_effort_values` 测试。

---

## 🟠 P1 — 逻辑错误 / 需求未实现

### 2. "回退菜单为猜测"分支是死代码，#14.5 风险提示放错位置 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/effort.rs`
- **位置**: ~line 76-95
- **问题**:
  `offered.is_empty()` 时 `supports` 必然为 false（因为 `supports_reasoning_effort_meta` 为 true
  时 `reasoning_effort_options_for` 经 `legacy_effort_options` 保证返回非空）。
  所以 `if supports { "回退菜单为猜测" }` 分支永远走不到。
  #14.5 的风险提示应在 `offered` 非空但选项来自 legacy 回退时展示。
- **方案**: 在 `reasoning_effort_options_for` 返回值中加 `is_fallback` 标记，
  或在 `effort.rs` 中检查 meta 里有没有 `reasoningEfforts` key 来判断是否为回退。
- **完成**: `offered` 非空时用 `parse_reasoning_efforts_meta(...).is_none()` 判断 legacy 回退，
  附风险提示；有显式 menu 时不提示。

### 3. `"上下文超"` 误匹配 `"上下文超时"`（超时被当成上下文溢出） ✅

- **文件**:
  - `crates/codegen/xai-grok-sampling-types/src/error.rs` (~line 471)
  - `crates/common/xai-grok-compaction/src/code_compaction/failure.rs` (~line 44)
- **问题**:
  `m.contains("上下文超")` 匹配 `"上下文超时"`（context timeout），这是可重试的瞬态错误，
  但 `is_context_length_error` 返回 true 后重试策略将其视为终态（不重试）。
- **方案**: 改为 `(m.contains("上下文超") && !m.contains("超时"))`，
  或用更完整词组：`"上下文超限"` / `"上下文超出"`。
- **完成**: 排除「超时」；补 `"上下文超限"` / `"上下文超出"`；两侧测试增加负例。

### 4. 裸 `/effort` 不是真正的 picker ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/effort.rs`
- **问题**: Issue #14.3 要求"打开 picker"。当前实现返回 `CommandResult::Message` 文本列表，
  用户仍需手动键入 `/effort high`。注释承认是"无 picker 的 picker"。
- **方案**: 作为增量改进可接受，但关 issue 前需明确说明限制，或后续补 popup picker。
- **完成**: 文案明确「文本引导，尚无 popup picker」；本轮不实现 popup（增量可接受）。

---

## 🟡 P2 — 误导 / 不一致 / 死代码

### 5. 模型不在 catalog 时报错信息有误导 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/effort.rs` (~line 77-91)
- **问题**: 模型不在 `available` 时报"未声明 reasoning effort 支持"，但真实原因是模型未找到。
- **方案**: 先检查 `available.contains_key(&model_id)`，不在时给出不同提示。
- **完成**: 缺 catalog 时单独报「不在会话 catalog」；补测试。

### 6. `args_required()` 返回 `true` 与裸 `/effort` 返回 Message 矛盾 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/effort.rs` (~line 32)
- **问题**: `args_required` 声明需要参数，但 `run("")` 返回 Message 而非 Error。
  如果框架拦截空参数调用，run 里的空参数分支走不到；如果不拦截，`args_required` 语义有误。
- **方案**: 改为 `false`（裸 `/effort` 现在是合法调用）。
- **完成**: `args_required() -> false` + 测试。

### 7. `/provider models` 写入 config 后当前会话可能不生效 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/provider.rs`
- **问题**: 写入 config.toml 后 `ModelState.available` 仍是旧值，
  用户紧接着 `/effort` 可能还是空下拉，需重新 `/model` 或重启。
- **方案**: `register_provider_models` 成功后通知 shell 刷新 catalog entry，
  或在 `/effort` 中 fallback 重读 config。
- **完成**:
  - `models_need_catalog_sync` 标志；`load_models_for` / 深链 open 成功写入后置位；
  - `inject_provider_models_into_session` 从 config 注入 **含 reasoning meta** 的 ModelInfo；
  - SwitchModel 路径复用 `models_meta` 再 register，不再丢 meta。

### 8. `models_meta` 字段存了但没人读 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/views/provider_modal/state.rs` (~line 263)
- **问题**: `models_meta` 被填充但无 UI 组件消费。reasoning 元数据已通过
  `register_provider_models` 写入 config（source of truth）。
- **方案**: 短期无 picker 计划则删除；保留则加 `#[allow(dead_code)]` 或在 render 中展示 badge。
- **完成**: 保留字段；SwitchModel 重注册与 catalog 同步消费它；注释说明用途。

### 9. `ReasoningMeta::is_meaningful()` 仅测试使用 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/slash/commands/provider.rs` (~line 44-48)
- **方案**: 在 `upsert_provider_model_entry` 中用做早退，或删除。
- **完成**: `upsert_provider_model_entry` 用 `is_meaningful()` 包住写入逻辑。

### 10. `is_context_length_error` 两份拷贝 ✅

- **文件**:
  - `crates/codegen/xai-grok-sampling-types/src/error.rs`
  - `crates/common/xai-grok-compaction/src/code_compaction/failure.rs`
- **问题**: 注释说"Keep in sync"，但新增模式时要改两处，容易漏。
- **方案**: 让 compaction 的 `failure::is_context_length_error` 直接调用
  `xai_grok_sampling_types::is_context_length_error`，或提取到共享 crate。
- **完成**: 单一实现留在 `xai-grok-compaction`（sampling-types 依赖 compaction，
  不能反向依赖）；sampling-types 改为 `pub use` re-export。

---

## 🟢 P3 — 冗余 / 兼容性（可选）

### 11. 冗余子串匹配 ✅

- **文件**: `error.rs` + `failure.rs`
- **问题**: `contains("context window exceeds")` 被 `contains("context window exceed")` 覆盖；
  `contains("context_window_exceed")` 覆盖 `contains("context_window_exceeded")`。
- **方案**: 删掉冗余项。
- **完成**: 只保留较短 stem（实现已合并到 compaction）。

### 12. `ContextTooLarge` 消息用 `•` 做列表标记 ✅

- **文件**: `crates/codegen/xai-grok-pager/src/scrollback/blocks/session_event.rs` (~line 221)
- **问题**: `•`（U+2022）在部分终端可能渲染异常。
- **方案**: 改用 `-` 或 `*`。风险低（项目已有中文输出，大概率 UTF-8 终端）。
- **完成**: 改为 ASCII `-`。

---

## 已确认无问题

- `parse_models_response` 重构：三种 envelope 格式处理正确，排序等价，`ReasoningEffortOption` 支持 bare string 反序列化 ✓
- `agent_view/mod.rs`：用 default meta 注册模型，正确 ✓（本轮进一步改为保留 models_meta）
- `settings/ui.rs`：与 `state.rs` 同步拆分 entries，一致 ✓
- `provider.rs` 测试覆盖：7 个新测试覆盖 envelope 格式 + reasoning meta 解析 ✓（+1 invalid skip）
- `effort.rs` 测试覆盖：2 个新测试覆盖 #14.3 和 #14.4 行为 ✓（本轮再补 catalog / fallback / args_required）
- `error.rs` / `failure.rs` 测试：新增 BYOK 网关变体用例 ✓（+ 超时负例）
