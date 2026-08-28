#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import filecmp
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import tarfile
import threading
import urllib.request
import zipfile
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable, Optional, Sequence

Definition = dict[str, str]
Totals = dict[str, Any]
PrepStep = dict[str, Any]
Binaries = dict[str, Path]

SCC_VERSION = '4.0.0'
TOKEI_VERSION = '14.0.0'

CORPUS_KEYS = ('name', 'remote', 'commit', 'languages', 'types')
CORPUS_REQUIRED = ('name', 'languages', 'types')

EQUAL_WORK_MEZURA = ['--hide', 'keywords', '--count-minified', '--count-generated',
                     '--counting', 'region']
EQUAL_WORK_SCC = ['-c', '--no-cocomo', '--no-config']
EQUAL_WORK_TOKEI = []


def pinned_flags(definition: Definition) -> tuple[list[str], list[str], list[str]]:
    return (['--languages', definition['languages']] + EQUAL_WORK_MEZURA,
            ['-i', definition['languages']] + EQUAL_WORK_SCC,
            ['-t', definition['types']] + EQUAL_WORK_TOKEI)

def real_home() -> Path:
    user = os.environ.get('SUDO_USER')
    if user:
        try:
            import pwd
            return Path(pwd.getpwnam(user).pw_dir)
        except Exception:
            pass
    return Path.home()


HOME = real_home()


def give_back_to_user(root: Path) -> None:
    user = os.environ.get('SUDO_USER')
    if not user or PLATFORM == 'windows' or not hasattr(os, 'chown'):
        return
    try:
        import pwd
        entry = pwd.getpwnam(user)
    except Exception:
        return
    targets = [root] + [p for p in root.rglob('*')]
    for target in targets:
        try:
            os.chown(target, entry.pw_uid, entry.pw_gid, follow_symlinks=False)
        except OSError:
            pass


def under_home(path: Path) -> Path:
    text = str(path)
    if text == '~':
        return HOME
    if text.startswith('~' + os.sep) or text.startswith('~/'):
        return HOME / text[2:]
    return path.expanduser()
DEFAULT_TOOLS = HOME / 'Documents' / 'dev' / 'tools'
DEFAULT_CORPUS = HOME / 'Documents' / 'dev' / 'bench' / 'linux'

SCC_ASSETS = {
    ('linux', 'x86_64'): 'scc_Linux_x86_64.tar.gz',
    ('linux', 'arm64'): 'scc_Linux_arm64.tar.gz',
    ('wsl', 'x86_64'): 'scc_Linux_x86_64.tar.gz',
    ('macos', 'x86_64'): 'scc_Darwin_x86_64.tar.gz',
    ('macos', 'arm64'): 'scc_Darwin_arm64.tar.gz',
    ('windows', 'x86_64'): 'scc_Windows_x86_64.zip',
    ('windows', 'arm64'): 'scc_Windows_arm64.zip',
}


def detect_platform() -> str:
    system = platform.system()
    if system == 'Windows':
        return 'windows'
    if system == 'Darwin':
        return 'macos'
    if system == 'Linux':
        try:
            if 'microsoft' in Path('/proc/version').read_text().lower():
                return 'wsl'
        except OSError:
            pass
        return 'linux'
    return system.lower()


def detect_arch() -> str:
    machine = platform.machine().lower()
    if machine in ('x86_64', 'amd64'):
        return 'x86_64'
    if machine in ('arm64', 'aarch64'):
        return 'arm64'
    return machine


PLATFORM = detect_platform()
ARCH = detect_arch()
EXE = '.exe' if PLATFORM == 'windows' else ''


class Transcript:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.file = None
        self.thread = None
        self.saved = None

    def start(self) -> None:
        try:
            self.file = open(self.path, 'wb')
            read_fd, write_fd = os.pipe()
            saved_out, saved_err = os.dup(1), os.dup(2)
            sys.stdout.flush()
            sys.stderr.flush()
            os.dup2(write_fd, 1)
            os.dup2(write_fd, 2)
            os.close(write_fd)
            self.saved = (saved_out, saved_err)

            def pump() -> None:
                while True:
                    try:
                        chunk = os.read(read_fd, 4096)
                    except OSError:
                        break
                    if not chunk:
                        break
                    try:
                        self.file.write(chunk)
                        self.file.flush()
                    except (OSError, ValueError):
                        pass
                    try:
                        os.write(saved_out, chunk)
                    except OSError:
                        pass
                try:
                    os.close(read_fd)
                except OSError:
                    pass

            self.thread = threading.Thread(target=pump, daemon=True)
            self.thread.start()
        except Exception:
            self.stop()
            say(f'WARNING: could not capture a transcript into {self.path}')

    def stop(self) -> None:
        saved = self.saved
        if saved:
            try:
                sys.stdout.flush()
                sys.stderr.flush()
                os.dup2(saved[0], 1)
                os.dup2(saved[1], 2)
            except OSError:
                pass
            self.saved = None
        if self.thread:
            self.thread.join(timeout=5)
            self.thread = None
        for fd in saved or ():
            try:
                os.close(fd)
            except OSError:
                pass
        if self.file:
            try:
                self.file.close()
            except OSError:
                pass
            self.file = None


def say(message: str) -> None:
    print(message, flush=True)


def run(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess:
    return subprocess.run(argv, **kwargs)


def capture_ok(argv: Sequence[str], timeout: int = 30) -> tuple[bool, str]:
    try:
        out = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.SubprocessError):
        return False, ''
    return out.returncode == 0, out.stdout.strip()


def capture(argv: Sequence[str], default: str = '', timeout: int = 30) -> str:
    ok, out = capture_ok(argv, timeout)
    return out if ok and out else default


def powershell(script: str, default: str = '') -> str:
    return capture(['powershell', '-NoProfile', '-Command', script], default)


