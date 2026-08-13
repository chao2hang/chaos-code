# [DRAFT — 未发布] microsoft/WSL issue 起草稿

发布前请:
1. 在 WSL 里运行 `bash scripts/collect-wsl-info.sh > wsl-info.txt`，把输出填入下方待补字段。
2. 决定标题措辞。
3. 决定是否附上 `dmesg` 完整输出(建议只截关键几行,勿泄漏 hostname 等)。

审核通过后可用 `gh issue create -R microsoft/WSL --title "..." --body-file docs/known-issues/wsl-p9io-issue-draft.md` 提交。

---

## Title (candidates)

- `AcceptAsync cancelation in p9io triggers immediate WSL shutdown under heavy small-file I/O`
- `WSL2 self-shutdowns with "Operation canceled @p9io.cpp:258 (AcceptAsync)" during cargo build`

## Body

### Environment

- Host OS: **Windows __ (build __)** ← 待补
- `wsl.exe --version`: ← 待补(内核 / WSLg / MSRDC / Windows)
- WSL kernel (in-guest `uname -a`):
  `Linux CPC 6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Thu Jun 5 18:30:46 UTC 2025 x86_64 GNU/Linux`
- Distro: Debian/Ubuntu 系,systemd 启用
- `.wslconfig`:
  ```
  [wsl2]
  networkingMode=Mirrored
  firewall=false
  memory=52428800000
  ```
  (≈ 48.8 GiB on a 47 GiB host — this is a data point, not necessarily the root cause.)
- Physical memory: 47 GiB total, 45 GiB free at the moment of crash.
- Rootfs: ext4 on `/dev/sdd`, 1 TB drive, 447 GB used.

### Summary

During a heavy Rust workspace build (`cargo build --release` + concurrent `cargo check`, with a `target/` directory around 150 GB and thousands of small file writes per second), WSL2 self-shutdowns without warning. It happened 3 times in a row across ~2 minutes (19:52, 19:53, 19:54 local), each time recovering into a fresh boot on its own.

The signature is always the same and always fires ~21–89 seconds into boot, correlating with when I resumed the paused `cargo` job in the previous session's terminal:

```
[   21.664257] Exception:
[   21.664640] Operation canceled @p9io.cpp:258 (AcceptAsync)
[   22.901943] systemd-journald[39]: Received SIGTERM from PID 1 (systemd-shutdow).
```

Second and third occurrence times: `[42.238 s]` and `[89.229 s]`, same lines.

`last -x reboot`:

```
Tue Jul 28 19:54   still running
Tue Jul 28 19:54 → 19:54  (00:00)
Tue Jul 28 19:53 → 19:53  (00:00)
Tue Jul 28 19:52 → 19:53  (00:00)
Tue Jul 28 18:26 → 19:53  (01:26)   ← last clean session
```

Nothing OOM-killer-related in `dmesg`; no kernel oops; the shutdown is initiated by WSL's user-mode side (`systemd-shutdow` is PID 1 acting on a `SIGTERM` from the WSL init service, not the guest kernel).

### Reproduction (approximate — not minimal yet)

1. On WSL2 rootfs (ext4, not `/mnt/c`), have a Rust workspace with `target/` ≥ 100 GB.
2. In one terminal, `cargo build --release` on the whole workspace.
3. In a second terminal, `cargo check -p <one-crate> --all-targets`.
4. Push memory pressure by having `memory=` in `.wslconfig` near the host's physical limit.

The bug reproduces reliably on my machine while the concurrent cargo jobs are churning through `target/debug/incremental/` and `.rmeta` writes.

I have not yet reduced this to a synthetic reproducer (e.g. `fio` on a large tempfs tree). If wanted, I can try.

### What I checked

- No OOM killer entries.
- No `dxg` fatals correlated in time (the `dxgkio_query_adapter_info: Ioctl failed: -22` lines are startup-only noise).
- No `EXT4-fs` errors before shutdown.
- 9P (`p9io.cpp`) is a Windows-side component of WSL, not in `microsoft/WSL2-Linux-Kernel`. So filing here rather than the kernel repo.
- After each cycle, `target/debug/incremental/` was corrupted enough that the next `cargo check` panicked with an rustc ICE:
  ```
  error: the compiler unexpectedly panicked. this is a bug.
  try_mark_green dep node stack:
  #0 check_mod_deathness(...)
  ```
  This is only a downstream symptom of the abrupt shutdown, not evidence of a compiler bug.

### Ask

- Is there a known bound on 9P `AcceptAsync` outstanding channels that heavy small-file I/O can exhaust?
- Would a more recent WSL kernel / mainline WSL update fix this? My kernel string is `2025-06-05`; I'd upgrade proactively but want to confirm this is a known fixed issue first.
- Any diagnostic knob to log the reason `AcceptAsync` was canceled (e.g. queue overflow vs. host-side memory pressure vs. IPC handle exhaustion)?

Happy to provide more logs. Please point me at what to capture — I have not yet reset the environment, but any subsequent boot will lose in-memory state.
