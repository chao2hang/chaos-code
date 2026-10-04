#!/usr/bin/env python3
"""Fail when committed instructions tell a reader to run a command that cannot work.

Two other gates read prose. `check-evidence-paths.py` rejects a document that points at
a session-private scratch directory, and `check-doc-path-refs.py` rejects a document that
points at a file that is not in the repository. Neither of them reads a captured
transcript: `check-evidence-paths.py` says in its own docstring that `docs/verification/
*.log` is deliberately not scanned, because a transcript quotes the absolute path the
command actually used and rewriting it would falsify it. So the one kind of pointer a
maintainer is most likely to copy-paste, a command line, was read by nothing.

What that hid, found on 2026-10-04:

    docs/verification/protocol-mirror-coverage-2026-10-03.log:61   bash scripts/ci/check-gui-protocol.sh
    docs/verification/protocol-mirror-coverage-2026-10-03.log:202  python3 scripts/ci/check-gui-protocol.sh

Line 61 is what was run. Line 202 is the section titled "reproduce it", and it hands the
reader a bash script to explain to Python, which answers `SyntaxError`. The reader who
follows it concludes the reproduction does not work, or concludes the guard is broken.
The same shape cost this repository a false measurement the same day, recorded in
`docs/ci-test-debt.md`: a sweep ran `python3 scripts/ci/check-versions.sh`, python raised
on a bash file, the exit code the loop looked at came from `tail`, and the gate was
reported as passing.

The two rules:

1. An interpreter cannot read what it was handed. Where a command line runs a named
   interpreter over a script file, the script's extension has to belong to that
   interpreter's family: `python3` gets `.py`, `bash` gets `.sh`, `pwsh` gets `.ps1`,
   `node` gets `.mjs`/`.cjs`/`.js`. `python3 -c`, `python3 -m`, `bash -c` and
   `python3 - <<'PY'` name no script file and are therefore not claims about a file.
   A token written inside quotes is data, so `printf '%s\n' 'python3 x.sh'`, which is how
   a ledger row for a wrong command gets written down, is not a claim about python3.
2. A path named by a command line has to still be there, unless the command is what
   puts it there. A transcript is a reproduction recipe, and a recipe that names a file
   which no longer exists reproduces
   nothing. `docs/verification/docker-gate-mirror-2026-10-03.log` tells the reader to
   write `docs/verification/todo-open-items-2026-10-03.tsv`, a name this repository
   retired on 2026-10-04 in favour of an undated export, and the line is correct about
   what happened then and useless as an instruction now.

What counts as a command line: a line whose first non-space text is `$ `, in a tracked
`*.md` or in `docs/verification/*.log`; plus, in either kind of file, every line of a
fence labelled as a shell (`sh`, `bash`, `console`, `pwsh`, ...), because those are the
blocks `CONTRIBUTING.md` asks a reader to paste. Both rules read both shapes. Reading only
`$` lines was the first version and it is the weaker one: on the current tree it reports 4
absent paths where the shipped rule reports 5, and the fifth sits inside a fence. When the
rule was written the fence tier surfaced four more, invented file names in three recipes of
`.agents/skills/chaos-upstream-sync/references/port-playbook.md`, a document whose whole job
is to be run; those recipes name real things now, one of them a version through a shell
variable, so they run instead of being excused.

The exclusions on `check_paths` are what make rule 2 readable at all: with none of them it
reports 11 paths on the current tree. The pattern rule accounts for 5 of the 6 (`docs/*.md`
and `x-<hash>.py` and `sync/upstream-$(date +%Y%m%d)` are shapes, not files), the redirect
rule for 1. The creator and copyist rules remove nothing today, which is said rather than
glossed over: they are pinned by fixtures, because `mkdir -p docs/verify` and
`python3 x.py > docs/out.tsv` are exactly the shapes that would be reported the first time
someone documents them, and a rule that goes red on its first true statement gets deleted.

Of the 5 absences that remain, all 5 are recorded: 3 are transcripts quoting an absence as
the finding itself, 1 names a file retired the next day, and 1 is a sample hook script
calling a script that belongs to whoever reads the guide.

Existence is answered from git rather than from the filesystem, and that choice is a rule of
its own. Asked of the filesystem, this guard was green on the machine that wrote the
transcript -- whose tree carries a 143M `apps/chaos-ui/node_modules` -- and red on a runner
that checked the same commit out into a tree without one. A verdict about a commit has to be
a property of the commit, so a path counts as content when git tracks it, when it is a
directory holding tracked content, or when .gitignore declares it generated: reading that
last kind is a recipe with a build step in front of it, which is what
`du -sh target .git apps/chaos-ui/node_modules` is. The ignore rule asks both shapes because
a pattern written `dir/` matches only directories, and asking about `dir` alone would put the
host-versus-runner split back one level down. A path that is neither tracked nor ignored is a
claim even when this particular checkout happens to have the file.

Exemptions are recorded, not silent. `scripts/ci/evidence-commands-allowlist.tsv` carries
`key<TAB>category<TAB>reason`, with a reason of at least 20 characters; the key is the
thing the finding names, which is the path a command points at for the three path
categories and the command itself (`python3 scripts/ci/x.sh`) for `quoted-command`, so a
row can only excuse the finding its own category describes. A row with no live finding
any more fails as stale, and a transcript that quotes a wrong command on purpose (some do:
diagnosing a mistake means writing it down) has to say so
with the `quoted-command` category rather than edit the transcript.

Not covered, stated rather than pretended: heredoc bodies (they are not command lines, and
`python3 - <<'PY'` claims nothing about a file); the exit code a quoted pipeline actually
returned, which is a claim no scanner can check without running it, and which is precisely
how the `| tail -3` false positive in `docs/ci-test-debt.md` happened; a command whose
interpreter is a variable (`$PY x.py`); the mirror image of the quoted-data rule, an
interpreter named outside quotes where it is still only an argument
(`echo run python3 scripts/ci/x.sh` reads as a claim and is reported); absolute
paths outside this repository, which rule 2 does not judge; and any path whose first
segment is not a tracked top-level entry, which is the same anchor rule
`check-doc-path-refs.py` uses and for the same reason.

Usage:
    check-evidence-commands.py [--root DIR] [--allowlist FILE] [--list] [--all]
Exit: 0 = every command line can run, 1 = problems, 2 = repo or allowlist unreadable.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

CATEGORIES = (
    "historical",       # correct about then, stale as an instruction now
    "recorded-absent",  # the transcript itself says the file was temporary or deleted
    "other-root",       # relative to another root: a scratch clone, a worktree, a user project
    "quoted-command",   # the wrong command is the thing being described, not a recipe
)

REASON_MIN = 20

# Interpreter command name => family. A command word is compared on its last path
# segment, so `./venv/bin/python3` is still python.
INTERPRETERS = {
    "python": "python", "python2": "python", "python3": "python",
    "bash": "shell", "sh": "shell", "dash": "shell", "zsh": "shell", "ksh": "shell",
    "pwsh": "pwsh", "powershell": "pwsh",
    "node": "node", "deno": "node", "bun": "node",
}

# Script extension => family. Only these extensions make a claim about which interpreter
# is expected; `.txt`, `.json` and a bare name make none.
SCRIPT_EXT = {
    "py": "python", "pyw": "python",
    "sh": "shell", "bash": "shell", "zsh": "shell",
    "ps1": "pwsh", "psm1": "pwsh", "psd1": "pwsh",
    "mjs": "node", "cjs": "node", "js": "node", "ts": "node", "mts": "node",
}

# What to run each family's scripts with, for the message.
PREFERRED = {"python": "python3", "shell": "bash", "pwsh": "pwsh", "node": "node"}

CMD_RE = re.compile(r"^\s*\$\s+(.+)$")
FENCE_RE = re.compile(r"^\s{0,3}(`{3,}|~{3,})\s*([A-Za-z0-9_+-]*)\s*$")
SHELL_LANGS = {"sh", "bash", "shell", "zsh", "console", "pwsh", "powershell", "ps1",
               "cmd", "bat", "batch", "dosbatch"}
SEPARATOR_RE = re.compile(r"\s*(?:\|\||&&|[|;&]|>{1,2}<?|<+)\s*")
# Rule 2 splits on command separators only, never on a redirect: it has to still see the
# `>` to know that what follows is written rather than read.
SPLIT_RE = re.compile(r"\s*(?:\|\||&&|[|;&])\s*")
REDIRECT_RE = re.compile(r"^\d*>&?>?$")
# Commands whose arguments are files they create or remove: those paths are not claims
# that the file is present.
CREATORS = {"mkdir", "touch", "rm", "rmdir", "tee", "truncate", "mkfifo",
            "ln"}
# These three write their last argument and read the ones before it, so the ones before
# it are still claims that the file is there. `ln` is exempt as a whole instead, because
# `ln -s nowhere link` is a legal dangling symlink.
COPYISTS = {"cp", "mv", "install"}

SEG = r"[\w.\-+@\u4e00-\u9fff]"
PATH_RE = re.compile(r"(?:\.{1,2}/)*" + SEG + r"+/(?:" + SEG + r"+/)*" + SEG + r"+")
# `...` and a placeholder in angle brackets describe a shape; `$(`, `%` and a brace group
# are substituted by the shell or by a human before the command runs.
PATTERN_CHARS = ("...", "…", "<", ">", "$", "%", "[", "]", "*", "?", "~", "{", "}")
QUOTE_CHARS = "\"'`"


class Repo:
    """The repository's own view of what its content is."""

    def __init__(self, root: Path) -> None:
        self.root = root
        self.files = self._git(["ls-files", "-z"])
        self.tops = {f.split("/", 1)[0] for f in self.files}
        # `git ls-files` lists files, never the directories holding them, and a command
        # line may well read a directory. Every ancestor of a tracked file is therefore
        # content of the repository too.
        self.dirs: set[str] = set()
        for rel in self.files:
            parts = rel.split("/")
            for i in range(1, len(parts)):
                self.dirs.add("/".join(parts[:i]))
        # Existence is asked of git, not of the filesystem: the same commit has to give
        # the same verdict in a fresh clone and in a tree somebody finished building in.
        # Answering from the filesystem is what made this guard green on the machine it
        # was written on and red in CI, because `apps/chaos-ui/node_modules` sits in that
        # machine's tree and is not in the commit.
        self._ignored: dict[str, bool] = {}

    def presents(self, rel: str) -> bool:
        """Is `rel` content of the repository: tracked, or generated and ignored?"""
        bare = rel.rstrip("/")
        if bare in self.files or bare in self.dirs:
            return True
        return self.is_ignored(bare)

    def is_ignored(self, rel: str) -> bool:
        """Does .gitignore declare `rel` as something the repository does not carry?

        A command line reading an ignored path is a recipe with a build step in front of
        it, not a reference to a file that went missing. Both shapes are asked in one
        call: a pattern written as `dir/` matches only a directory, so `git check-ignore`
        answers "not ignored" for `dir` in a tree that was never built and "ignored" in
        one that was, which is the very host-versus-CI difference this method exists to
        remove. Asked per path, because the paths that get here are the handful whose
        first segment is tracked and which are not tracked themselves.
        """
        if rel not in self._ignored:
            proc = subprocess.run(
                ["git", "-C", str(self.root), "check-ignore", "-q", "--stdin"],
                input=f"{rel}\n{rel}/\n", capture_output=True, text=True)
            if proc.returncode not in (0, 1):
                raise RuntimeError(f"git check-ignore {rel}: {proc.stderr.strip()}")
            self._ignored[rel] = proc.returncode == 0
        return self._ignored[rel]

    def _git(self, args: list[str]) -> set[str]:
        proc = subprocess.run(["git", "-C", str(self.root), *args],
                              capture_output=True, text=True)
        if proc.returncode != 0:
            raise RuntimeError(f"git {' '.join(args)}: {proc.stderr.strip()}")
        return {f for f in proc.stdout.split("\0") if f}

    def scanned(self) -> list[str]:
        """Transcripts plus every tracked document, sorted."""
        return sorted(f for f in self.files
                      if f.endswith(".md") or (f.startswith("docs/verification/")
                                               and f.endswith(".log")))


