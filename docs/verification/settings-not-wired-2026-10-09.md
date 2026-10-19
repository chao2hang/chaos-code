# 设置页未接入 chaos code 的取证与接线方案（2026-10-09）

> 取证时间：2026-10-08 21:08–21:25（首轮，针对已被删除的 ZCode 前端）与
> 2026-10-09 12:20–12:52（复测，针对当时在用的 `apps/chaos-ui`）。
> 12:55 之后 `apps/chaos-ui` 与整套 TypeScript 协议镜像按决策①整体退役，
> 本文「当前在用」指的都是退役前的工作树。
> 本文的 file:line 全部按 **2026-10-09 12:52 的工作树**核对。`chaos-engine/src/lib.rs`
> 在那段时间里每 1–5 分钟被改写一次，行号会漂，判断以函数名/字符串为准。

## 1. 环境变了：两轮取证针对的不是同一套前端

| 时间 | 设置页来自哪里 | 状态 |
|---|---|---|
| 10-08 21:08 | `frontend/packages/ui`（ZCode 官方前端移植） | **该目录已不在本机**，只剩 `frontend/node_modules`；git 从未跟踪过它 |
| 10-09 12:20 | `apps/chaos-ui`（仓库自带 UI，`src/main.tsx` 1795 行 + `dist/`） | 当前在用 |

首轮结论（13 个可见分区里 1 真接入 / 2 纯前端 / 3 假接通 / 7 未接入）随前端一起作废。
`crates/codegen/xai-grok-web/src/chaos_bridge.rs`（原 `zcode_bridge.rs`，10-09 更名）仍在被维护，但它服务的
前端已经不在这台机器上，本文不再据此判断「设置有没有接入」。

## 2. 当前设置面板的真实接线状态

面板由 `apps/chaos-ui/src/settings.ts`（已随 12:55 退役）生成，9 个分类里只有 3 个带控件
（`control: 'appearance' | 'model' | 'provider' | 'none'`）。
### 2.1 真接上的

- **模型**：`update_settings` / `set_model` → `GuiSettings.model` → 每次开跑前
  `Engine::run_options()` 现读（`chaos-engine/src/lib.rs`，函数注释原文
  「a pick that only changed the label on the composer pill was no pick at all」）。
- 模式与思考强度：`SetMode` / `SetThoughtLevel` 同样进 `run_options()`。
  **但 UI 一条都没发**（`main.tsx` 发出的消息集合里没有 `set_mode`/`set_thought_level`）。

### 2.2 存了没人读（假接通）

- **Provider Base URL**：`UpdateSettings` 校验 `https://` 后写进
  `GuiSettings.base_url`、落盘、回 `SettingsUpdated`。全仓库除 `GetSettings` 回显外
  没有第二个读者；`run_options()` 只带 `model/mode/thought_level`；
  `HttpPromptAdapter` 的 `base_url/chat_endpoint/models_endpoint/api_key` 是
  `from_env()` 启动时算好的普通字段，**没有任何路径能在运行时改它**。
  改完点「保存」（`apps/chaos-ui/src/main.tsx` 的 `saveSettings`），下一轮照打老 endpoint。
- `ListSubagents` / `ListWorkflows` 的处理直接返回空 `Vec`，不是从任何真相来源读的。

### 2.3 后端有、前端问不到

- **`usage.rs` 那 1005 行当前没有任何调用方**：`ClientMessage::UsageStats`
  （带 `range`/`time_zone`）与 `ServerMessage::UsageStatsSnapshot { snapshot }` 都在
  `lib.rs` 里，注释写着「这是 Web 桥 `usage-stats.getAppUsageSnapshot` 的取数入口」，
  但 `protocol_schema.rs` 的 TS 镜像里没有 `usage_stats`，生成的
  `apps/chaos-ui/src/generated/protocol.ts` 里也没有，UI 更没有面板。
- **模型候选**：`ListProviderModels` → `ProviderModels { models, configured_model,
  reachable, detail }` 已实现且已进 TS 镜像，**UI 从不发送**；设置面板里模型是自由文本框。

