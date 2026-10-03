#!/usr/bin/env python3
"""Fixtures for `scripts/upstream-recon.sh`: what a recon run may and may not do.

The script's own header promised three things -- it never fetches, it never
writes outside `sync/recon/`, and a network failure exits non-zero instead of
writing a record that looks like a completed review. None of that had ever been
run. On 2026-10-03 the documented command destroyed a record: the output file is
named `$(date -u)-<tip>.md`, so a second run on the same UTC day redirected over
the first, and what it overwrote was a hand-enriched 100-line record (ancestor
check, fork scale, the count of changed files inside the l10n-protected paths).
The regenerated 21-line table took its place, and only `git status` said so.

Every case here runs the shipped script bytes inside a throwaway repository: a
local bare git repo stands in for upstream (`git ls-remote` works on paths), and
a loopback HTTP server stands in for the compare API, so the real `curl`, the
real status handling and the real record layout are exercised. The promises in
the header are pinned by cases 5-8; the non-destructive rule is cases 2-4.

    python3 scripts/ci/test-upstream-recon.py
"""

import functools
import http.server
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / 'scripts/upstream-recon.sh'
API_REPO_PATH = 'repos/xai-org/grok-build'
RECORD_NAME = re.compile(r'^\d{4}-\d{2}-\d{2}-[0-9a-f]{9}(-\d+)?\.md$')


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args) -> None:  # the fixture asserts, it does not log
        pass


def run_git(args, cwd):
    return subprocess.run(['git', '-c', 'user.email=fixture@example.invalid',
                           '-c', 'user.name=Fixture', *args],
                          cwd=str(cwd), capture_output=True, text=True, check=True)