def command_lines(text: str) -> list[tuple[int, str]]:
    """Every line that presents itself as a command: (lineno, command).

    A `$ ` line is a command anywhere. Inside a shell-labelled fence every line is one,
    because that is the block a reader pastes. Fences are tracked but not blanked: line
    numbers have to stay what `grep -n` prints.
    """
    out: list[tuple[int, str]] = []
    fence: tuple[str, int] | None = None
    lang = ""
    for lineno, line in enumerate(text.split("\n"), 1):
        m = FENCE_RE.match(line)
        if m:
            marker, run, info = m.group(1)[0], len(m.group(1)), m.group(2)
            if fence is not None and not info and marker == fence[0] and run >= fence[1]:
                fence = None  # CommonMark: a closer carries no info string and is not shorter
            elif fence is None:
                fence, lang = (marker, run), info.lower()
            continue
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        marked = CMD_RE.match(line)
        if marked:
            out.append((lineno, marked.group(1).rstrip()))
        elif lang in SHELL_LANGS and fence is not None:
            out.append((lineno, stripped))
    return out


def first_script_operand(tokens: list[str]) -> str | None:
    """The first token that claims to be a script file, skipping flags and `-`."""
    for tok in tokens:
        tok = tok.strip(QUOTE_CHARS)
        if not tok or tok.startswith("-"):
            continue
        ext = tok.rsplit(".", 1)[-1].lower() if "." in tok else ""
        if ext in SCRIPT_EXT:
            return tok.strip(QUOTE_CHARS)
    return None


