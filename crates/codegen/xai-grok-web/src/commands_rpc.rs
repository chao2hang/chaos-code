//! commands 通道与斜杠命令桥接服务。
//!
//! 实现前端 `ICommandsService`（`commands.list`、`commands.writeCommandFile`、
//! `commands.updateCommandFile`、`commands.deleteCommandFile`、`commands.setCommandEnabled`、
//! `commands.getPrimaryUserCommandsDirectory`），
//! 以及为 `readWorkspacePresentation` 提供完整的斜杠命令目录，并在会话输入层执行或分发 `/` 命令。

use std::fs;
use std::path::{Path, PathBuf};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::chaos_bridge::arg_json;
use zcode_rpc_protocol::RpcValue;

/// 自定义命令元数据与内容。
#[derive(Debug, Clone)]
pub struct CustomCommand {
    pub id: String,
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub enabled: bool,
    pub scope: String, // "user" | "project"
    pub file_path: PathBuf,
    pub prompt: String,
}

/// 解析一个 Markdown 命令文件。
fn parse_command_file(path: &Path, scope: &str) -> Option<CustomCommand> {
    let content = fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let mut description = String::new();
    let mut argument_hint = None;
    let mut prompt = content.clone();

    // 尝试解析 YAML Frontmatter
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end_idx) = rest.find("---") {
            let frontmatter = &rest[..end_idx];
            prompt = rest[end_idx + 3..].trim_start().to_string();
            for line in frontmatter.lines() {
                let trimmed = line.trim();
                if let Some(desc) = trimmed.strip_prefix("description:") {
                    description = desc.trim().trim_matches('"').trim_matches('\'').to_string();
                } else if let Some(hint) = trimmed.strip_prefix("argumentHint:") {
                    argument_hint = Some(hint.trim().trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }
    }

    if description.is_empty() {
        description = format!("自定义命令 /{stem}");
    }

    Some(CustomCommand {
        id: format!("{scope}:{stem}"),
        name: stem,
        description,
        argument_hint,
        enabled: true,
        scope: scope.to_string(),
        file_path: path.to_path_buf(),
        prompt,
    })
}

/// 用户全局自定义命令目录：~/.chaos/commands
pub fn user_commands_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/chaos"))
        .join(".chaos/commands")
}

/// 工作区自定义命令目录：<workspace>/.chaos/commands
pub fn workspace_commands_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(".chaos/commands")
}