def quote(part: Any) -> str:
    part = str(part)
    return f'"{part}"' if ' ' in part and not part.startswith('"') else part


def join_cmd(parts: Iterable[Any]) -> str:
    return ' '.join(quote(p) for p in parts)


def cpu_name() -> str:
    if PLATFORM == 'windows':
        return powershell('(Get-CimInstance Win32_Processor).Name', 'unknown')
    if PLATFORM == 'macos':
        return capture(['sysctl', '-n', 'machdep.cpu.brand_string'], 'unknown')
    for line in capture(['lscpu']).splitlines():
        if line.startswith('Model name:'):
            return line.split(':', 1)[1].strip()
    return 'unknown'


def ram_bytes() -> Optional[int]:
    if PLATFORM == 'windows':
        value = powershell('(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory')
        return int(value) if value.isdigit() else None
    if PLATFORM == 'macos':
        value = capture(['sysctl', '-n', 'hw.memsize'])
        return int(value) if value.isdigit() else None
    try:
        for line in Path('/proc/meminfo').read_text().splitlines():
            if line.startswith('MemTotal:'):
                return int(line.split()[1]) * 1024
    except OSError:
        pass
    return None


def cpu_scaling() -> str:
    if PLATFORM == 'windows':
        return powershell('(powercfg /getactivescheme)', 'unknown')
    if PLATFORM == 'macos':
        return 'n/a'
    try:
        return Path('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor').read_text().strip()
    except OSError:
        return 'none'


def filesystem_of(path: Path) -> str:
    if PLATFORM == 'windows':
        drive = str(Path(path).resolve())[0]
        return powershell(f'(Get-Volume -DriveLetter {drive}).FileSystemType', 'unknown')
    out = capture(['df', '-T', str(path)]) or capture(['df', str(path)])
    lines = out.splitlines()
    if len(lines) >= 2:
        fields = lines[-1].split()
        return f'{fields[1]} {fields[0]}' if len(fields) > 6 else fields[0]
    return 'unknown'


def device_of(path: Path) -> str:
    if PLATFORM == 'windows':
        drive = str(Path(path).resolve())[0]
        found = powershell(
            f'$d = (Get-Partition -DriveLetter {drive} | Get-Disk); '
            f'$p = Get-PhysicalDisk | Where-Object {{$_.DeviceId -eq $d.Number}}; '
            f'"{{0}}, {{1}}, {{2}}" -f $d.FriendlyName, $p.MediaType, $p.BusType')
        return found or 'unknown'
    if PLATFORM == 'macos':
        out = capture(['diskutil', 'info', str(path)])
        wanted = ('Device / Media Name', 'Solid State', 'Protocol')
        found = [line.split(':', 1)[1].strip() for line in out.splitlines()
                 if line.strip().startswith(wanted) and ':' in line]
        return ', '.join(found) or 'unknown'

    source = capture(['df', '--output=source', str(path)]).splitlines()
    if len(source) < 2:
        return 'unknown'
    parent = capture(['lsblk', '-no', 'PKNAME', source[-1].strip()]).strip()
    if not parent:
        return 'unknown'
    block = Path('/sys/block') / parent

    def read(where: Path) -> str:
        try:
            return where.read_text().strip()
        except OSError:
            return ''

    parts = [read(block / 'device' / 'model') or parent]
    if read(block / 'queue' / 'rotational') == '1':
        parts.append('spinning')
    speed = read(block / 'device' / 'device' / 'current_link_speed')
    width = read(block / 'device' / 'device' / 'current_link_width')
    if speed:
        parts.append(f'{speed} x{width}' if width else speed)
    return ', '.join(parts)


def distro() -> str:
    if PLATFORM in ('linux', 'wsl'):
        try:
            for line in Path('/etc/os-release').read_text().splitlines():
                if line.startswith('PRETTY_NAME='):
                    return line.split('=', 1)[1].strip().strip('"')
        except OSError:
            pass
    return platform.platform()


def git_state(path: Path, untracked: bool = True) -> dict[str, Any]:
    head = capture(['git', '-C', str(path), 'rev-parse', 'HEAD'], 'not a git repo')
    argv = ['git', '-C', str(path), 'status', '--porcelain']
    if not untracked:
        argv.append('-uno')
    ok, out = capture_ok(argv, timeout=300)
    if not ok:
        return {'head': head, 'clean': None, 'dirty': []}
    dirty = [line.split(maxsplit=1)[1].split(' -> ')[-1].strip('"')
             for line in out.splitlines() if len(line.split(maxsplit=1)) == 2]
    return {'head': head, 'clean': not dirty, 'dirty': dirty}


def build_inputs(repo: Path) -> list[str]:
    try:
        text = (repo / 'Cargo.toml').read_text(encoding='utf-8')
    except OSError:
        return []
    members = re.search(r'members\s*=\s*\[(.*?)\]', text, re.S)
    if not members:
        return []
    return ['Cargo.toml', 'Cargo.lock'] + re.findall(r'"([^"]+)"', members.group(1))


def reaches_the_build(path: str, inputs: list[str]) -> bool:
    if not inputs:
        return True
    return any(path == entry or path.startswith(entry + '/') for entry in inputs)


def tool_version(binary: Path) -> str:
    return capture([str(binary), '--version'], 'missing').replace('\n', ' ').strip()