def check_interpreter(rel: str, lineno: int, cmd: str) -> list[tuple[str, str]]:
    """Rule 1: the interpreter and the script it is handed must be from one family.

    Each finding is returned as `(unit, message)`, where `unit` is the command as the
    finding reports it (`python3 scripts/ci/x.sh`) and is also the key a
    `quoted-command` ledger row has to carry.
    """
    problems: list[tuple[str, str]] = []
    for seg in SEPARATOR_RE.split(cmd):
        tokens = seg.split()
        for i, tok in enumerate(tokens):
            if tok[0] in QUOTE_CHARS:
                # Text inside quotes is data with an interpreter's name in it, not a
                # command being run: `printf '%s\n' 'python3 x.sh'` writes a line. The
                # cost is a command word written in quotes (`'python3' x.py`), which is
                # not something a document writes.
                continue
            family = INTERPRETERS.get(tok.rsplit("/", 1)[-1])
            if family is None:
                continue
            operand = first_script_operand(tokens[i + 1:])
            if operand is None:
                break
            ofam = SCRIPT_EXT[operand.rsplit(".", 1)[-1].lower()]
            if ofam != family:
                unit = f"{tok} {operand}"
                problems.append((
                    unit,
                    f"{rel}:{lineno}: `{unit}` cannot work: {tok} is a "
                    f"{family} interpreter and `.{operand.rsplit('.', 1)[-1].lower()}` "
                    f"is a {ofam} script, run it with {PREFERRED[ofam]}"
                ))
            break
    return problems


