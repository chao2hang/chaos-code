//! `/fallback` -- manage the fallback model chain.
//!
//! When the primary model fails (rate limit, 5xx, network), the sampler
//! tries each fallback model in order. The chain is persisted to
//! `[fallback] models` in `~/.grok/config.toml`.
//!
//! Usage:
//! - `/fallback` — show the current chain
//! - `/fallback set model1,model2,model3` — replace the entire chain
//! - `/fallback add model1,model2` — append to the end of the chain
//! - `/fallback remove model1` — remove a model from the chain
//! - `/fallback clear` — empty the chain

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Manage the fallback model chain.
pub struct FallbackCommand;

impl SlashCommand for FallbackCommand {
    fn name(&self) -> &str {
        "fallback"
    }

    fn description(&self) -> &str {
        "查看或设置备用模型链（主模型失败时按顺序尝试）"
    }

    fn usage(&self) -> &str {
        "/fallback [set|add|remove|clear] [model1,model2,...]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let args = args.trim();
        if args.is_empty() {
            return show_current_chain();
        }

        let (sub, rest) = split_subcommand(args);
        match sub {
            "set" => {
                let models = parse_models(rest);
                if models.is_empty() {
                    return CommandResult::Error("用法：/fallback set model1,model2,...".into());
                }
                match persist_fallback_models(&models) {
                    Ok(()) => {
                        CommandResult::Message(format!("备用模型链已设为：{}", models.join(" → ")))
                    }
                    Err(e) => CommandResult::Error(format!("保存失败：{e}")),
                }
            }
            "add" => {
                let to_add = parse_models(rest);
                if to_add.is_empty() {
                    return CommandResult::Error("用法：/fallback add model1,model2,...".into());
                }
                let mut current = load_fallback_models();
                for m in &to_add {
                    if !current.contains(m) {
                        current.push(m.clone());
                    }
                }
                match persist_fallback_models(&current) {
                    Ok(()) => CommandResult::Message(format!(
                        "已添加。当前备用模型链：{}",
                        if current.is_empty() {
                            "（空）".into()
                        } else {
                            current.join(" → ")
                        }
                    )),
                    Err(e) => CommandResult::Error(format!("保存失败：{e}")),
                }
            }
            "remove" => {
                let to_remove = parse_models(rest);
                if to_remove.is_empty() {
                    return CommandResult::Error("用法：/fallback remove model1".into());
                }
                let mut current = load_fallback_models();
                current.retain(|m| !to_remove.contains(m));
                match persist_fallback_models(&current) {
                    Ok(()) => CommandResult::Message(format!(
                        "已移除。当前备用模型链：{}",
                        if current.is_empty() {
                            "（空）".into()
                        } else {
                            current.join(" → ")
                        }
                    )),
                    Err(e) => CommandResult::Error(format!("保存失败：{e}")),
                }
            }
            "clear" => match persist_fallback_models(&[]) {
                Ok(()) => CommandResult::Message("备用模型链已清空。".into()),
                Err(e) => CommandResult::Error(format!("保存失败：{e}")),
            },
            _ => CommandResult::Error(format!(
                "未知子命令「{sub}」。可用：set / add / remove / clear"
            )),
        }
    }
}

fn show_current_chain() -> CommandResult {
    let models = load_fallback_models();
    if models.is_empty() {
        CommandResult::Message(
            "当前未设置备用模型链。\n用法：/fallback set model1,model2,...".into(),
        )
    } else {
        CommandResult::Message(format!(
            "当前备用模型链：{}\n\
             子命令：set（替换）/ add（追加）/ remove（移除）/ clear（清空）",
            models.join(" → ")
        ))
    }
}

fn split_subcommand(args: &str) -> (&str, &str) {
    let mut parts = args.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    (sub, rest)
}

fn parse_models(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

// ── Config persistence ───────────────────────────────────────────────

/// Load the fallback model chain from `~/.grok/config.toml` `[fallback].models`.
pub fn load_fallback_models() -> Vec<String> {
    let path = xai_grok_tools::util::grok_home::grok_home().join("config.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    doc.get("fallback")
        .and_then(|t| t.get("models"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Persist the fallback model chain to `~/.grok/config.toml` `[fallback].models`.
fn persist_fallback_models(models: &[String]) -> std::io::Result<()> {
    let path = xai_grok_tools::util::grok_home::grok_home().join("config.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    let arr = toml_edit::Array::from_iter(models.iter().map(|s| s.as_str()));
    doc["fallback"]["models"] = toml_edit::value(arr);

    std::fs::write(&path, doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_models_handles_commas_and_whitespace() {
        assert_eq!(parse_models("a, b ,c"), vec!["a", "b", "c"]);
        assert!(parse_models("").is_empty());
        assert!(parse_models(" , , ").is_empty());
    }

    #[test]
    fn split_subcommand_basic() {
        assert_eq!(split_subcommand("set a,b"), ("set", "a,b"));
        assert_eq!(split_subcommand("clear"), ("clear", ""));
        assert_eq!(split_subcommand(""), ("", ""));
    }
}