def collect_machine(tools: Path, corpus: Path, repo: Path) -> dict[str, Any]:
    head = git_state(corpus)
    mine = git_state(repo, untracked=False)
    inputs = build_inputs(repo)
    built_from = [p for p in mine['dirty'] if reaches_the_build(p, inputs)]
    beside_it = [p for p in mine['dirty'] if p not in built_from]
    return {
        'date': datetime.now().astimezone().isoformat(timespec='seconds'),
        'platform': PLATFORM,
        'arch': ARCH,
        'os': distro(),
        'kernel': platform.release(),
        'cpu': cpu_name(),
        'logical_cores': os.cpu_count(),
        'ram_bytes': ram_bytes(),
        'cpu_scaling': cpu_scaling(),
        'corpus_fs': filesystem_of(corpus),
        'corpus_device': device_of(corpus),
        'corpus': str(corpus),
        'corpus_head': head['head'],
        'corpus_clean': head['clean'] if head['clean'] is not None else 'could not tell',
        'tools': str(tools),
        'mezura_head': mine['head'],
        'mezura_clean': 'could not tell' if mine['clean'] is None else not built_from,
        'mezura_changed_before_building': sorted(built_from),
        'mezura_changed_beside_the_build': sorted(beside_it),
        'global_gitignore': capture(['git', 'config', '--get', 'core.excludesFile'], 'none'),
        'hyperfine': capture(['hyperfine', '--version'], 'missing'),
        'mezura': tool_version(tools / f'mezura{EXE}'),
        'scc': tool_version(tools / f'scc{EXE}'),
        'tokei': tool_version(tools / f'tokei{EXE}'),
    }


def download(url: str, dest: Path) -> None:
    say(f'   downloading {url}')
    try:
        with urllib.request.urlopen(url, timeout=120) as response, open(dest, 'wb') as out:
            shutil.copyfileobj(response, out)
    except Exception as error:
        raise SystemExit(f'could not download {url}\n{error}')


def setup_scc(tools: Path) -> None:
    binary = tools / f'scc{EXE}'
    if binary.exists():
        say(f'   scc already at {binary}')
        return
    asset = SCC_ASSETS.get((PLATFORM, ARCH))
    if not asset:
        raise SystemExit(f'no scc asset known for {PLATFORM}/{ARCH}; download it by hand into {tools}')
    url = f'https://github.com/boyter/scc/releases/download/v{SCC_VERSION}/{asset}'
    archive = tools / asset
    download(url, archive)
    if asset.endswith('.zip'):
        with zipfile.ZipFile(archive) as zf:
            zf.extract(f'scc{EXE}', tools)
    else:
        with tarfile.open(archive) as tf:
            tf.extract('scc', tools)
    archive.unlink()
    binary.chmod(0o755)
    say(f'   scc {SCC_VERSION} -> {binary}')


def setup_tokei(tools: Path) -> None:
    binary = tools / f'tokei{EXE}'
    if binary.exists():
        say(f'   tokei already at {binary}')
        return
    if not shutil.which('cargo'):
        raise SystemExit('cargo is needed to build tokei, and it is not on PATH')
    say(f'   cargo install tokei {TOKEI_VERSION}')
    result = run(['cargo', 'install', 'tokei', '--version', TOKEI_VERSION, '--locked'])
    if result.returncode != 0:
        raise SystemExit('cargo install tokei failed')
    built = HOME / '.cargo' / 'bin' / f'tokei{EXE}'
    shutil.copy2(built, binary)
    say(f'   tokei {TOKEI_VERSION} -> {binary}')


def setup_corpus(corpus: Path, definition: Definition) -> None:
    commit, remote = definition['commit'], definition['remote']
    head = capture(['git', '-C', str(corpus), 'rev-parse', 'HEAD']) if corpus.is_dir() else ''
    if commit and head == commit:
        say(f'   corpus already pinned at {commit[:9]}')
        return
    if not remote:
        if not corpus.is_dir():
            raise SystemExit(f'{definition["name"]} names no remote to fetch from, '
                             f'so {corpus} has to be there already')
        if commit:
            raise SystemExit(f'{corpus}\nis at {head or "no commit at all"}, and '
                             f'{definition["name"]} pins {commit}.\n'
                             f'There is no remote to fetch it from, so put the checkout on '
                             f'that commit yourself, or clear the commit from the definition.')
        say(f'   {definition["name"]} is taken as it stands at {corpus}')
        return
    corpus.mkdir(parents=True, exist_ok=True)
    if not (corpus / '.git').exists():
        run(['git', 'init', '-q', str(corpus)], check=True)
        run(['git', '-C', str(corpus), 'remote', 'add', 'origin', remote], check=True)
    say(f'   fetching {commit[:9]} from {remote}')
    run(['git', '-C', str(corpus), 'fetch', '--depth', '1', 'origin', commit], check=True)
    run(['git', '-C', str(corpus), 'checkout', '-q', 'FETCH_HEAD'], check=True)


def setup_mezura(tools: Path, repo: Path) -> None:
    binary = tools / f'mezura{EXE}'
    if not shutil.which('cargo'):
        raise SystemExit('cargo is needed to build mezura, and it is not on PATH')
    say(f'   cargo build --release in {repo}')
    result = run(['cargo', 'build', '--release'], cwd=repo)
    if result.returncode != 0:
        raise SystemExit('cargo build --release failed')
    shutil.copy2(repo / 'target' / 'release' / f'mezura{EXE}', binary)
    say(f'   mezura -> {binary}')


WINDOWS_HIGH_PERF = '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c'


def is_privileged() -> bool:
    if PLATFORM == 'windows':
        try:
            import ctypes
            return ctypes.windll.shell32.IsUserAnAdmin() != 0
        except Exception:
            return False
    return hasattr(os, 'geteuid') and os.geteuid() == 0