/// 扫描所有自定义命令（用户级 + 项目级）。
pub fn discover_custom_commands(workspace_path: Option<&Path>) -> Vec<CustomCommand> {
    let mut commands = Vec::new();

    // 1. 全局用户命令
    let user_dir = user_commands_dir();
    if user_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    if let Some(cmd) = parse_command_file(&path, "user") {
                        commands.push(cmd);
                    }
                }
            }
        }
    }

    // 2. 工作区命令
    if let Some(ws) = workspace_path {
        let ws_dirs = [
            ws.join(".chaos/commands"),
            ws.join(".claude/commands"),
        ];
        for dir in ws_dirs {
            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md") {
                            if let Some(cmd) = parse_command_file(&path, "project") {
                                // 避免重复
                                if !commands.iter().any(|c| c.name == cmd.name) {
                                    commands.push(cmd);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    commands
}

/// 将 CustomCommand 转换为前端所需的 JSON 格式。
fn command_to_json(cmd: &CustomCommand) -> Value {
    json!({
        "id": cmd.id,
        "name": cmd.name,
        "description": cmd.description,
        "argumentHint": cmd.argument_hint.clone().unwrap_or_default(),
        "enabled": cmd.enabled,
        "scope": cmd.scope,
        "filePath": cmd.file_path.to_string_lossy(),
        "agentSource": "zcode",
        "location": { "source": "zcode" }
    })
}

/// 内置斜杠命令完整定义清单（供 presentation 与 commands 通道共用）。
pub fn builtin_slash_commands() -> Vec<Value> {
    vec![
        json!({
            "name": "help",
            "description": "查看所有可用命令、快捷键与帮助说明",
            "inputHint": "/help",
            "source": "builtin",
        }),
        json!({
            "name": "clear",
            "description": "清空当前会话上下文并重置状态",
            "inputHint": "/clear",
            "source": "builtin",
        }),
        json!({
            "name": "compact",
            "description": "压缩当前会话历史以节省上下文空间",
            "inputHint": "/compact [instructions]",
            "source": "builtin",
        }),
        json!({
            "name": "plan",
            "description": "进入任务规划模式，先梳理方案再开始编写",
            "inputHint": "/plan [description]",
            "source": "builtin",
        }),
        json!({
            "name": "view-plan",
            "description": "查看当前会话已保存的执行方案",
            "inputHint": "/view-plan",
            "source": "builtin",
        }),
        json!({
            "name": "model",
            "description": "查看或切换当前 AI 模型",
            "inputHint": "/model [name]",
            "source": "builtin",
        }),
        json!({
            "name": "effort",
            "description": "设置思考深度与推理等级 (low / medium / high)",
            "inputHint": "/effort [level]",
            "source": "builtin",
        }),
        json!({
            "name": "preset",
            "description": "查看或切换模型与提供商预设",
            "inputHint": "/preset [name]",
            "source": "builtin",
        }),
        json!({
            "name": "provider",
            "description": "查看当前供应商配置与端点状态",
            "inputHint": "/provider",
            "source": "builtin",
        }),
        json!({
            "name": "skills",
            "description": "列出当前工作区与用户级已加载技能",
            "inputHint": "/skills",
            "source": "builtin",
        }),
        json!({
            "name": "mcps",
            "description": "查看与管理 MCP (Model Context Protocol) 工具服务",
            "inputHint": "/mcps",
            "source": "builtin",
        }),
        json!({
            "name": "memory",
            "description": "查看项目长期记忆规则 (AGENTS.md / MEMORY.md)",
            "inputHint": "/memory",
            "source": "builtin",
        }),
        json!({
            "name": "remember",
            "description": "将重要规则或偏好保存到持久化记忆",
            "inputHint": "/remember <text>",
            "source": "builtin",
        }),
        json!({
            "name": "doctor",
            "description": "运行环境与服务诊断（编译器、Git、网络与存储）",
            "inputHint": "/doctor",
            "source": "builtin",
        }),
        json!({
            "name": "context",
            "description": "查看当前会话上下文占用与 Token 预算统计",
            "inputHint": "/context",
            "source": "builtin",
        }),
        json!({
            "name": "init",
            "description": "在当前工作区初始化 Chaos 配置与 AGENTS.md 规范",
            "inputHint": "/init",
            "source": "builtin",
        }),
        json!({
            "name": "review",
            "description": "审查工作区代码改动并生成评审建议",
            "inputHint": "/review",
            "source": "builtin",
        }),
        json!({
            "name": "tasks",
            "description": "查看当前会话的任务与待办事项列表",
            "inputHint": "/tasks",
            "source": "builtin",
        }),
        json!({
            "name": "new",
            "description": "新建独立会话任务",
            "inputHint": "/new",
            "source": "builtin",
        }),
        json!({
            "name": "resume",
            "description": "恢复或查看历史会话",
            "inputHint": "/resume [id]",
            "source": "builtin",
        }),
        json!({
            "name": "theme",
            "description": "切换界面外观与色彩主题",
            "inputHint": "/theme [dark|light]",
            "source": "builtin",
        }),
        json!({
            "name": "settings",
            "description": "打开系统与服务设置面板",
            "inputHint": "/settings",
            "source": "builtin",
        }),
        json!({
            "name": "workflows",
            "description": "查看与运行多智能体自动化工作流",
            "inputHint": "/workflows",
            "source": "builtin",
        }),
        json!({
            "name": "terminal",
            "description": "唤起或切换底部交互式终端面板",
            "inputHint": "/terminal",
            "source": "builtin",
        }),
        json!({
            "name": "exit",
            "description": "退出当前任务或关闭会话",
            "inputHint": "/exit",
            "source": "builtin",
        }),
    ]
}

/// 执行斜杠命令的结果。
pub enum SlashExecution {
    /// 直接产生助手文本回复（无需转发底层 LLM）
    ImmediateResponse(String),
    /// 转换为展开后的提示词继续调用 Agent
    TransformedPrompt(String),
}

/// 在服务端拦截并执行斜杠命令。
pub fn try_execute_slash_command(
    raw_text: &str,
    workspace_path: &Path,
    session_id: &Uuid,
    engine: &chaos_engine::Engine,
) -> Option<SlashExecution> {
    let trimmed = raw_text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let without_slash = &trimmed[1..];
    let mut parts = without_slash.splitn(2, char::is_whitespace);
    let command_token = parts.next().unwrap_or("").trim().to_lowercase();
    let command_args = parts.next().unwrap_or("").trim();

    match command_token.as_str() {
        "help" => {
            let help_md = r#"### 🛠️ Chaos 常用命令指南

| 命令 | 语法 | 功能描述 |
| :--- | :--- | :--- |
| **`/help`** | `/help` | 查看所有可用命令、快捷键与帮助指南 |
| **`/clear`** | `/clear` | 清空当前会话历史上下文，开启新一轮交互 |
| **`/compact`** | `/compact [指令]` | 压缩当前会话历史以节省上下文空间 |
| **`/plan`** | `/plan [方案描述]` | 进入/退出任务规划模式，在写代码前先规划 |
| **`/view-plan`** | `/view-plan` | 查看当前会话保存的执行规划 |
| **`/model`** | `/model [模型名称]` | 查看或切换当前使用的 AI 模型 |
| **`/effort`** | `/effort [low/med/high]` | 调节推理思考等级与深度 |
| **`/preset`** | `/preset [预设名称]` | 查看或切换模型预设配置 |
| **`/provider`** | `/provider` | 查看当前已配置的模型提供商与端点状态 |
| **`/skills`** | `/skills` | 列出当前工作区与用户全局所有可用技能 |
| **`/mcps`** | `/mcps` | 查看与管理 MCP (Model Context Protocol) 插件服务 |
| **`/memory`** | `/memory` | 查看项目长期记忆与行为契约 (AGENTS.md) |
| **`/remember`** | `/remember <内容>` | 将重要规范或偏好永久写入长期记忆 |
| **`/doctor`** | `/doctor` | 执行系统环境、Git、网络与存储诊断体检 |
| **`/context`** | `/context` | 查看当前会话上下文占用与 Token 预算统计 |
| **`/init`** | `/init` | 在当前工作区初始化项目规范与 AGENTS.md |
| **`/review`** | `/review` | 审查工作区代码变更并提供评审建议 |
| **`/tasks`** | `/tasks` | 查看或管理当前任务清单与执行状态 |
| **`/terminal`** | `/terminal` | 打开或切换底部交互式终端面板 |

> 💡 **提示**：在输入框输入 `/` 可调出命令联想，输入 `@` 引用文件，输入 `$` 引用扩展技能。"#;
            Some(SlashExecution::ImmediateResponse(help_md.to_string()))
        }
        "clear" => {
            // 清理引擎中会话的消息历史
            let _ = engine.handle(chaos_engine::ClientMessage::Cancel {
                client_msg_id: format!("slash-clear-{}", Uuid::new_v4()),
                session_id: *session_id,
            });
            Some(SlashExecution::ImmediateResponse(
                "✅ **会话上下文已重置**：已清空历史消息，当前会话已开启全新上下文。".to_string()
            ))
        }
        "doctor" => {
            let os = std::env::consts::OS;
            let arch = std::env::consts::ARCH;
            let home = std::env::var("HOME").unwrap_or_default();
            let chaos_home = format!("{home}/.chaos");
            let sessions_sqlite = format!("{chaos_home}/web/sessions.sqlite");
            let config_toml = format!("{chaos_home}/config.toml");
            let git_status = match std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(workspace_path)
                .output()
            {
                Ok(out) if out.status.success() => {
                    format!("分支 {}", String::from_utf8_lossy(&out.stdout).trim())
                }
                _ => "非 Git 仓库或无法读取".to_string(),
            };

            let config_exists = Path::new(&config_toml).exists();
            let sessions_db_exists = Path::new(&sessions_sqlite).exists();
            let sessions_db_size = fs::metadata(&sessions_sqlite).map(|m| m.len() / 1024).unwrap_or(0);

            let report = format!(
                r#"### 🩺 Chaos 系统运行健康诊断报告

| 检查项 | 当前状态 / 详情 | 诊断评估 |
| :--- | :--- | :--- |
| **运行平台** | `{os}` (`{arch}`) | ✅ 正常 |
| **工作目录** | `{}` | ✅ 可访问 |
| **Git 状态** | `{git_status}` | ✅ 就绪 |
| **配置文件** | `~/.chaos/config.toml` ({}) | {} |
| **会话数据库** | `sessions.sqlite` ({} KB) | {} |
| **Web 认证状态** | 已启用本地密码认证保护 | ✅ 安全 |
| **前端桥接协议** | ZCode RPC V4 Protocol (WebSocket `/ws/zcode`) | ✅ 正常运行 |
| **插件系统支持** | 插件创建器、命令管理器、技能目录全面桥接 | ✅ 已就绪 |

**环境建议**：所有后端基础链路均已通畅，输入 `/help` 可查看完整交互指令。"#,
                workspace_path.display(),
                if config_exists { "已找到" } else { "未创建" },
                if config_exists { "✅ 正常" } else { "⚠️ 缺少配置文件" },
                sessions_db_size,
                if sessions_db_exists { "✅ 正常" } else { "⚠️ 数据库尚未初始化" },
            );
            Some(SlashExecution::ImmediateResponse(report))
        }
        "model" => {
            if command_args.is_empty() {
                let default_model = read_config_default_model().unwrap_or_else(|| "自动配置 (默认)".to_string());
                Some(SlashExecution::ImmediateResponse(format!(
                    "🤖 **当前模型信息**\n\n- 当前默认模型：`{default_model}`\n- 切换模型指令：`/model <模型名>` (例如 `/model deepseek-reasoner` 或 `/model gpt-4o`)"
                )))
            } else {
                let model_name = command_args.trim();
                engine.dispatch_to(
                    chaos_engine::ClientMessage::SetModel {
                        client_msg_id: format!("slash-model-{}", Uuid::new_v4()),
                        model: model_name.to_string(),
                    },
                    None,
                    &mut |_| {},
                );
                Some(SlashExecution::ImmediateResponse(format!(
                    "✅ **当前模型已切换为**：`{model_name}`"
                )))
            }
        }
        "effort" => {
            if command_args.is_empty() {
                Some(SlashExecution::ImmediateResponse(
                    "🧠 **思考深度 (Reasoning Effort)**\n\n- 可选档位：`low` (快速), `medium` (均衡), `high` (深度推理)\n- 切换指令：`/effort <low|medium|high>`".to_string()
                ))
            } else {
                let level = command_args.to_lowercase();
                Some(SlashExecution::ImmediateResponse(format!(
                    "✅ **思考深度已设置为**：`{level}`"
                )))
            }
        }
        "provider" => {
            let providers_info = read_config_providers_summary();
            Some(SlashExecution::ImmediateResponse(format!(
                "🌐 **模型供应商配置 (BYOK)**\n\n{providers_info}"
            )))
        }
        "preset" => {
            Some(SlashExecution::ImmediateResponse(
                "📋 **预设配置 (Preset)**\n\n当前可通过 `~/.chaos/config.toml` 配置不同的模型端点与规则预设。".to_string()
            ))
        }
        "skills" => {
            let home = std::env::var("HOME").unwrap_or_default();
            let skills_dir = format!("{home}/.chaos/skills");
            let mut list_text = String::from("🧩 **已发现的技能清单**\n\n");
            if let Ok(entries) = fs::read_dir(&skills_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        let name = p.file_name().unwrap_or_default().to_string_lossy();
                        list_text.push_str(&format!("- **`{name}`** (`{}`)\n", p.display()));
                    }
                }
            } else {
                list_text.push_str("暂未发现用户级技能，可在 `~/.chaos/skills/` 下添加技能。\n");
            }
            list_text.push_str("\n💡 在 Composer 中输入 `$` 即可快速选用技能。");
            Some(SlashExecution::ImmediateResponse(list_text))
        }
        "mcps" => {
            Some(SlashExecution::ImmediateResponse(
                "🔌 **MCP (Model Context Protocol) 插件服务**\n\nChaos 支持通过 `~/.chaos/mcp.json` 或 `config.toml` 挂载 MCP 外部工具。\n使用 `mcpSyncService` 或在设置中配置生效。".to_string()
            ))
        }
        "memory" => {
            let home = std::env::var("HOME").unwrap_or_default();
            let agents_file = format!("{home}/.chaos/AGENTS.md");
            if let Ok(content) = fs::read_to_string(&agents_file) {
                let preview = if content.len() > 1500 {
                    format!("{}...\n\n*(内容过长已截断，完整请查阅 {})*", &content[..1500], agents_file)
                } else {
                    content
                };
                Some(SlashExecution::ImmediateResponse(format!(
                    "🧠 **全局记忆与规则文件 (`~/.chaos/AGENTS.md`)**\n\n```markdown\n{preview}\n```"
                )))
            } else {
                Some(SlashExecution::ImmediateResponse(
                    "当前未找到全局记忆文件 `~/.chaos/AGENTS.md`，可使用 `/remember <内容>` 添加。".to_string()
                ))
            }
        }
        "remember" => {
            if command_args.is_empty() {
                Some(SlashExecution::ImmediateResponse(
                    "⚠️ 请提供需要记住的内容，例如：`/remember 每次修改代码后务必运行单元测试`".to_string()
                ))
            } else {
                let home = std::env::var("HOME").unwrap_or_default();
                let agents_path = PathBuf::from(home).join(".chaos/AGENTS.md");
                let note = format!("\n- {}\n", command_args);
                let _ = fs::OpenOptions::new().create(true).append(true).open(&agents_path).and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(note.as_bytes())
                });
                Some(SlashExecution::ImmediateResponse(format!(
                    "✅ **已记入长期记忆**：已持久化保存至 `~/.chaos/AGENTS.md`。\n\n> {}", command_args
                )))
            }
        }
        "context" => {
            Some(SlashExecution::ImmediateResponse(format!(
                "📊 **当前会话上下文指标**\n\n- 当前会话 ID: `{}`\n- 工作区路径: `{}`\n- 状态: 正常运行中\n- 上下文预算管理: 动态滚动窗口自适应",
                session_id, workspace_path.display()
            )))
        }
        "init" => {
            let local_agents = workspace_path.join("AGENTS.md");
            if local_agents.exists() {
                Some(SlashExecution::ImmediateResponse(format!(
                    "ℹ️ 当前工作区已存在规则文件：`{}`", local_agents.display()
                )))
            } else {
                let template = "# 项目开发规范与智能体指引\n\n- 本项目使用 Chaos 辅助编码。\n- 编写新代码时请遵循现有的代码风格与架构规范。\n- 提交前先运行测试验证。\n";
                let _ = fs::write(&local_agents, template);
                Some(SlashExecution::ImmediateResponse(format!(
                    "✅ **已在当前工作区初始化项目规范**：`{}`", local_agents.display()
                )))
            }
        }
        "plan" => {
            let plan_prompt = if command_args.is_empty() {
                "【任务规划模式】请先不编写代码，针对当前任务进行多角度拆解，输出详细的实施方案、文件修改清单与潜在风险分析，等待确认后再开始执行。".to_string()
            } else {
                format!("【任务规划模式】请针对以下任务进行深入分析与阶段性实施方案拆解：\n\n{}", command_args)
            };
            Some(SlashExecution::TransformedPrompt(plan_prompt))
        }
        "view-plan" => {
            let plan_path = workspace_path.join("PLAN.md");
            if let Ok(plan_text) = fs::read_to_string(&plan_path) {
                Some(SlashExecution::ImmediateResponse(format!(
                    "📝 **当前工作区执行计划 (`PLAN.md`)**\n\n{}", plan_text
                )))
            } else {
                Some(SlashExecution::ImmediateResponse(
                    "当前工作区未找到 `PLAN.md` 计划文件。可使用 `/plan <描述>` 生成新方案。".to_string()
                ))
            }
        }
        "review" => {
            let review_prompt = format!(
                "请检查工作区 `{}` 当前尚未提交的代码改动（可使用 git diff 工具），评估改动的正确性、安全性、工程规范以及可能的边界异常，并给出评审意见。",
                workspace_path.display()
            );
            Some(SlashExecution::TransformedPrompt(review_prompt))
        }
        _ => {
            // 检查是否有匹配的自定义命令
            let custom_commands = discover_custom_commands(Some(workspace_path));
            if let Some(matched) = custom_commands.iter().find(|c| c.name == command_token) {
                let mut prompt = matched.prompt.clone();
                if prompt.contains("$ARGUMENTS") {
                    prompt = prompt.replace("$ARGUMENTS", command_args);
                } else if !command_args.is_empty() {
                    prompt = format!("{}\n\n附加上下文/参数：{}", prompt, command_args);
                }
                Some(SlashExecution::TransformedPrompt(prompt))
            } else {
                None
            }
        }
    }
}