class Fixture(unittest.TestCase):
    """One throwaway fork + one fake upstream + one fake compare API."""

    server = None
    port = 0

    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(tempfile.mkdtemp(prefix='upstream-recon-fixture-'))
        work = cls.root / 'upstream-work'
        work.mkdir()
        run_git(['init', '--quiet', '--initial-branch=main', str(work)], cwd=work)
        (work / 'README.md').write_text('upstream\n', encoding='utf-8')
        run_git(['add', 'README.md'], cwd=work)
        run_git(['commit', '-q', '-m', 'Synced from monorepo'], cwd=work)
        cls.tip = run_git(['rev-parse', 'HEAD'], cwd=work).stdout.strip()
        subprocess.run(['git', 'clone', '--quiet', '--bare', str(work),
                        str(cls.root / 'upstream.git')],
                       check=True, capture_output=True, text=True)
        cls.empty_upstream = cls.root / 'empty-upstream.git'
        subprocess.run(['git', 'init', '--quiet', '--bare',
                        str(cls.empty_upstream)], check=True, capture_output=True,
                       text=True)

        cls.api_dir = cls.root / 'api' / API_REPO_PATH / 'compare'
        cls.api_dir.mkdir(parents=True)
        payload = {'status': 'ahead', 'ahead_by': 9, 'behind_by': 0,
                   'total_commits': 9}
        (cls.api_dir / 'PLACEHOLDER...HEAD').write_text(json.dumps(payload),
                                                        encoding='utf-8')
        quiet = functools.partial(QuietHandler, directory=str(cls.root / 'api'))
        cls.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), quiet)
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls) -> None:
        if cls.server is not None:
            cls.server.shutdown()
            cls.server.server_close()
        shutil.rmtree(cls.root, ignore_errors=True)

    def setUp(self) -> None:
        case = Path(tempfile.mkdtemp(prefix='fork-', dir=self.root))
        (case / 'scripts').mkdir()
        # The shipped bytes, not a copy of the logic: a mutation to the real
        # script has to change what these cases see.
        shutil.copyfile(SCRIPT, case / 'scripts/upstream-recon.sh')
        self.case = case
        self.recon_dir = case / 'sync/recon'

    def set_source_rev(self, value) -> None:
        if value is None:
            return
        (self.case / 'SOURCE_REV').write_text(value + '\n', encoding='utf-8')

    def api_name(self, source_rev: str) -> Path:
        # The script asks for `<source_rev>...HEAD`; the one fixture payload is
        # published under the name the current case will request.
        stored = self.api_dir / 'PLACEHOLDER...HEAD'
        shutil.copyfile(stored, self.api_dir / f'{source_rev}...HEAD')

    #: A fixture run is a few git commands against a local repo, so this is orders
    #: of magnitude above what a healthy run takes. It exists because a script that
    #: loops forever (one mutation of the record-name loop did) otherwise hangs the
    #: whole suite: no failure, no named case, just a CI job sitting on the runner
    #: limit.
    SCRIPT_TIMEOUT_SECONDS = 60

    def run_script(self, upstream=None, api_root=None):
        env = dict(os.environ)
        env['UPSTREAM_REMOTE'] = str(upstream if upstream is not None
                                     else self.root / 'upstream.git')
        env['UPSTREAM_API'] = (api_root if api_root is not None else
                               f'http://127.0.0.1:{self.port}/{API_REPO_PATH}')
        try:
            return subprocess.run(
                ['bash', str(self.case / 'scripts/upstream-recon.sh')],
                capture_output=True, text=True, env=env, cwd=str(self.case),
                timeout=self.SCRIPT_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            self.fail(
                f'upstream-recon.sh had not finished after '
                f'{self.SCRIPT_TIMEOUT_SECONDS}s against a local fixture repo; '
                'it is stuck, most likely in the loop that chooses a record name')

    def records(self):
        return sorted(p.name for p in self.recon_dir.glob('*.md')) if self.recon_dir.exists() else []

    def test_first_run_writes_the_record_named_by_date_and_tip(self) -> None:
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        self.api_name(source)
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(len(self.records()), 1, self.records())
        name = self.records()[0]
        self.assertTrue(name.endswith(f'-{self.tip[:9]}.md'), name)
        text = (self.recon_dir / name).read_text(encoding='utf-8')
        self.assertIn(f'`{source}`', text)
        self.assertIn(f'`{self.tip}`', text)
        self.assertIn('| 上游领先 `SOURCE_REV` 的提交数 | 9 |', text)
        self.assertIn(f'wrote sync/recon/{name}', result.stdout)
        # The record is built with mktemp (0600) and cp carries that mode to a
        # new file, so without an explicit chmod the report lands unreadable to
        # everyone else on the machine.
        mode = (self.recon_dir / name).stat().st_mode & 0o777
        self.assertEqual(mode & 0o044, 0o044,
                         f'{oct(mode)}: a record other maintainers must read was '
                         'written private')

    def test_an_identical_rerun_leaves_the_record_untouched(self) -> None:
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        self.api_name(source)
        self.assertEqual(self.run_script().returncode, 0)
        name = self.records()[0]
        before = (self.recon_dir / name).read_bytes()
        mtime = (self.recon_dir / name).stat().st_mtime_ns
        again = self.run_script()
        self.assertEqual(again.returncode, 0, again.stdout + again.stderr)
        self.assertEqual(self.records(), [name], 'a rerun must not add a file')
        self.assertEqual((self.recon_dir / name).read_bytes(), before)
        self.assertEqual((self.recon_dir / name).stat().st_mtime_ns, mtime,
                         'an unchanged record must not even be rewritten')
        self.assertIn('already holds this exact record', again.stdout)

    def test_a_second_run_never_overwrites_a_record_it_did_not_write(self) -> None:
        # The 2026-10-03 incident, replayed: the file already holds a
        # hand-enriched record, so the generated one has to land beside it.
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        self.api_name(source)
        self.assertEqual(self.run_script().returncode, 0)
        name = self.records()[0]
        enriched = ('# 上游侦察\n\n人手补的清点：ancestor=yes、分叉规模 969、'
                    '强保护路径 290 files。\n')
        (self.recon_dir / name).write_text(enriched, encoding='utf-8')
        second = self.run_script()
        self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
        self.assertEqual((self.recon_dir / name).read_text(encoding='utf-8'),
                         enriched, 'a recon record was overwritten')
        self.assertEqual(len(self.records()), 2, self.records())
        fresh = [n for n in self.records() if n != name][0]
        self.assertTrue(fresh.endswith(f'-{self.tip[:9]}-2.md'), fresh)
        self.assertIn('| 上游领先 `SOURCE_REV` 的提交数 | 9 |',
                      (self.recon_dir / fresh).read_text(encoding='utf-8'))
        self.assertIn('never overwritten', second.stderr)
        self.assertIn(f'wrote sync/recon/{fresh}', second.stdout)

    def test_a_third_different_run_adds_a_third_record(self) -> None:
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        self.api_name(source)
        self.assertEqual(self.run_script().returncode, 0)
        name = self.records()[0]
        (self.recon_dir / name).write_text('人手补的清点。\n', encoding='utf-8')
        self.assertEqual(self.run_script().returncode, 0)
        (self.recon_dir / f'{name[:-3]}-2.md').write_text('第二次人手补的清点。\n',
                                                          encoding='utf-8')
        third = self.run_script()
        self.assertEqual(third.returncode, 0, third.stdout + third.stderr)
        self.assertEqual(len(self.records()), 3, self.records())
        self.assertIn(f'{name[:-3]}-3.md', self.records(), self.records())
        self.assertEqual((self.recon_dir / f'{name[:-3]}-2.md').read_text(encoding='utf-8'),
                         '第二次人手补的清点。\n')

    def test_missing_source_rev_fails_and_writes_nothing(self) -> None:
        result = self.run_script()
        self.assertEqual(result.returncode, 1)
        self.assertIn('missing', result.stderr)
        self.assertEqual(self.records(), [])

    def test_empty_source_rev_fails_and_writes_nothing(self) -> None:
        self.set_source_rev('   ')
        result = self.run_script()
        self.assertEqual(result.returncode, 1)
        self.assertIn('empty', result.stderr)
        self.assertEqual(self.records(), [])

    def test_upstream_without_commits_fails_and_writes_nothing(self) -> None:
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        self.api_name(source)
        result = self.run_script(upstream=self.empty_upstream)
        self.assertEqual(result.returncode, 1)
        self.assertIn('could not read upstream HEAD', result.stderr)
        self.assertEqual(self.records(), [])

    def test_compare_api_failure_writes_no_record(self) -> None:
        # The header promises a failed comparison is not recorded as a review.
        source = uuid.uuid4().hex
        self.set_source_rev(source)
        result = self.run_script(api_root=f'http://127.0.0.1:{self.port}/no-such-repo')
        self.assertEqual(result.returncode, 1)
        self.assertIn('compare API returned HTTP', result.stderr)
        self.assertEqual(self.records(), [])


class ShippedRepository(unittest.TestCase):
    def test_committed_records_are_still_records(self) -> None:
        recon = REPO / 'sync/recon'
        generated = [p for p in sorted(recon.iterdir()) if RECORD_NAME.match(p.name)]
        self.assertGreaterEqual(len(generated), 2, generated)
        for path in generated:
            self.assertTrue(path.read_text(encoding='utf-8').startswith('# 上游侦察'),
                            f'{path.name} is named like a generated record but is not one')
        # The hand-written review that lives in the same directory is exactly the
        # kind of file the old redirect used to sit on top of.
        self.assertTrue(any(p.name.endswith('upstream-adjudication.md')
                            for p in recon.iterdir()),
                        'the adjudication note belongs beside the generated records')

    def test_the_script_has_no_bare_overwrite_of_a_record(self) -> None:
        text = SCRIPT.read_text(encoding='utf-8')
        self.assertNotIn('} >"$out_file"', text,
                         'the record must be built in a temp file and placed by name')
        self.assertIn('cmp -s "$record" "$out_file"', text)


if __name__ == '__main__':
    unittest.main(verbosity=2)