def prep_cpu_governor() -> Optional[PrepStep]:
    paths = sorted(Path('/sys/devices/system/cpu').glob('cpu[0-9]*/cpufreq/scaling_governor'))
    if not paths:
        return None
    try:
        current = {path: path.read_text().strip() for path in paths}
        available = paths[0].with_name('scaling_available_governors').read_text().split()
    except OSError:
        return None
    if 'performance' not in available or all(v == 'performance' for v in current.values()):
        return None

    def apply() -> None:
        for path in paths:
            try:
                path.write_text('performance')
            except OSError as error:
                say(f'   could not set {path}: {error}')

    def restore() -> None:
        for path, value in current.items():
            try:
                path.write_text(value)
            except OSError:
                pass

    was = '/'.join(sorted(set(current.values())))
    return {'what': f'cpu governor on {len(paths)} cpus: {was} -> performance',
            'apply': apply, 'restore': restore}


def prep_windows_power() -> Optional[PrepStep]:
    active = capture(['powercfg', '/getactivescheme'])
    guid = next((t for t in active.replace(':', ' ').split() if t.count('-') == 4), '')
    if not guid or guid.lower() == WINDOWS_HIGH_PERF:
        return None
    if WINDOWS_HIGH_PERF not in capture(['powercfg', '/list']).lower():
        return None

    def apply() -> None:
        run(['powercfg', '/setactive', WINDOWS_HIGH_PERF])

    def restore() -> None:
        run(['powercfg', '/setactive', guid])

    return {'what': f'power scheme: {guid} -> high performance',
            'apply': apply, 'restore': restore}


def prep_plan() -> list[PrepStep]:
    if PLATFORM in ('linux', 'wsl'):
        step = prep_cpu_governor()
    elif PLATFORM == 'windows':
        step = prep_windows_power()
    else:
        step = None
    return [step] if step else []


def how_to_elevate() -> tuple[str, str]:
    if PLATFORM == 'windows':
        return 'administrator', 'run it from a terminal opened with "Run as administrator"'
    argv = [a for a in sys.argv if a not in ('--setup', '--setup-only')]
    return 'root', f'sudo {sys.executable} {" ".join(argv)}'


def announce_prep(plan: list[PrepStep], assume_yes: bool) -> None:
    if not plan or is_privileged():
        return
    authority, how = how_to_elevate()
    say(f'Not running as {authority}, so the machine is measured exactly as it is now.')
    say(f'\nAs {authority} this run would first set:')
    for step in plan:
        say(f'   {step["what"]}')
    say('and put it back at the end, however the run ends. To do that:')
    say(f'\n   {how}\n')
    say('The numbers are real either way. But a cpu that clocks itself up and down')
    say('mid-run widens the spread, and a small difference can disappear into it.')
    if assume_yes:
        say('--yes was given, so carrying on as an ordinary user.')
        return
    if not sys.stdin.isatty():
        say('no terminal to ask at, so carrying on as an ordinary user.')
        return
    if input('Continue anyway, without it? [y/N] ').strip().lower() not in ('y', 'yes'):
        raise SystemExit('stopped. nothing was measured and nothing was changed.')


def apply_prep(plan: list[PrepStep], applied: list[PrepStep]) -> list[PrepStep]:
    if not plan or not is_privileged():
        return applied
    say('== machine preparation')
    for step in plan:
        say(f'   {step["what"]}')
        applied.append(step)
        step['apply']()
    return applied