### 2.4 明确只读（by design，不是缺陷）

通用（版本/协议/监听地址/状态持久化）、权限（Safe Web Mode、工作区、逐次确认）、
安全（令牌、公开域名、预览代理）、远程、更新——都由启动环境变量决定，
`settings.ts` 给每行写了 `locked` 理由。API Key 只回 `has_api_key` 布尔；
主题只进浏览器 localStorage。

## 3. 接线方案（已设计，未落地）

### A. Provider 真配置：改 endpoint/key 真的生效，并写回 `~/.chaos/config.toml`

1. `provider.rs`：把 4 个 endpoint 字段收进 `struct Endpoint`，放
   `Mutex<Endpoint>`；读侧 `current_endpoint()` **克隆**而不是持锁发请求
   （一次生成可长到 5 分钟，持锁会把设置面板的改写堵死）；锁中毒按
   「持有者死了」而不是「这里没有值」处理（沿用仓库既有 `lock_or_recover` 口径）。
2. `PromptAdapter::set_endpoint(&self, base_url, api_key) -> Result<(), String>`，
   默认实现返回错误：CLI 子进程/演示 adapter 每次调用时才解析 endpoint，
   运行时无可改之物，就如实报错，而不是回一个静默成功。
3. **凭据不跟请求跑**：`api_key = None` 只在新旧 URL 的 scheme+host+port 相同时保留旧
   key；换了主机一律不继承；空串 = 清除。这条与 `ValidateProvider` 既有约束同源
   （「sending the host's credential to whatever a page asks for is not a validation feature」）。
4. `trait base_url()` 返回类型改 `Option<String>`（锁后面借不出来），只有 1 个实现 +
   1 个调用点（`ValidateProvider` 用它拒绝探针打到非本机 endpoint）。
5. 写回配置：新增 `write_chaos_config_provider()`，只改
   `[model_providers.<默认模型解析到的那个 id>]` 段内的 `base_url` / `api_key` 两行，
   其余字节原样保留；临时文件 + `rename` 原子替换；解析不到目标段就报错，不猜。

### B. 给 `usage.rs` 接通入口

`protocol_schema.rs` / `chaos-protocol-schema` 增 `usage_stats` / `usage_stats_snapshot` 后，
换用 `chaos_engine::client_message_samples()` 与 `safe_mode_tag` 的引擎侧单一清单
（`check-gui-protocol.sh` 与 TS 镜像已随 `apps/chaos-ui` 退役）→ 设置面板加「使用统计」分类，
打开设置页时按浏览器 `Intl.DateTimeFormat().resolvedOptions().timeZone` 发一次查询。

### C. 模型改真实候选

`main.tsx` 在切到设置页时发 `list_provider_models`，把 `models` 喂给
`<input list=...>` + `<datalist>`：既给候选又允许手填未知 id；`reachable=false` 时把
`detail` 原话显示出来，不伪装成「列表为空」。

## 4. 落地被并发写入打断的经过（原样记录）

| 时间 | 事实 |
|---|---|
| 12:26:08 | `chaos-engine/src/lib.rs` 在我读取后 11 秒被改写（`protocol_schema.rs` 12:23:33、`main.tsx` 12:24:25、`usage.rs` 12:21:59） |
| 12:29 | 我对 `lib.rs`/`provider.rs`/`protocol_schema.rs`/`settings.ts`/`main.tsx`/`protocol.ts` 做了快照到 `/tmp/chaos-snapshot/` |
| 12:33–12:38 | 在 `provider.rs` 落 `Endpoint` + `Mutex` + `repoint` 共 5 处编辑；`lib.rs` 落 `base_url() -> Option<String>` 与 `set_endpoint` 默认实现 |
| 12:39:56 | `provider.rs` 被**整文件回写**：大小回到 12:29 快照的 31,948 字节，`grep -c 'struct Endpoint'` = 0。我的 5 处编辑全部消失 |
| 12:40–12:49 | `lib.rs` 继续被改写，但用的是逐处编辑，我那两处存活 → 出现 trait 声明 `Option<String>` 与实现 `Option<&str>` 不一致，crate 编不过 |
| 12:50 | 我把自己在 `lib.rs` 的那两处**原样撤销**，树恢复到我接手前的形态 |