def command_word(tokens: list[str]) -> str:
    """The command a pipeline segment runs, skipping `VAR=value` prefixes and flags."""
    for tok in tokens:
        if tok.startswith("-") or "=" in tok:
            continue
        return tok.strip(QUOTE_CHARS).rsplit("/", 1)[-1].lower()
    return ""


def check_paths(repo: Repo, rel: str, lineno: int, cmd: str) -> list[tuple[str, str]]:
    """Rule 2: a repository path named by any command line has to exist. (raw hits)

    Three things are deliberately not claims that a file is present:

    - a token shaped like a pattern (`docs/*.md`, `scripts/x-<hash>.py`). The test runs on
      the whole whitespace token before the path is cut out of it, because the path regex
      stops at `<`: `scripts/ci/x-<hash>.py` yields `scripts/ci/x-`, which would otherwise
      be reported as a missing file;
    - a redirect target, and every argument of `mkdir`, `touch`, `rm`, `tee` and friends.
      A transcript line `python3 x.py > docs/out.tsv` is a command that *writes* the file,
      and `rm docs/tmp.md` records a removal. `cp`, `mv` and `install` are read as what
      they are: their last argument is written, the ones before it have to already be
      there. `ln` is exempt entirely, because a symlink may point at something that does
      not exist yet;
    - a path whose first segment is not a tracked top-level entry (`target/`, `/tmp/...`,
      `../other/`, an untracked directory that exists only in a developer's checkout).
    """
    hits: list[tuple[str, str]] = []
    for segment in SPLIT_RE.split(cmd):
        tokens = segment.split()
        word = command_word(tokens)
        if not tokens or word in CREATORS:
            continue
        if word in COPYISTS:
            tokens = tokens[:-1]
        prev = ""
        for tok in tokens:
            was_redirect = bool(REDIRECT_RE.match(prev))
            prev = tok
            if was_redirect or any(c in tok for c in PATTERN_CHARS):
                continue
            for found in PATH_RE.findall(tok):
                bare = found[2:] if found.startswith("./") else found
                if bare.split("/")[0] not in repo.tops:
                    continue
                if repo.presents(bare):
                    continue
                hits.append((bare, f"{rel}:{lineno}"))
    return hits


def load_allowlist(path: Path) -> dict[str, tuple[str, str]]:
    out: dict[str, tuple[str, str]] = {}
    if not path.exists():
        return out
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) < 3:
            raise ValueError(
                f"{path.name}:{lineno}: expected 3 tab-separated columns "
                "(path, category, reason)"
            )
        tok, category, reason = (c.strip() for c in cols[:3])
        if category not in CATEGORIES:
            raise ValueError(
                f"{path.name}:{lineno}: category `{category}` is not one of: "
                f"{', '.join(CATEGORIES)}"
            )
        if len(reason) < REASON_MIN:
            raise ValueError(
                f"{path.name}:{lineno}: reason for `{tok}` is under {REASON_MIN} "
                "characters, which is a note rather than a judgement"
            )
        if tok in out:
            raise ValueError(f"{path.name}:{lineno}: duplicate entry for `{tok}`")
        out[tok] = (category, reason)
    return out