def do_check(corpus: Path, binaries: Binaries, definition: Definition) -> int:
    mezura, control = binaries['mezura'], binaries['control']
    scc, tokei = binaries['scc'], binaries['tokei']
    mezura_pinned, scc_pinned, tokei_pinned = pinned_flags(definition)
    plans = (
        ('mezura', 't1', [mezura, corpus] + mezura_pinned + ['--output', 'json']),
        ('scc', 't1', [scc, corpus] + scc_pinned + ['--format', 'json']),
        ('tokei', 't1', [tokei, corpus] + tokei_pinned + ['--output', 'json']),
        ('mezura', 't2', [mezura, corpus, '--output', 'json']),
        ('scc', 't2', [scc, corpus, '--format', 'json']),
        ('tokei', 't2', [tokei, corpus, '--output', 'json']),
    )
    say(f'== check: {definition["name"]} at {corpus}')
    bad = []
    with tempfile.TemporaryDirectory() as scratch:
        for tool, tier, argv in plans:
            path = Path(scratch) / f'{tier}-{tool}.json'
            with open(path, 'w', encoding='utf-8') as out:
                result = run([str(a) for a in argv], stdout=out, stderr=subprocess.PIPE, text=True)
            if result.returncode != 0:
                reason = (result.stderr or '').strip().splitlines()
                say(f'   {tool:<7} {tier}  FAILED, exit {result.returncode}'
                    + (f': {reason[0]}' if reason else ''))
                bad.append(f'{tool} {tier}')
                continue
            totals = totals_from(tool, path)
            if not totals:
                say(f'   {tool:<7} {tier}  ran, but no counts could be read from its output')
                bad.append(f'{tool} {tier}')
                continue
            if not totals['files']:
                say(f'   {tool:<7} {tier}  counted nothing at all, so its share of the '
                    f'{definition["name"]} definition names no language this tree has')
                bad.append(f'{tool} {tier}')
                continue
            files = crunch(totals['files'])
            lines = crunch(totals['lines'])
            say(f'   {tool:<7} {tier}  ok   {files:>10} files  {lines:>14} lines')

        marker = Path(scratch) / 'hyperfine.json'
        result = run(['hyperfine', '-N', '--warmup', '0', '--runs', '2',
                      '--export-json', str(marker), join_cmd([mezura, corpus])],
                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        if result.returncode != 0 or not marker.is_file():
            say(f'   hyperfine     FAILED: {(result.stderr or "").strip().splitlines()[:1]}')
            bad.append('hyperfine')
        else:
            say('   hyperfine     ok')

        if not control.exists():
            say('   control       not made yet, the next run will copy it from mezura')
        elif not filecmp.cmp(mezura, control, shallow=False):
            say('   control       stale, the next run will refresh it from mezura')
        elif run([str(control), '--version'], stdout=subprocess.DEVNULL,
                 stderr=subprocess.DEVNULL).returncode != 0:
            say('   control       will not run')
            bad.append('control')
        else:
            say('   control       ok')

    if bad:
        say(f'{len(bad)} of {len(plans) + 1} checks failed: {", ".join(bad)}')
        return 1
    say('all good. nothing was written.')
    return 0


def crunch(number: Any) -> str:
    return f'{number:,}' if isinstance(number, int) else str(number)


def do_setup(tools: Path, corpus: Path, repo: Path, definition: Definition) -> None:
    say('== setup')
    (tools / 'control').mkdir(parents=True, exist_ok=True)
    setup_scc(tools)
    setup_tokei(tools)
    setup_corpus(corpus, definition)
    setup_mezura(tools, repo)


class Runner:
    def __init__(self, res: Path, warmup: int, runs: int, settle: int) -> None:
        self.res = res
        self.warmup = warmup
        self.runs = runs
        self.settle = settle
        self.failures = []

    def hyperfine(self, name: str, commands: list[str],
                  env: Optional[dict[str, str]] = None) -> None:
        say(f'>> {name}')
        argv = ['hyperfine', '-N', '--warmup', str(self.warmup), '--runs', str(self.runs)]
        if self.settle:
            argv += ['--setup', f'{quote(sys.executable)} -c "import time; time.sleep({self.settle})"']
        argv += ['--export-json', str(self.res / f'{name}.json'),
                 '--export-markdown', str(self.res / f'{name}.md')]
        argv += commands
        merged = dict(os.environ, **(env or {}))
        result = run(argv, env=merged)
        if result.returncode != 0:
            say(f'WARNING: hyperfine reported a problem on {name}')
            self.failures.append(name)

    def capture_output(self, name: str, command: list[str], as_json: bool) -> None:
        suffix = 'json' if as_json else 'txt'
        with open(self.res / 'out' / f'{name}.{suffix}', 'w', encoding='utf-8') as out:
            result = run(command, stdout=out,
                         stderr=subprocess.DEVNULL if as_json else subprocess.STDOUT)
        if result.returncode != 0:
            label = f'{name}.{suffix}'
            say(f'WARNING: {command[0]} exited {result.returncode} while writing {label}')
            self.failures.append(label)


def totals_from(tool: str, path: Path) -> Optional[Totals]:
    try:
        with open(path, encoding='utf-8') as handle:
            data = json.load(handle)
        return read_totals(tool, data)
    except Exception as error:
        say(f'WARNING: no counts from {path.name}: {error}')
        return None


def read_totals(tool: str, data: Any) -> Optional[Totals]:
    if tool == 'mezura':
        total = data['total']
        third = 'blanks' if 'blanks' in total else 'extra'
        return {'model': data['scope']['counting'], 'files': total['files'],
                'lines': total['lines'], 'code': total['code'],
                'comments': total['comments'], 'third': third, 'value': total[third]}
    if tool == 'scc':
        return {'model': 'region', 'files': sum(x['Count'] for x in data),
                'lines': sum(x['Lines'] for x in data), 'code': sum(x['Code'] for x in data),
                'comments': sum(x['Comment'] for x in data), 'third': 'blanks',
                'value': sum(x['Blank'] for x in data)}
    if tool == 'tokei':
        total = data['Total']
        files = sum(len(v.get('reports', [])) for k, v in data.items() if k != 'Total')
        return {'model': 'region', 'files': files,
                'lines': total['code'] + total['comments'] + total['blanks'],
                'code': total['code'], 'comments': total['comments'],
                'third': 'blanks', 'value': total['blanks']}
    return None


def first_token(command: str) -> str:
    if command.startswith('"'):
        end = command.find('"', 1)
        return command[1:end] if end > 0 else command
    return command.split(' ', 1)[0]


def tool_of(command: str, binaries: dict[str, str]) -> str:
    return binaries.get(first_token(command), 'unknown')


def build_record(res: Path, machine: dict[str, Any], settings: dict[str, Any],
                 counts: dict[tuple[str, str], Optional[Totals]],
                 binaries: dict[str, str]) -> dict[str, Any]:
    measurements = []
    for path in sorted(res.glob('*.json')):
        if path.name == 'run.json':
            continue
        try:
            with open(path, encoding='utf-8') as handle:
                document = json.load(handle)
        except Exception as error:
            say(f'WARNING: skipping {path.name}: {error}')
            continue
        for result in document.get('results', []):
            tier = path.stem[:2] if path.stem.startswith(('t1', 't2')) else None
            tool = tool_of(result['command'], binaries)
            counted = counts.get((tier, tool)) if tier else None
            measurements.append({
                'set': path.stem,
                'tool': tool,
                'command': result['command'],
                'mean_s': round(result['mean'], 6),
                'stddev_s': round(result.get('stddev') or 0, 6),
                'median_s': round(result['median'], 6),
                'min_s': round(result['min'], 6),
                'max_s': round(result['max'], 6),
                'user_s': round(result.get('user') or 0, 6),
                'system_s': round(result.get('system') or 0, 6),
                'runs': len(result['times']),
                'counted_files': counted['files'] if counted else None,
                'counted_lines': counted['lines'] if counted else None,
                'lines_per_sec': round(counted['lines'] / result['mean']) if counted and result['mean'] else None,
            })
    return {
        'format': 1,
        'machine': machine,
        'settings': settings,
        'counts': [dict(set=tier, tool=tool, **values)
                   for (tier, tool), values in sorted(counts.items()) if values],
        'measurements': measurements,
    }


def write_csvs(res: Path, record: dict[str, Any]) -> None:
    with open(res / 'summary.csv', 'w', newline='', encoding='utf-8') as handle:
        fields = ['set', 'tool', 'command', 'mean_s', 'stddev_s', 'median_s', 'min_s', 'max_s',
                  'user_s', 'system_s', 'runs', 'counted_files', 'counted_lines', 'lines_per_sec']
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(record['measurements'])
    with open(res / 'counts.csv', 'w', newline='', encoding='utf-8') as handle:
        fields = ['set', 'tool', 'model', 'files', 'lines', 'code', 'comments', 'third', 'value']
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(record['counts'])


INDEX_FIELDS = ['stamp', 'corpus', 'platform', 'mezura_head', 'mezura_clean',
                'corpus_head', 'corpus_pinned', 'prepared', 'worst_control',
                't1_mezura_s', 't1_scc_s', 't1_tokei_s', 'path']


def worst_control(measurements: list[dict[str, Any]]) -> Any:
    worst = None
    for name in ('control-start', 'control-end'):
        means = [m['mean_s'] for m in measurements if m['set'] == name and m['mean_s']]
        if len(means) == 2:
            ratio = max(means) / min(means)
            worst = ratio if worst is None else max(worst, ratio)
    return round(worst, 4) if worst else ''


def mean_of(measurements: list[dict[str, Any]], name: str, tool: str) -> Any:
    for m in measurements:
        if m['set'] == name and m['tool'] == tool:
            return m['mean_s']
    return ''


def append_index(outroot: Path, record: dict[str, Any], res: Path) -> None:
    path = outroot / 'index.csv'
    machine, settings = record['machine'], record['settings']
    row = {
        'stamp': settings['stamp'],
        'corpus': settings['corpus'],
        'platform': machine['platform'],
        'mezura_head': str(machine['mezura_head'])[:12],
        'mezura_clean': machine['mezura_clean'],
        'corpus_head': str(machine['corpus_head'])[:12],
        'corpus_pinned': settings['corpus_pinned'],
        'prepared': bool(settings['machine_prepared']),
        'worst_control': worst_control(record['measurements']),
        't1_mezura_s': mean_of(record['measurements'], 't1-fwd', 'mezura'),
        't1_scc_s': mean_of(record['measurements'], 't1-fwd', 'scc'),
        't1_tokei_s': mean_of(record['measurements'], 't1-fwd', 'tokei'),
        'path': str(res.relative_to(outroot)),
    }
    fresh = not path.exists()
    with open(path, 'a', newline='', encoding='utf-8') as handle:
        writer = csv.DictWriter(handle, fieldnames=INDEX_FIELDS)
        if fresh:
            writer.writeheader()
        writer.writerow(row)


def write_notes(res: Path, stamp: str, tools: Path, corpus: Path,
                definition: Definition) -> None:
    pin = definition['commit'][:9] if definition['commit'] else 'not pinned'
    (res / 'notes.md').write_text(
        f'# Benchmark session notes {stamp}\n'
        f'- [ ] corpus and tools on a local disk, not a network or /mnt mount\n'
        f'      corpus: {corpus}\n'
        f'      tools:  {tools}\n'
        f'- [ ] same corpus as the session this is compared against '
        f'({definition["name"]} @ {pin})\n'
        f'- [ ] machine quiet during the run\n'
        f'- [ ] mezura built from the working tree being measured\n'
        f'\nobservations:\n-\n', encoding='utf-8')


def read_corpus_def(script_dir: Path, given: str) -> Definition:
    named = script_dir / 'corpora' / f'{given}.conf'
    looks_like_path = os.sep in given or (os.altsep and os.altsep in given)
    path = under_home(Path(given)) if looks_like_path else named
    if not path.is_file() and not looks_like_path:
        path = under_home(Path(given))
    if not path.is_file():
        known = sorted(p.stem for p in (script_dir / 'corpora').glob('*.conf'))
        raise SystemExit(f'no corpus definition at {path}\n'
                         f'known: {", ".join(known) or "none"}')
    values = {key: '' for key in CORPUS_KEYS}
    for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if '=' not in line:
            raise SystemExit(f'{path}:{number}: not a "name = value" line: {line}')
        key, value = line.split('=', 1)
        key, value = key.strip(), value.strip()
        if key not in CORPUS_KEYS:
            raise SystemExit(f'{path}:{number}: unknown setting {key!r}, '
                             f'expected one of {", ".join(CORPUS_KEYS)}')
        values[key] = value
    missing = [key for key in CORPUS_REQUIRED if not values[key]]
    if missing:
        raise SystemExit(f'{path}: {", ".join(missing)} must be given')
    values['path'] = str(path)
    return values


CONFIG_KEYS = ('tools', 'corpus', 'out')


def read_config(path: Path) -> dict[str, str]:
    values = {}
    try:
        text = path.read_text(encoding='utf-8')
    except OSError:
        return values
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if '=' not in line:
            raise SystemExit(f'{path}:{number}: not a "name = value" line: {line}')
        key, value = line.split('=', 1)
        key, value = key.strip(), value.strip()
        if key not in CONFIG_KEYS:
            raise SystemExit(f'{path}:{number}: unknown setting {key!r}, '
                             f'expected one of {", ".join(CONFIG_KEYS)}')
        if value:
            values[key] = value
    return values


def parse_args(config: dict[str, str]) -> argparse.Namespace:
    def default_for(key: str, env_name: str, fallback: Any) -> str:
        return os.environ.get(env_name) or config.get(key) or fallback

    parser = argparse.ArgumentParser(description='Run the mezura benchmark suite.')
    parser.add_argument('--tools', type=Path,
                        default=Path(default_for('tools', 'MEZURA_BENCH_TOOLS', DEFAULT_TOOLS)),
                        help='directory holding the mezura, scc and tokei binaries')
    parser.add_argument('--corpus', type=Path,
                        default=Path(default_for('corpus', 'MEZURA_BENCH_CORPUS', DEFAULT_CORPUS)),
                        help='the checkout that gets counted')
    parser.add_argument('--out', type=Path,
                        default=Path(config['out']) if 'out' in config else None,
                        help='where the results directory is written (default: results/ beside this script)')
    parser.add_argument('--corpus-def', default='linux',
                        help='name under corpora/, or a path to a corpus definition file')
    parser.add_argument('--setup', action='store_true',
                        help='fetch scc, tokei and the corpus, and build mezura, before measuring')
    parser.add_argument('--setup-only', action='store_true', help='do the setup and stop')
    parser.add_argument('--check', action='store_true',
                        help='run each tool once, prove the numbers can be read, write nothing')
    parser.add_argument('--keep-raw', action='store_true',
                        help='keep the raw tool output instead of removing it once read')
    parser.add_argument('--no-prep', action='store_true',
                        help='do not touch the cpu governor or power scheme, even as root')
    parser.add_argument('--yes', action='store_true',
                        help='do not stop to ask when the machine could not be prepared')
    parser.add_argument('--warmup', type=int, default=None)
    parser.add_argument('--runs', type=int, default=None)
    parser.add_argument('--settle', type=int, default=None)
    return parser.parse_args()


def require_ready(tools: Path, corpus: Path, definition: Definition) -> Binaries:
    binaries = {name: tools / f'{name}{EXE}' for name in ('mezura', 'scc', 'tokei')}
    binaries['control'] = tools / 'control' / f'mezura{EXE}'
    missing = [str(b) for name, b in binaries.items() if name != 'control' and not b.exists()]
    if missing:
        raise SystemExit('these are not there, run with --setup:\n  ' + '\n  '.join(missing))
    if not corpus.is_dir():
        raise SystemExit(f'the corpus is not there: {corpus}\nrun with --setup')
    if not shutil.which('hyperfine'):
        raise SystemExit('hyperfine is not on PATH')

    head = capture(['git', '-C', str(corpus), 'rev-parse', 'HEAD'], 'not a git repo')
    if definition['commit'] and head != definition['commit']:
        raise SystemExit(
            f'{corpus}\n'
            f'is not at the commit {definition["name"]} is defined against.\n'
            f'  wanted: {definition["commit"]}\n'
            f'  found:  {head}\n'
            f'Numbers measured against a different tree compare with nothing else.\n\n'
            f'  python3 {sys.argv[0]} --setup\n\n'
            f'puts it right. To measure a tree as it stands, write a corpus definition for '
            f'it with no commit.')
    return binaries


def main() -> int:
    script_dir = Path(__file__).resolve().parent
    args = parse_args(read_config(script_dir / 'benchmark.conf'))
    definition = read_corpus_def(script_dir, args.corpus_def)
    repo = script_dir.parent
    tools = under_home(args.tools)
    corpus = under_home(args.corpus)

    for name in ('CLICOLOR_FORCE', 'MEZURA_PHASE_TIMING', 'SCC_CONFIG_PATH', 'RAYON_NUM_THREADS'):
        os.environ.pop(name, None)

    if args.check and not (args.setup or args.setup_only):
        return do_check(corpus, require_ready(tools, corpus, definition), definition)

    if args.setup or args.setup_only:
        if is_privileged():
            authority = 'administrator' if PLATFORM == 'windows' else 'root'
            raise SystemExit(
                f'the setup must not run as {authority}. It builds with cargo and writes into\n'
                f'  {tools}\n'
                f'  {HOME / ".cargo"}\n'
                f'  {repo / "target"}\n'
                f'and all of it would come out owned by {authority}, which breaks your next\n'
                f'ordinary build. cargo is usually not even on {authority}\'s PATH.\n\n'
                f'  {sys.executable} {" ".join(sys.argv)}\n\n'
                f'Run the setup as yourself, then measure with sudo.')
        tools.mkdir(parents=True, exist_ok=True)
        do_setup(tools, corpus, repo, definition)
        failed = do_check(corpus, require_ready(tools, corpus, definition), definition)
        if failed or args.setup_only:
            return failed
        say('')

    plan = [] if args.no_prep else prep_plan()
    announce_prep(plan, args.yes)

    binaries = require_ready(tools, corpus, definition)
    mezura, control = binaries['mezura'], binaries['control']
    scc, tokei = binaries['scc'], binaries['tokei']

    control.parent.mkdir(parents=True, exist_ok=True)
    if not control.exists() or not filecmp.cmp(mezura, control, shallow=False):
        say('refreshing the control copy of mezura')
        shutil.copy2(mezura, control)

    applied = []
    try:
        applied = apply_prep(plan, applied)
        return measure(args, tools, corpus, mezura, control, scc, tokei, applied, definition)
    finally:
        for step in reversed(applied):
            say(f'putting back: {step["what"]}')
            try:
                step['restore']()
            except Exception as error:
                say(f'WARNING: could not put back {step["what"]}: {error}')


def measure(args: argparse.Namespace, tools: Path, corpus: Path, mezura: Path, control: Path,
            scc: Path, tokei: Path, applied: list[PrepStep], definition: Definition) -> int:
    script_dir = Path(__file__).resolve().parent
    repo = script_dir.parent

    target = corpus
    warmup = args.warmup if args.warmup is not None else 3
    runs = args.runs if args.runs is not None else 15
    settle = args.settle if args.settle is not None else 3

    stamp = datetime.now().strftime('%Y%m%d-%H%M%S')
    outroot = (under_home(args.out) if args.out else script_dir / 'results')
    res = outroot / definition['name'] / PLATFORM / stamp
    if res.exists():
        raise SystemExit(f'{res} is already there, refusing to write over it')
    (res / 'out').mkdir(parents=True, exist_ok=True)

    transcript = Transcript(res / 'transcript.txt')
    transcript.start()
    try:
        return run_phases(args, tools, corpus, repo, mezura, control, scc, tokei, applied,
                          definition, res, outroot, target, warmup, runs, settle, stamp)
    finally:
        transcript.stop()


def run_phases(args: argparse.Namespace, tools: Path, corpus: Path, repo: Path, mezura: Path,
               control: Path, scc: Path, tokei: Path, applied: list[PrepStep],
               definition: Definition, res: Path, outroot: Path, target: Path,
               warmup: int, runs: int, settle: int, stamp: str) -> int:
    say('== phase 0: machine state')
    machine = collect_machine(tools, corpus, repo)
    (res / 'machine.txt').write_text(
        '\n'.join(f'{k}: {v}' for k, v in machine.items()) + '\n', encoding='utf-8')
    for line in (res / 'machine.txt').read_text(encoding='utf-8').splitlines():
        say('   ' + line)
    write_notes(res, stamp, tools, corpus, definition)

    runner = Runner(res, warmup, runs, settle)

    mezura_pinned, scc_pinned, tokei_pinned = pinned_flags(definition)
    m1 = join_cmd([mezura, target] + mezura_pinned)
    s1 = join_cmd([scc, target] + scc_pinned)
    k1 = join_cmd([tokei, target] + tokei_pinned)
    m2 = join_cmd([mezura, target])
    s2 = join_cmd([scc, target])
    k2 = join_cmd([tokei, target])

    say('== phase 0b: output and JSON captures (also the settling runs)')
    for name, argv in (
        ('t1-mezura', [mezura, target] + mezura_pinned),
        ('t1-scc', [scc, target] + scc_pinned),
        ('t1-tokei', [tokei, target] + tokei_pinned),
        ('t2-mezura', [mezura, target]),
        ('t2-scc', [scc, target]),
        ('t2-tokei', [tokei, target]),
    ):
        runner.capture_output(name, [str(a) for a in argv], as_json=False)
    for name, argv in (
        ('t1-mezura', [mezura, target] + mezura_pinned + ['--output', 'json']),
        ('t1-scc', [scc, target] + scc_pinned + ['--format', 'json']),
        ('t1-tokei', [tokei, target] + tokei_pinned + ['--output', 'json']),
        ('t2-mezura', [mezura, target, '--output', 'json']),
        ('t2-scc', [scc, target, '--format', 'json']),
        ('t2-tokei', [tokei, target, '--output', 'json']),
    ):
        runner.capture_output(name, [str(a) for a in argv], as_json=True)

    say('== phase 1: opening control run (gate: 1.00x inside the interval, or stop here)')
    runner.hyperfine('control-start', [join_cmd([mezura, target]), join_cmd([control, target])])

    say('== phase 2: table 1, same work')
    runner.hyperfine('t1-fwd', [m1, s1, k1])
    runner.hyperfine('t1-rev', [k1, s1, m1])

    say('== phase 3: table 2, out of the box')
    runner.hyperfine('t2-fwd', [m2, s2, k2])
    runner.hyperfine('t2-rev', [k2, s2, m2])

    say('== phase 4: thread sweep')
    runner.hyperfine('sweep-scc', [
            s1,
            f'{s1} --file-process-job-workers 32',
            f'{s1} --file-process-job-workers 64',
            f'{s1} --file-process-job-workers 32 --directory-walker-job-workers 16',
            f'{s1} --file-process-job-workers 64 --directory-walker-job-workers 16'])
    runner.hyperfine('sweep-mezura', [
            m1,
            f'{m1} --threads 4 64',
            f'{m1} --threads 16 64',
            f'{m1} --threads 8 32',
            f'{m1} --threads 8 128',
            f'{m1} --threads 16 128'])
    say('== phase 5: closing control run (gate: still 1.00x, or the machine drifted)')
    runner.hyperfine('control-end', [join_cmd([mezura, target]), join_cmd([control, target])])

    say('== summary')
    binaries = {str(mezura): 'mezura', str(control): 'mezura',
                str(scc): 'scc', str(tokei): 'tokei'}
    counts = {}
    for tool in ('mezura', 'scc', 'tokei'):
        for tier in ('t1', 't2'):
            counts[(tier, tool)] = totals_from(tool, res / 'out' / f'{tier}-{tool}.json')

    settings = {
        'stamp': stamp, 'target': str(target),
        'warmup': warmup, 'runs': runs, 'settle': settle,
        'corpus': definition['name'], 'corpus_def': definition['path'],
        'mezura_pinned': mezura_pinned, 'scc_pinned': scc_pinned, 'tokei_pinned': tokei_pinned,
        'corpus_commit': definition['commit'], 'hyperfine_failures': runner.failures,
        'corpus_pinned': bool(definition['commit'])
                         and machine['corpus_head'] == definition['commit'],
        'machine_prepared': [step['what'] for step in applied],
    }
    try:
        record = build_record(res, machine, settings, counts, binaries)
        with open(res / 'run.json', 'w', encoding='utf-8') as handle:
            json.dump(record, handle, indent=1)
        write_csvs(res, record)
        append_index(outroot, record, res)
    except Exception as error:
        say(f'WARNING: the summary could not be written: {error}')
        say(f'         the raw output is kept in {res / "out"} and the hyperfine exports stand')
        return 1

    if not args.keep_raw:
        shutil.rmtree(res / 'out', ignore_errors=True)

    give_back_to_user(outroot)

    say(f'done. everything is in {res}')
    if runner.failures:
        say(f'hyperfine had trouble with: {", ".join(runner.failures)}')
    say('fill in notes.md, and check both control runs before believing anything else.')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        say('interrupted. anything that was changed on the machine has been put back.')
        sys.exit(130)