撤销后 `cargo check -p chaos-engine --lib` 仍有 9 条错误，全部集中在
`SafeModeRefusal`（`lib.rs:1756` 把 `#[derive]` 用在了非类型项上、
`lib.rs:2027/2057` 缺 `Clone/Debug/Eq/Serialize/Deserialize`）——这是 12:50:46
那次写入留下的半成品，与本文描述的改动无关。

**结论**：`chaos-engine` 与 `apps/chaos-ui` 这两个热区同一时刻只能有一个写入者。
上面 A/B/C 三步需要同时改 `lib.rs`、`provider.rs`、`protocol_schema.rs`、
`main.tsx`、`settings.ts`，必须串行；12:29 的快照与本文的方案可以让接手的人不重复设计。

## 5. 14:26 之后的新表面，以及 B 已经落地（20:15 复核）

12:55–14:26 之间这套 Web 线被换了：`apps/chaos-ui/` 64 个文件、
`chaos-engine/src/protocol_schema.rs`、生成用的 `bin/chaos-protocol-schema.rs`
以及 4 个协议镜像闸门（`scripts/ci/check-gui-protocol.sh`、
`check-protocol-mirror.py`、`check-e2e-registration.py`、`check-brand-protocol.py`）
全部 staged 删除。前端契约现在只有一个文件：`xai-grok-web/src/chaos_bridge.rs`
（167 KB，由 `zcode_bridge.rs` 改名而来），说的是 ZCode 的二进制 RPC 帧。
所以 §3 里绑在 TS 协议上的写法作废，B 的落点改成 RPC 通道。

**已经落地并测过的（B）**：`usage-stats.getAppUsageSnapshot` 以前不在任何路由表里，
设置页打开时拿到的是 `handle_call` 末尾那句「未实现通道方法」，而引擎里那 1000 行
聚合（`Engine::app_usage_snapshot`，注释本来就写着「这是 Web 桥
`usage-stats.getAppUsageSnapshot` 的取数入口」）一个调用方都没有。补了四段：

| 位置 | 内容 |
|---|---|
| `chaos_bridge.rs` `WorkerCommand::AppUsageSnapshot` | 汇总要走引擎状态，跟其它引擎调用一样放 host 线程，不占 socket 的帧循环 |
| `chaos_bridge.rs` worker 分支 | `engine.app_usage_snapshot(&range, time_zone)`，原样转发，不加不估 |
| `chaos_bridge.rs` `delegate_to_engine` | 通道过滤加 `usage-stats`；`range` 缺省 `all`，`timeZone`/`time_zone` 都认 |
| `chaos-engine/src/lib.rs` 测试 | 一条真实轮次 → 快照：120/34/154，归到实际选中的模型头上 |

测试：`cargo test -p chaos-engine --lib` **225 passed**；
`cargo test -p xai-grok-web --lib usage_stats_tests` **3 passed**
（路由把请求交给 worker、缺省值正确、`getEntitlementSnapshot` 仍走静态答复、
worker 用引擎快照回话且空引擎就是全零）。`getEntitlementSnapshot` 故意没接：
Chaos 没有订阅配额那套东西，把引擎用量当成配额余量是造假。

## 6. 新表面上「还没接入」的清单（20:15 复核，行号按当日工作树）