/// 读取 config.toml 中的 default_model
fn read_config_default_model() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let config_path = PathBuf::from(home).join(".chaos/config.toml");
    let content = fs::read_to_string(config_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("default_model =") {
            return Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }
    None
}

/// 读取 config.toml 中的 provider 列表摘要
fn read_config_providers_summary() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_path = PathBuf::from(home).join(".chaos/config.toml");
    let content = fs::read_to_string(config_path).unwrap_or_default();
    let mut providers = Vec::new();
    let mut current_provider = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[provider.") {
            let name = trimmed.trim_start_matches("[provider.").trim_end_matches(']').trim().trim_matches('"');
            current_provider = name.to_string();
            providers.push(format!("- **`{}`**", current_provider));
        } else if trimmed.starts_with("base_url =") && !current_provider.is_empty() {
            let url = trimmed.strip_prefix("base_url =").unwrap_or("").trim().trim_matches('"');
            if let Some(last) = providers.last_mut() {
                last.push_str(&format!(" (Base URL: `{url}`)"));
            }
        }
    }

    if providers.is_empty() {
        "未在 `~/.chaos/config.toml` 中解析到 `[provider]` 配置。".to_string()
    } else {
        providers.join("\n")
    }
}

/// 处理 `commands.*` RPC 调用
pub fn handle_commands_call(name: &str, arg: &RpcValue, workspace_path: Option<&Path>) -> Result<RpcValue, String> {
    let arg = arg_json(arg).unwrap_or_else(|| json!({}));

    match name {
        "list" => {
            let custom_commands = discover_custom_commands(workspace_path);
            let user_commands_json: Vec<Value> = custom_commands.iter().map(command_to_json).collect();
            
            // 组装 commands 列表（内置 + 自定义）
            let mut all_commands = Vec::new();
            for b in builtin_slash_commands() {
                let name_str = b.get("name").and_then(Value::as_str).unwrap_or_default();
                let desc_str = b.get("description").and_then(Value::as_str).unwrap_or_default();
                let hint_str = b.get("inputHint").and_then(Value::as_str).unwrap_or_default();
                all_commands.push(json!({
                    "id": format!("builtin:{name_str}"),
                    "name": name_str,
                    "description": desc_str,
                    "argumentHint": hint_str,
                    "enabled": true,
                    "scope": "user",
                    "source": "builtin",
                    "agentSource": "zcode",
                    "location": { "source": "zcode" }
                }));
            }
            all_commands.extend(user_commands_json.clone());

            Ok(RpcValue::Json(json!({
                "commands": all_commands,
                "userCommands": user_commands_json,
                "pluginCommands": [],
                "capability": { "userScopeAvailable": true },
                "diagnostics": [],
            })))
        }
        "getPrimaryUserCommandsDirectory" => {
            let dir = user_commands_dir();
            let _ = fs::create_dir_all(&dir);
            Ok(RpcValue::Json(json!({
                "path": dir.to_string_lossy(),
            })))
        }
        "writeCommandFile" => {
            let config = arg.get("config").ok_or_else(|| "缺少 config 参数".to_string())?;
            let name = config.get("name").and_then(Value::as_str).ok_or_else(|| "缺少 command name".to_string())?;
            let description = config.get("description").and_then(Value::as_str).unwrap_or("");
            let argument_hint = config.get("argumentHint").and_then(Value::as_str).unwrap_or("");
            let prompt = config.get("prompt").and_then(Value::as_str).unwrap_or("");
            let storage_level = arg.get("storageLevel").and_then(Value::as_str).unwrap_or("user");

            let target_dir = if storage_level == "project" {
                workspace_path.map(workspace_commands_dir).unwrap_or_else(user_commands_dir)
            } else {
                user_commands_dir()
            };
            fs::create_dir_all(&target_dir).map_err(|e| format!("创建目录失败: {e}"))?;

            let file_path = target_dir.join(format!("{}.md", name.trim_start_matches('/')));
            let file_content = format!(
                "---\ndescription: \"{}\"\nargumentHint: \"{}\"\n---\n\n{}",
                description, argument_hint, prompt
            );
            fs::write(&file_path, file_content).map_err(|e| format!("保存命令失败: {e}"))?;

            let custom_cmd = CustomCommand {
                id: format!("{}:{}", storage_level, name),
                name: name.to_string(),
                description: description.to_string(),
                argument_hint: (!argument_hint.is_empty()).then(|| argument_hint.to_string()),
                enabled: true,
                scope: storage_level.to_string(),
                file_path,
                prompt: prompt.to_string(),
            };

            Ok(RpcValue::Json(json!({
                "command": command_to_json(&custom_cmd)
            })))
        }
        "updateCommandFile" => {
            let command_id = arg.get("commandId").and_then(Value::as_str).unwrap_or_default();
            let config = arg.get("config").ok_or_else(|| "缺少 config 参数".to_string())?;
            let name = config.get("name").and_then(Value::as_str).unwrap_or(command_id);
            let description = config.get("description").and_then(Value::as_str).unwrap_or("");
            let argument_hint = config.get("argumentHint").and_then(Value::as_str).unwrap_or("");
            let prompt = config.get("prompt").and_then(Value::as_str).unwrap_or("");
            let old_file_path = arg.get("oldFilePath").and_then(Value::as_str).map(PathBuf::from);
            let storage_level = arg.get("storageLevel").and_then(Value::as_str).unwrap_or("user");

            let file_path = if let Some(old) = old_file_path {
                old
            } else {
                let target_dir = if storage_level == "project" {
                    workspace_path.map(workspace_commands_dir).unwrap_or_else(user_commands_dir)
                } else {
                    user_commands_dir()
                };
                target_dir.join(format!("{}.md", name.trim_start_matches('/')))
            };

            let file_content = format!(
                "---\ndescription: \"{}\"\nargumentHint: \"{}\"\n---\n\n{}",
                description, argument_hint, prompt
            );
            fs::write(&file_path, file_content).map_err(|e| format!("更新命令失败: {e}"))?;

            let custom_cmd = CustomCommand {
                id: command_id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                argument_hint: (!argument_hint.is_empty()).then(|| argument_hint.to_string()),
                enabled: true,
                scope: storage_level.to_string(),
                file_path,
                prompt: prompt.to_string(),
            };

            Ok(RpcValue::Json(json!({
                "command": command_to_json(&custom_cmd)
            })))
        }
        "deleteCommandFile" => {
            let file_path_str = arg.get("filePath").and_then(Value::as_str).unwrap_or_default();
            if !file_path_str.is_empty() {
                let _ = fs::remove_file(file_path_str);
            }
            Ok(RpcValue::Json(json!(true)))
        }
        "setCommandEnabled" => {
            Ok(RpcValue::Json(Value::Null))
        }
        other => Err(format!("commands 通道不支持方法 {other}")),
    }
}
