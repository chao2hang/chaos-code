//! terminal 通道：Web 端真 PTY。
//!
//! 官方壳的 `ITerminalService`（`terminal.create/write/resize/dispose` +
//! `onDynamicData/onDynamicExit` 动态事件）映射到 `portable-pty`：每条 WebSocket
//! 会话持有一个 [`TerminalHub`]，socket 断开时 hub 被 drop，pty 主端随之关闭、
//! 子 shell 收到 HUP 退出，不残留孤儿进程。
//!
//! 事件回传走会话循环的 `tokio::select!`：读线程把 PTY 输出塞进无界 mpsc，
//! 循环侧按 terminal id 找到 `onDynamicData(id)` 监听的 rpc id 后 EventFire。

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

/// 一条 PTY 输出/退出事件，从读线程送回会话循环。
pub enum TerminalEvent {
    Data { terminal_id: String, bytes: Vec<u8> },
    Exit { terminal_id: String, exit_code: i32 },
}

struct TerminalSession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

/// 每条 WebSocket 会话一个的 PTY 注册表。
pub struct TerminalHub {
    sessions: Mutex<HashMap<String, TerminalSession>>,
    events: UnboundedSender<TerminalEvent>,
    next_id: std::sync::atomic::AtomicU64,
}

impl TerminalHub {
    pub fn new(events: UnboundedSender<TerminalEvent>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            events,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// `terminal.create`：解析 shell 与 cwd，起读线程/退出观察线程。
    pub fn create(&self, cols: u16, rows: u16, cwd: Option<&str>) -> Result<Value, String> {
        let shell = resolve_shell();
        let cwd = resolve_cwd(cwd);
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .to_string();

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("无法创建 PTY: {error}"))?;

        let mut command = CommandBuilder::new(&shell);
        command.cwd(&cwd);
        // 继承当前进程环境（systemd unit 已带 PATH），再强制交互终端能力位。
        #[cfg(unix)]
        for (key, value) in std::env::vars_os() {
            use std::os::unix::ffi::OsStrExt;
            if key.as_bytes().contains(&0) || value.as_bytes().contains(&0) {
                continue;
            }
            command.env(&key, &value);
        }
        #[cfg(not(unix))]
        for (key, value) in std::env::vars() {
            command.env(&key, &value);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("无法在 PTY 中启动 {shell}: {error}"))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("无法读取 PTY 输出: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("无法写入 PTY: {error}"))?;

        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let event_tx = self.events.clone();
        let data_id = id.clone();
        std::thread::Builder::new()
            .name(format!("web-pty-{id}"))
            .spawn(move || {
                let mut reader = reader;
                let mut buffer = [0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if event_tx
                                .send(TerminalEvent::Data {
                                    terminal_id: data_id.clone(),
                                    bytes: buffer[..read].to_vec(),
                                })
                                .is_err()
                            {
                                break; // 会话已关闭
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| format!("无法启动 PTY 读线程: {error}"))?;

        let exit_tx = self.events.clone();
        let exit_id = id.clone();
        let exit_alive = alive.clone();
        std::thread::Builder::new()
            .name(format!("web-pty-exit-{id}"))
            .spawn(move || {
                let mut child = child;
                let exit_code = child
                    .wait()
                    .map(|status| status.exit_code() as i32)
                    .unwrap_or(-1);
                exit_alive.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = exit_tx.send(TerminalEvent::Exit {
                    terminal_id: exit_id,
                    exit_code,
                });
            })
            .map_err(|error| format!("无法启动 PTY 观察线程: {error}"))?;

        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id.clone(),
                TerminalSession {
                    writer: Mutex::new(writer),
                    master: Mutex::new(pair.master),
                    alive,
                },
            );

        Ok(json!({
            "id": id,
            "shell": shell,
            "fontFamily": "ui-monospace, SFMono-Regular, SF Mono, Menlo, Monaco, Consolas, monospace",
            "fontSize": 13,
            "fontFamilySource": "fallback",
            "windowsPty": null,
        }))
    }

    /// `terminal.write`：把浏览器键入写进 PTY 主端。
    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = sessions
            .get(id)
            .ok_or_else(|| format!("Terminal not found: {id}"))?;
        let mut writer = session
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|error| format!("写入终端失败: {error}"))
    }

    /// `terminal.resize`：TIOCSWINSZ，内核给子 shell 发 SIGWINCH。
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = sessions
            .get(id)
            .ok_or_else(|| format!("Terminal not found: {id}"))?;
        session
            .master
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("调整终端尺寸失败: {error}"))
    }

    /// `terminal.dispose`：杀子进程并移除会话；读线程随 fd 关闭自然结束。
    pub fn dispose(&self, id: &str) {
        let session = {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            sessions.remove(id)
        };
        if let Some(session) = session {
            session
                .alive
                .store(false, std::sync::atomic::Ordering::SeqCst);
            // 显式取回 master 所有权再丢弃：portable-pty 的 master drop 即关主端，
            // 子 shell 收到 HUP 退出；读线程随 fd 关闭自然结束。
            drop(
                session
                    .master
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
}

impl Drop for TerminalHub {
    fn drop(&mut self) {
        // socket 断开：清掉全部 PTY 会话。子 shell 靠 HUP 退出。
        let ids: Vec<String> = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.dispose(&id);
        }
    }
}

fn is_executable(command: &str) -> bool {
    if command.contains('/') || command.contains('\\') {
        return std::fs::metadata(command)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
            && is_x_ok(command);
    }
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| {
                let candidate = dir.join(command);
                is_x_ok(&candidate.to_string_lossy())
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_x_ok(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_x_ok(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

/// `$SHELL` 可能残留坏路径；不可执行时按 zsh/bash/sh 回退（与上游一致）。
fn resolve_shell() -> String {
    let candidates = [
        std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.trim().is_empty()),
        Some("/bin/zsh".to_owned()),
        Some("/bin/bash".to_owned()),
        Some("/bin/sh".to_owned()),
    ];
    for candidate in candidates.into_iter().flatten() {
        if is_executable(&candidate) {
            return candidate;
        }
    }
    "/bin/sh".to_owned()
}

/// cwd 可能已被删除/移动；不可用时回 HOME，再回根目录（与上游一致）。
fn resolve_cwd(requested: Option<&str>) -> PathBuf {
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty());
    let candidates: Vec<Option<&str>> = vec![requested, home.as_deref(), Some("/")];
    for candidate in candidates.into_iter().flatten() {
        let path = Path::new(candidate);
        if path.is_dir() {
            return path.to_path_buf();
        }
    }
    PathBuf::from("/")
}