| 位置 | 现状 | 判断 |
|---|---|---|
| `chaos_bridge.rs:1042` `"toolCalls": false` + `map_engine_event`（只映 TextDelta / ReasoningDelta / Completed / Cancelled）+ 全文件无 `ToolApprovalRequested` | Web 这条路看不到工具执行，也批不了工具 | **最大的一块**。声明与实现自洽，不是撒谎，但 `HeadlessProcessAdapter`（`main.rs:33`）驱动的 CLI 是真在跑工具的，能力全压在桥这一层 |
| `chaos_bridge.rs:425` `provider-settings.getView/refresh` + `setDefaultModel` | 读 `~/.chaos/config.toml`；默认模型写回会保留无关 TOML | 模型这一半是真接了的 |
| `chaos_bridge.rs:1188` `"api": { … "baseUrl": base_url }` | 只是回显运行时真相，**没有** base_url / api_key 的写通道 | 想改接入点仍然只能改配置文件重启 |
| `chaos_bridge.rs:617` `("subagents", "list") => json!([])` | 空数组，无注释 | 引擎那边同样给硬编码空表，两头都空 |
| `chaos_bridge.rs:405` `{"mode":"alwaysOn","enabled":true,"source":"remote"}` | 谎称来源是 remote，本机没有远端配置这个东西 | 措辞问题，值本身无害 |
| `chaos_bridge.rs:499` `oauth.restoreCachedSessionState` 等 | 本地身份冒充登录态，注释已写明「这不是真实的云端登录」 | 如实声明，可接受 |
| `chaos_bridge.rs:515/616` `credential.load`→Null、`client-scenes.list`→[] | 无对应功能 | 如实空 |
| `chaos_bridge.rs:1251` `setting.get/update` → `~/.chaos/web/zcode_settings.json` | 21:30 复核修正：`recentProjects` / `lastWorkspaceSession` **有读者**（`lib.rs:172` 的 `/api/server-info` 用它拼工作区列表），`locale`、`memoryEnabled`、`autoDownloadAndInstallUpdates` 等大部分键仍然写进去没人读 | 半个池子。已把写入改成原子 + 失败如实报错（见 §7），没接上的键该由前端置灰 |
| `chaos-engine/src/lib.rs` `GuiSettings` / `UpdateSettings` / `GetSettings` | 桥里 **0** 处引用（`grep -c` = 0） | 旧 chaos-ui 退役后这套引擎内设置状态成了死表面 |
| `chaos_bridge.rs:1037` `chaos_runtime_capabilities` | 明确声明 `imageInput/toolCalls/reasoningLevelControl/contextWindowControl` 全 false | 前端据此把控件置灰，是「宁缺毋假」的正解 |

## 7. 0.0.0.0 监听与访问控制实测（21:10–21:33）

不用改代码：`CHAOS_WEB_BIND` 早就支持（`lib.rs:1139`）。跑起来的是
`CHAOS_WEB_BIND=0.0.0.0 CHAOS_WEB_PORT=8787 CHAOS_WEB_TOKEN=… ./target/debug/chaos-web`，
日志 `Chaos Web listening on http://0.0.0.0:8787`，Provider 侧
`1064 model(s) listed, configured model deepseek-flash is listed`。

同一台机器上以 LAN 地址 `172.16.2.253:8787` 实测 `/ws/zcode`：

| 请求 | 结果 | 依据 |
|---|---|---|
| `Origin` 与 `Host` 同源 + 正确 Bearer | **101** 升级成功 | `origin_allowed_against` 拿 `Origin` 比请求 `Host`（`lib.rs:405`） |
| 同源、不带 token | 401 `credential_required` | `authorized()`（`lib.rs:234`） |
| `Origin` 指向本机 LAN 但 `Host` 是 `127.0.0.1` | 401 `origin_not_allowed` | 同上，伪造同源不成立 |
| 不带 `Origin`（非浏览器客户端）、不带 token | 401 | token 这一关跟浏览器无关 |

两条踩过的坑：`CHAOS_WEB_PUBLIC_ORIGIN` **只接受 https**（`must be an https: origin`，
所以想用它必须先上 TLS 反代）；纯 http 的局域网直连靠的是「Origin 与 Host 同源」这条，
浏览器从 `http://<局域网IP>:8787` 打开自己就能连上，换成别的来源一律 401。
`/health` 与 `/api/server-info` 不鉴权，后者会回本机工作区绝对路径——绑 0.0.0.0 之前
得知道这件事。

