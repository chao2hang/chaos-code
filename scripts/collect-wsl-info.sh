#!/usr/bin/env bash
# Collects the environment facts the WSL p9io crash issue draft needs.
# Run inside WSL:  bash scripts/collect-wsl-info.sh > wsl-info.txt
# Then paste the relevant lines into docs/known-issues/wsl-p9io-issue-draft.md.
set -u

echo "### Windows version"
cmd.exe /c ver 2>/dev/null || echo "(cmd.exe ver unavailable)"
echo
echo "### wsl.exe --version"
wsl.exe --version 2>/dev/null || echo "(wsl.exe --version unavailable)"
echo
echo "### uname -a"
uname -a
echo
echo "### /proc/version"
cat /proc/version 2>/dev/null
echo
echo "### .wslconfig (sanitised — review before pasting)"
WSLCONFIG="$(wslpath 'C:\Users'"\\$(cmd.exe /c echo %USERNAME% 2>/dev/null | tr -d '\r')"'\.wslconfig' 2>/dev/null)"
if [ -z "$WSLCONFIG" ]; then
  for p in /mnt/c/Users/*/.wslconfig; do
    [ -f "$p" ] && { WSLCONFIG="$p"; break; }
  done
fi
if [ -f "$WSLCONFIG" ]; then
  sed -E 's/(password|secret|token|key)[[:space:]]*=.*/\1 = <redacted>/I' "$WSLCONFIG"
else
  echo "(.wslconfig not found)"
fi
echo
echo "### Memory"
free -h
echo
echo "### Disk (rootfs)"
df -h /
echo
echo "### Last reboots"
last -x reboot | head -10
echo
echo "### Previous boot errors (journalctl -b -1 -p err)"
journalctl -b -1 -p err --no-pager 2>/dev/null | tail -40 || echo "(journalctl -b -1 unavailable — may need sudo or boot did not persist)"
echo
echo "### Current boot p9io / shutdown signatures (dmesg)"
dmesg 2>/dev/null | grep -iE 'p9io|AcceptAsync|SIGTERM|shutdow' | tail -20 || echo "(dmesg requires sudo)"
echo
echo "### DONE — review for hostname/secrets before pasting into the issue."
