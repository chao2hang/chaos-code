# WSL2 9P shutdown loop — 2026-07-28

**状态**: 本地存档,证据留档,尚未外发。
**触发场景**: 在 chaos-code 仓库(WSL2 ext4)并发跑 `cargo build --release` 和 `cargo check -p xai-grok-shell --all-targets`,`target/` 已 ≈150 GB。
**症状**: WSL2 内核 log 抛 `Operation canceled @p9io.cpp:258 (AcceptAsync)`,systemd 立即收到 `SIGTERM from PID 1 (systemd-shutdow)`,发行版被强制重启。19:52–19:54 之间反复 4 次。

## 环境

| 项 | 值 |
|---|---|
| Windows 主机 | (未记录,补录时用 `[System.Environment]::OSVersion` + `winver`) |
| WSL 内核 | `6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun 5 18:30:46 UTC 2025` |
| WSL 编译器 | `gcc 11.2.0` |
| 发行版 | Debian/Ubuntu 系(zsh 5.x, systemd unit `zfs-load-module.service` 存在) |
| 内存 | 47 GiB 总,崩溃前 45 GiB available |
| `/dev/sdd` (ext4 rootfs) | 1007 GB total,447 GB 已用 |
| `.wslconfig` | `[wsl2] networkingMode=Mirrored, firewall=false, memory=52428800000` (≈48.8 GiB,接近物理上限) |
| 显卡直通 | dxg 存在,启动时 `dxgkio_query_adapter_info: Ioctl failed: -22`(与 crash 无关,启动噪音) |
| 项目 target 目录 | 150 GB(崩溃前含另一份 debug 构建时估计更大,可能 ≥400 GB) |

## 时间线

`last -x reboot`:

```
Tue Jul 28 19:54   还在跑
Tue Jul 28 19:54 → 19:54  (00:00)
Tue Jul 28 19:53 → 19:53  (00:00)
Tue Jul 28 19:52 → 19:53  (00:00)   ← 崩溃开始
Tue Jul 28 18:26 → 19:53  (01:26)   ← 崩溃前一次正常 boot
```

`dmesg` 每次 boot 都有同一条前置签名(3 次都命中同一行号 `p9io.cpp:258`):

```
[   21.664257] Exception:
[   21.664640] Operation canceled @p9io.cpp:258 (AcceptAsync)
[   22.901943] systemd-journald[39]: Received SIGTERM from PID 1 (systemd-shutdow).
```

第二次(`42.238` 秒)和第三次(`89.229` 秒)间隔递增,说明 host 侧 WSL service 有 backoff。

## 假设

按证据强度排:

1. **9P AcceptAsync 在文件树剧烈变动时失稳**。cargo 一次 build 会创建/覆写数万个 `.rmeta`/`.o`/`incremental/` 小文件,9P 服务器要为每一次 `open/mkdir/rename` 建立 accept 通道。上面的 exception 明确来自 `AcceptAsync`,不是数据传输阶段。
2. **`memory=52428800000` 几乎榨干物理内存**。Windows 侧 host 处理 9P 请求也要页表,vmmem 涨到接近上限后 Windows 可能触发内存回收,拖慢 9P 通道超时。
3. **WSL 内核已过时**(2025-06-05 build,今天是 2026-07)。半年后可能已有修复。

第一条与第二条可能耦合:大量并发 I/O 期间 host 内存吃紧 → 9P 队列长度爆掉 → AcceptAsync canceled → WSL 主动 shutdown。

## 后遗症

- `~/.chaos` 里当前会话的多个后台 `cargo` 任务 ID 从 chaos-code CLI 里丢失了(重启后进程表清空)。
- `target/debug/incremental/` 状态被破坏,下次 `cargo check` 触发 rustc ICE:
  ```
  error: the compiler unexpectedly panicked. this is a bug.
  try_mark_green dep node stack:
  #0 check_mod_deathness(xai_grok_tools[..]::grok_build::scheduler::occurrence_journal)
  ```
  修复: `rm -rf target/debug/incremental`。

## 待补充

发外部 issue 前需要补录。**一键收集脚本**：`bash scripts/collect-wsl-info.sh > wsl-info.txt`（在 WSL 里运行，输出含 Windows 版本、`wsl --version`、`.wslconfig`、内存、磁盘、`last reboot`、`journalctl -b -1 -p err`、`dmesg` 签名）。脚本会尝试脱敏，但粘贴前请再人工核对 hostname / 密钥。

- [ ] Windows 版本号 (`ver` 或 `winver`)
- [ ] `wsl.exe --version` 完整输出(内核版本、WSLg 版本、MSRDC 版本)
- [ ] 最小复现步骤(能否用 `fio` / 大量 `touch` 复现,而非依赖 chaos-code)
- [ ] 崩溃时是否有 `\\wsl$\` 或 `\\wsl.localhost\` 的 Windows 侧文件浏览
- [ ] `journalctl -b -1 -p err` 从上一个 boot 抓完整错误(现在只留了 `-b 0`)

## 关联的可行 workaround

- **降低 `.wslconfig` `memory`** 到 32 GiB 或 40 GiB,给 host 留缓冲。
- **搬 target 到 non-WSL 挂载**(如 `\\wsl$\` 之外),或在 `Cargo.toml` 用 `target-dir` 单独放 tmpfs。
- **升级 WSL 内核**: PowerShell 里 `wsl --update`。
- **压 rustc 并发**: `CARGO_BUILD_JOBS=8` 或 `-C codegen-units=<小值>`,减少并发文件写入。
- 单窗口测试时避免同时跑 `build --release` + `check`。

## 参考

- `p9io.cpp` 在 microsoft/WSL2-Linux-Kernel 里并不存在,该文件属于 WSL 的 Windows 侧组件(mm/WSL user-mode service),因此 issue 应发到 `microsoft/WSL`(不是 `WSL2-Linux-Kernel`)。
- Similar historical reports: `microsoft/WSL#9231` 类别下有多起 `AcceptAsync` 触发 shutdown 的 case(需要复核最新状态)。