## 8. 21:00–21:35 这一轮补的三件事

1. **`authRequired` 不再撒谎**（`lib.rs:220` 原为硬编码 `false`）：现在回
   `!token.is_empty()`，并有测试 `server_info_reports_whether_a_credential_is_required`。
   之前 token 正在挡 `/ws`，`/api/server-info` 却告诉客户端「不需要凭据」。
2. **`setting.update` 落盘改成原子 + 失败上报**（`update_setting_state`）：同目录临时
   文件 + `rename`，`create_dir_all` / `write` / `rename` 任一失败都变成 promise error。
   原来三个错误全被丢掉并回 `Null`（前端理解成「已保存」）。测试
   `setting_update_persists_and_reports_a_failed_write` 覆盖「写得进去、读得回来、
   HOME 是普通文件时报错、盘上不留半截临时文件」。
3. **使用统计的端到端证据**：新增 `tests/usage_stats_flow.rs`——真 axum 端口 +
   真 WebSocket + 官方二进制帧（`Promise(100)` 帧调 `usage-stats.getAppUsageSnapshot`）
   + 引擎里真跑一轮，断言快照里的 120/34/154 就是那一轮 Provider 报的数，
   `timeZone` 按请求原样回来，token 归到实际选中的模型上。

本轮没做的：**工具事件与审批透传**。协议 crate、`frames.json`、`handshake-capture.json`
里都没有官方前端 `tool_call` 事件的任何形状依据，凭猜测定义一套 stream-event 就是
本文 §6 批评的那种造假；要做得到有真前端的机器上抓一次才能动手。

测试：`cargo test -p xai-grok-web` 全绿（lib 44 个 + 32 个集成测试二进制），
`cargo test -p chaos-engine --lib` 225 个全绿，`cargo fmt --check` 干净，
`clippy` 无新增告警。

## 9. 绑 0.0.0.0 之后暴露出来的 `file.*`（21:35–21:52，已修）

`/api/server-info` 不鉴权只是小事，真正的问题在 `file` 通道：`readdir`、`stat`、
`readTextFile`、`resolvePath`、`checkFilesExist` 五个方法以前都拿调用方给的**任意绝对
路径**直接碰文件系统。

探针（先写成断言暴露的 `#[ignore]` 测试，跑 `--ignored` 通过）：

| 调用 | 修之前 |
|---|---|
| `file.readTextFile { path: <临时目录里的文件> }` | 内容原样读回 |
| `file.readdir { path: "/" }` | 列出根目录，含 `etc` |

只听 loopback 时这约等于「本用户本来就读得到」；`CHAOS_WEB_BIND=0.0.0.0` 之后就变成
「局域网里任何拿着 token 的设备能读 `~/.chaos/config.toml`，里面是 Provider 的
api_key」。

修法：五个方法统一先过 `confined()`，根取 `CHAOS_WEB_FILE_ROOT`，否则取进程工作目录
——**跟宿主自己报给前端的那个根一致**（`ensureConversationWorkspace` 与
`/api/server-info` 说的都是 `current_dir()`），所以这不是新增约束，是把没执行的说法执行掉。
符号链接按真实目标判（根里的软链指向根外算根外）；路径还不存在时拿最近的已存在祖先判断。
`checkFilesExist` 越界时直接报错，没有回 `exists: false`——那是把越界藏起来。
启动横幅多打一行 `file.* is confined to <根>; widen with CHAOS_WEB_FILE_ROOT`，
因为等操作者在报错里看到就太晚了。

`tests/file_channel_scope.rs`（不再 ignore）四条：根外绝对路径被拒且报错点名
`CHAOS_WEB_FILE_ROOT`、`readdir("/")` 被拒、根内软链指向根外被拒、根内
`Cargo.toml` 照旧读得到。

**行为变化要知道**：`/api/server-info` 里那个 `/home/chaos/ai` 工作区现在在默认根之外，
要在界面上浏览它，启动时加 `CHAOS_WEB_FILE_ROOT=/home/chaos`。