def scan(repo: Repo) -> tuple[list[str], list[tuple[str, str]],
                             list[tuple[str, str]], dict[str, int]]:
    """Returns (hard problems, mismatch findings, dangling paths, counters).

    Both finding lists hold `(key, message)` pairs: the key is what a ledger row has to
    carry to record the finding, the message is what gets printed.
    """
    problems: list[str] = []
    mismatches: list[tuple[str, str]] = []
    dangling: list[tuple[str, str]] = []
    counters = {"files": 0, "cmd_lines": 0, "mismatch": 0}
    for rel in repo.scanned():
        counters["files"] += 1
        doc = repo.root / rel
        try:
            text = doc.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: unreadable: {exc}")
            continue
        for lineno, cmd in command_lines(text):
            counters["cmd_lines"] += 1
            found = check_interpreter(rel, lineno, cmd)
            counters["mismatch"] += len(found)
            mismatches.extend(found)
            dangling.extend(check_paths(repo, rel, lineno, cmd))
    return problems, mismatches, dangling, counters


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(Path(__file__).resolve().parents[2]))
    parser.add_argument("--allowlist", default=None,
                        help="override the recorded-exception table (used by the fixtures)")
    parser.add_argument("--list", action="store_true", dest="list_files",
                        help="print the files that would be scanned and exit")
    parser.add_argument("--all", action="store_true",
                        help="print recorded findings as well as unrecorded ones")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    try:
        repo = Repo(root)
    except RuntimeError as exc:
        print(f"check-evidence-commands: {exc}", file=sys.stderr)
        return 2

    if args.list_files:
        for rel in repo.scanned():
            print(rel)
        return 0

    allow_path = (Path(args.allowlist) if args.allowlist
                  else root / "scripts/ci/evidence-commands-allowlist.tsv")
    try:
        allow = load_allowlist(allow_path)
    except ValueError as exc:
        print(f"check-evidence-commands: bad allowlist: {exc}", file=sys.stderr)
        return 2

    problems, mismatches, dangling, counters = scan(repo)

    # The two rules are recorded by different keys, and a row may only record what its
    # category describes: a path row cannot excuse an interpreter mismatch, and a quoted
    # command cannot excuse an absent file. Otherwise one loose row silences both rules.
    quoted = {tok for tok, (category, _) in allow.items() if category == "quoted-command"}
    path_rows = set(allow) - quoted

    path_hits: dict[str, int] = {}
    quoted_hits: dict[str, int] = {}
    for unit, message in mismatches:
        quoted_hits[unit] = quoted_hits.get(unit, 0) + 1
        if unit in quoted and not args.all:
            continue
        category = "quoted-command" if unit in quoted else "unrecorded"
        problems.append(f"{message} ({category})")

    for tok, where in dangling:
        path_hits[tok] = path_hits.get(tok, 0) + 1
        if tok in path_rows and not args.all:
            continue
        category = allow[tok][0] if tok in path_rows else "unrecorded"
        problems.append(
            f"{where}: `{tok}` names a file that is not in the repository, so this "
            f"command line reproduces nothing ({category})"
        )

    # Liveness is judged per category as well, so a row cannot stay in the ledger because
    # the other rule happens to report something with the same text.
    stale = (set(path_rows) - set(path_hits)) | (set(quoted) - set(quoted_hits))
    for tok in sorted(stale):
        category, reason = allow[tok]
        problems.append(
            f"{allow_path}: recorded `{tok}` ({category}) has no finding left; the "
            f"command line was fixed, so the entry is stale. Reason was: {reason}"
        )

    for problem in problems:
        print(f"check-evidence-commands: {problem}", file=sys.stderr)

    summary = (
        f"check-evidence-commands: {counters['files']} files, "
        f"{counters['cmd_lines']} command lines, {counters['mismatch']} interpreter "
        f"mismatch(es), {len(dangling)} dangling path(s), {len(allow)} recorded"
    )
    if problems:
        print(f"{summary}; {len(problems)} problem(s)", file=sys.stderr)
        return 1
    print(f"{summary}; every command line can run and every absence is recorded and live")
    return 0


if __name__ == "__main__":
    sys.exit(main())
