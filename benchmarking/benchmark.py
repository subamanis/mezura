#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import tarfile
import time
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
RECORD_FORMAT = 1

CORPUS_KEYS = ('name', 'remote', 'commit', 'languages', 'types')
CORPUS_REQUIRED = ('name', 'languages', 'types')
# Native executables only: a script tool (cloc is Perl) would run as its interpreter's
# process, so the Defender exclusion checks would need to look at that name instead.
TOOLS = ('mezura', 'scc', 'tokei')

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

    def start(self) -> None:
        global ACTIVE_TRANSCRIPT
        try:
            self.file = open(self.path, 'w', encoding='utf-8', errors='replace')
        except OSError:
            self.file = None
            warn(f'could not capture a transcript into {self.path}')
            return
        ACTIVE_TRANSCRIPT = self

    def write(self, message: str) -> None:
        if self.file:
            try:
                self.file.write(ANSI_ESCAPES.sub('', message) + '\n')
                self.file.flush()
            except OSError:
                pass

    def stop(self) -> None:
        global ACTIVE_TRANSCRIPT
        ACTIVE_TRANSCRIPT = None
        if self.file:
            try:
                self.file.close()
            except OSError:
                pass
            self.file = None


ACTIVE_TRANSCRIPT: Optional[Transcript] = None


def say(message: str) -> None:
    print(message, flush=True)
    if ACTIVE_TRANSCRIPT is not None:
        ACTIVE_TRANSCRIPT.write(message)


COLOR_CODES = {'green': '32', 'red': '31', 'yellow': '33', 'bold': '1',
               'blue': '38;2;110;160;220'}
ANSI_ESCAPES = re.compile(r'\x1b\[[0-9;]*m')
COLORS_ON = False


def enable_colors() -> None:
    global COLORS_ON
    if 'NO_COLOR' in os.environ or not sys.stdout.isatty():
        return
    if PLATFORM == 'windows':
        try:
            import ctypes
            kernel = ctypes.windll.kernel32
            handle = kernel.GetStdHandle(-11)
            mode = ctypes.c_uint32()
            if not kernel.GetConsoleMode(handle, ctypes.byref(mode)):
                return
            kernel.SetConsoleMode(handle, mode.value | 0x0004)
        except Exception:
            return
    COLORS_ON = True


def paint(color: str, text: str) -> str:
    return f'\x1b[{COLOR_CODES[color]}m{text}\x1b[0m' if COLORS_ON else text


def header(title: str) -> None:
    say('')
    say(paint('bold', title))


def warn(message: str) -> None:
    say(paint('yellow', 'WARNING: ') + message)


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


def powershell_ok(script: str) -> tuple[bool, str]:
    return capture_ok(['powershell', '-NoProfile', '-Command', script])


def quote(part: Any) -> str:
    part = str(part).replace('\\', '/')
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


def parse_porcelain(out: str) -> list[str]:
    return [line.split(maxsplit=1)[1].split(' -> ')[-1].strip('"')
            for line in out.splitlines() if len(line.split(maxsplit=1)) == 2]


def git_state(path: Path, untracked: bool = True) -> dict[str, Any]:
    head = capture(['git', '-C', str(path), 'rev-parse', 'HEAD'], 'not a git repo')
    argv = ['git', '-C', str(path), 'status', '--porcelain']
    if not untracked:
        argv.append('-uno')
    ok, out = capture_ok(argv, timeout=300)
    if not ok:
        return {'head': head, 'clean': None, 'dirty': []}
    dirty = parse_porcelain(out)
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
        **{name: tool_version(tools / f'{name}{EXE}') for name in TOOLS},
    }


def parse_exclusion_list(raw: str) -> Optional[list[str]]:
    if raw.strip().upper().startswith('N/A'):
        return None
    return [entry.strip() for entry in raw.split('|') if entry.strip()]


def read_defender_list(property_name: str) -> Optional[list[str]]:
    ok, out = powershell_ok(
        f'$ErrorActionPreference = "Stop"; '
        f'try {{ $p = Get-MpPreference; '
        f'if (-not $p.PSObject.Properties.Match("{property_name}").Count) {{ exit 4 }}; '
        f'$p.{property_name} -join "|" }} catch {{ exit 3 }}')
    if not ok:
        return None
    return parse_exclusion_list(out)


def sits_under(path: Path, roots: list[str]) -> bool:
    text = str(path).replace('/', '\\').lower().rstrip('\\')
    for root in roots:
        prefix = root.replace('/', '\\').lower().rstrip('\\')
        if text == prefix or text.startswith(prefix + '\\'):
            return True
    return False


def defender_state(corpus: Path, binaries: Binaries) -> dict[str, Any]:
    if PLATFORM != 'windows':
        state = {'defender_realtime': 'not applicable',
                 'defender_corpus_excluded': 'not applicable'}
        for tool in TOOLS:
            state[f'defender_process_excluded_{tool}'] = 'not applicable'
            state[f'defender_binary_excluded_{tool}'] = 'not applicable'
        return state
    unknown = ('unknown (needs admin)' if not is_privileged()
               else 'unknown (MS Defender could not be queried)')
    processes = read_defender_list('ExclusionProcess')
    paths = read_defender_list('ExclusionPath')
    state = {'defender_realtime':
             powershell('(Get-MpComputerStatus).RealTimeProtectionEnabled', 'unknown')}
    state['defender_corpus_excluded'] = unknown if paths is None else sits_under(corpus, paths)
    entries = None if processes is None else [p.replace('/', '\\').lower() for p in processes]
    for tool in TOOLS:
        binary = binaries[tool]
        if entries is None:
            state[f'defender_process_excluded_{tool}'] = unknown
        else:
            state[f'defender_process_excluded_{tool}'] = (
                binary.name.lower() in entries
                or str(binary).replace('/', '\\').lower() in entries)
        state[f'defender_binary_excluded_{tool}'] = (
            unknown if paths is None else sits_under(binary, paths))
    return state


def find_unequal_exclusions(state: dict[str, Any]) -> Optional[str]:
    problems = []
    for kind, field in (('process', 'defender_process_excluded'),
                        ('binary path', 'defender_binary_excluded')):
        values = {tool: state[f'{field}_{tool}'] for tool in TOOLS}
        if any(not isinstance(v, bool) for v in values.values()):
            continue
        if len(set(values.values())) == 1:
            continue
        excluded = ', '.join(sorted(t for t, v in values.items() if v))
        included = ', '.join(sorted(t for t, v in values.items() if not v))
        problems.append(f'{kind} exclusions cover {excluded} and not {included}')
    return '. '.join(problems) or None


def refuse_asymmetric_exclusions(state: dict[str, Any], allowed: bool) -> Optional[str]:
    unequal = find_unequal_exclusions(state)
    if not unequal:
        return None
    if not allowed:
        raise SystemExit(
            f'MS Defender does not treat the three tools equally:\n{unequal}.\n\n'
            f'Files opened by an excluded process are never scanned in real time, so this\n'
            f'run would measure which tool escaped the antivirus, not which counts faster.\n\n'
            f'  Get-MpPreference | Select -ExpandProperty ExclusionProcess\n'
            f'  Get-MpPreference | Select -ExpandProperty ExclusionPath\n'
            f'  Remove-MpPreference -ExclusionProcess <name>.exe\n\n'
            f'Either exclude all three or none, or rerun with --allow-unequal-exclusions\n'
            f'to measure anyway and have the results marked as such.')
    warn(f'measuring with unequal MS Defender exclusions: {unequal}')
    warn('the results may not be representative of real performance')
    return unequal


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
        raise SystemExit(f'no scc asset known for {PLATFORM}/{ARCH}. Download it by hand into {tools}')
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
    with tempfile.TemporaryDirectory() as scratch:
        result = run(['cargo', 'install', 'tokei', '--version', TOKEI_VERSION, '--locked',
                      '--root', scratch])
        if result.returncode != 0:
            raise SystemExit('cargo install tokei failed')
        built = Path(scratch) / 'bin' / f'tokei{EXE}'
        if not built.is_file():
            raise SystemExit(f'cargo reported success and {built} is not there')
        shutil.copy2(built, binary)
    say(f'   tokei {TOKEI_VERSION} -> {binary}')


def run_git_or_stop(argv: list[str]) -> None:
    if run(argv).returncode != 0:
        raise SystemExit('this failed, and the corpus cannot be set up without it:\n  '
                         + ' '.join(argv))


def setup_corpus(corpus: Path, definition: Definition) -> None:
    commit, remote = definition['commit'], definition['remote']
    head = capture(['git', '-C', str(corpus), 'rev-parse', 'HEAD']) if corpus.is_dir() else ''
    if commit and head == commit:
        say(f'   corpus already pinned at {commit[:9]}')
        return
    checkout = (corpus / '.git').exists() and bool(head)
    if not commit and (checkout or (corpus.is_dir() and not remote)):
        say(f'   {definition["name"]} is taken as it stands at {corpus}')
        return
    if not remote:
        if not corpus.is_dir():
            raise SystemExit(f'{definition["name"]} names no remote to fetch from, '
                             f'so {corpus} has to be there already')
        raise SystemExit(f'{corpus}\nis at {head or "no commit at all"}, and '
                         f'{definition["name"]} pins {commit}.\n'
                         f'There is no remote to fetch it from, so put the checkout on '
                         f'that commit yourself, or clear the commit from the definition.')
    if (not commit and not checkout and corpus.is_dir()
            and any(p.name != '.git' for p in corpus.iterdir())):
        raise SystemExit(f'{corpus}\nalready holds files and is not a git checkout, so the '
                         f'setup will not write over them. Empty it, point the definition '
                         f'elsewhere, or clear its remote to measure it as it stands.')
    corpus.mkdir(parents=True, exist_ok=True)
    if not (corpus / '.git').exists():
        run_git_or_stop(['git', 'init', '-q', str(corpus)])
        run_git_or_stop(['git', '-C', str(corpus), 'remote', 'add', 'origin', remote])
    if commit:
        say(f'   fetching {commit[:9]} from {remote}')
    else:
        say(f'   cloning the default branch of {remote}')
    run_git_or_stop(['git', '-C', str(corpus), 'fetch', '--depth', '1', 'origin',
                     commit or 'HEAD'])
    run_git_or_stop(['git', '-C', str(corpus), 'checkout', '-q', 'FETCH_HEAD'])


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
                warn(f'could not set {path}: {error}')

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
    return 'root', f'sudo {sys.executable} {" ".join(sys.argv)}'


def announce_prep(plan: list[PrepStep], assume_yes: bool) -> None:
    if not plan or is_privileged():
        return
    authority, how = how_to_elevate()
    say('')
    say(f'Not running as {authority}, so the machine is measured exactly as it is now.')
    say(f'As {authority} this run would first set:')
    for step in plan:
        say(f'   {step["what"]}')
    say('and put it back at the end, however the run ends. To do that:')
    say(f'\n   {how}\n')
    if assume_yes:
        say('--yes given, carrying on.')
        return
    if not sys.stdin.isatty():
        say('no terminal to ask at, carrying on.')
        return
    if input('Continue anyway, without it? [y/N] ').strip().lower() not in ('y', 'yes'):
        raise SystemExit('stopped.')


def apply_prep(plan: list[PrepStep], applied: list[PrepStep]) -> list[PrepStep]:
    if not plan or not is_privileged():
        return applied
    header('== machine preparation')
    for step in plan:
        say(f'   {step["what"]}')
        applied.append(step)
        step['apply']()
    return applied


def do_check(corpus: Path, binaries: Binaries, definition: Definition,
             allow_unequal: bool) -> int:
    mezura = binaries['mezura']
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
    header(f'== check: {definition["name"]} at {corpus}')
    bad = []
    with tempfile.TemporaryDirectory() as scratch:
        for tool, tier, argv in plans:
            path = Path(scratch) / f'{tier}-{tool}.json'
            with open(path, 'w', encoding='utf-8') as out:
                result = run([str(a) for a in argv], stdout=out, stderr=subprocess.PIPE, text=True)
            if result.returncode != 0:
                reason = (result.stderr or '').strip().splitlines()
                say(f'   {tool:<7} {tier}  ' + paint('red', f'FAILED, exit {result.returncode}')
                    + (f': {reason[0]}' if reason else ''))
                bad.append(f'{tool} {tier}')
                continue
            totals = totals_from(tool, path)
            if not totals:
                say(f'   {tool:<7} {tier}  '
                    + paint('red', 'ran, but no counts could be read from its output'))
                bad.append(f'{tool} {tier}')
                continue
            if not totals['files']:
                say(f'   {tool:<7} {tier}  '
                    + paint('red', f'counted nothing at all, so its share of the '
                                   f'{definition["name"]} definition names no language '
                                   f'this tree has'))
                bad.append(f'{tool} {tier}')
                continue
            files = crunch(totals['files'])
            lines = crunch(totals['lines'])
            say(f'   {tool:<7} {tier}  ' + paint('green', 'ok')
                + f'   {files:>10} files  {lines:>14} lines')

        say('')
        marker = Path(scratch) / 'hyperfine.json'
        result = run(['hyperfine', '-N', '--warmup', '0', '--runs', '2',
                      '--export-json', str(marker), join_cmd([mezura, corpus])],
                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        if result.returncode != 0 or not marker.is_file():
            say('   hyperfine   '
                + paint('red', f'FAILED: {(result.stderr or "").strip().splitlines()[:1]}'))
            bad.append('hyperfine')
        else:
            say('   hyperfine   ' + paint('green', 'ok'))

        if PLATFORM == 'windows':
            state = defender_state(corpus, binaries)
            values = [state[f'defender_process_excluded_{t}'] for t in TOOLS]
            if any(not isinstance(v, bool) for v in values):
                say('   MS Defender '
                    + paint('yellow', f'exclusions {state["defender_process_excluded_mezura"]}'))
            else:
                unequal = find_unequal_exclusions(state)
                if unequal:
                    if allow_unequal:
                        say('   MS Defender ' + paint('yellow', f'unequal: {unequal}'))
                    else:
                        say('   MS Defender ' + paint('red', f'unequal: {unequal}'))
                        bad.append('defender')
                else:
                    verdict = ('all three excluded' if values[0]
                               else paint('yellow', 'none excluded'))
                    say('   MS Defender ' + paint('green', 'ok') + f', {verdict}')
                if state['defender_corpus_excluded'] is True:
                    say('   corpus        under a Defender exclusion path')
                else:
                    say('   corpus      ' + paint('yellow', 'not under any Defender exclusion path'))

    say('')
    if bad:
        say(paint('red', f'{len(bad)} checks failed: {", ".join(bad)}'))
        return 1
    say(paint('green', 'all good.'))
    return 0


def crunch(number: Any) -> str:
    return f'{number:,}' if isinstance(number, int) else str(number)


def background_busy_percent(seconds: int = 4) -> Optional[float]:
    if PLATFORM in ('linux', 'wsl'):
        def snap() -> tuple[int, int]:
            numbers = [int(x) for x in Path('/proc/stat').read_text().splitlines()[0].split()[1:]]
            idle = numbers[3] + (numbers[4] if len(numbers) > 4 else 0)
            return idle, sum(numbers)
        idle_before, total_before = snap()
        time.sleep(seconds)
        idle_after, total_after = snap()
        total = total_after - total_before
        if not total:
            return None
        return round(100 * (1 - (idle_after - idle_before) / total), 1)
    if PLATFORM == 'windows':
        samples = []
        for _ in range(seconds):
            value = powershell(
                "(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor "
                "| Where-Object {$_.Name -eq '_Total'}).PercentProcessorTime").strip()
            if value.isdigit():
                samples.append(int(value))
            time.sleep(1)
        return round(sum(samples) / len(samples), 1) if samples else None
    return None


def do_noise(corpus: Path, binaries: Binaries, definition: Definition) -> int:
    cores = os.cpu_count() or 1
    header('== noise')
    busy = background_busy_percent()
    if busy is None:
        say('   background    not sampled on this platform')
    else:
        others = round(busy * cores / 100, 1)
        say(f'   background    {busy}% busy, ~{others} of {cores} cores')

    with tempfile.TemporaryDirectory() as scratch:
        marker = Path(scratch) / 'noise.json'
        result = run(['hyperfine', '-N', '--warmup', '0', '--runs', '5',
                      '--export-json', str(marker),
                      join_cmd([binaries['mezura'], corpus])],
                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        if result.returncode != 0 or not marker.is_file():
            say('   workload      hyperfine failed')
            return 1
        with open(marker, encoding='utf-8') as handle:
            data = json.load(handle)['results'][0]

    times = data['times']
    warm = times[1:]
    warm_mean = sum(warm) / len(warm)
    spread = round(100 * (max(warm) - min(warm)) / min(warm), 1) if min(warm) else 0.0
    cpu = (data.get('user') or 0) + (data.get('system') or 0)
    reached = round(cpu / data['mean'], 1) if data['mean'] else 0.0
    first = round(times[0] / warm_mean, 2) if warm_mean else 0.0

    say(f'   workload      mezura on {definition["name"]}, 5 runs')
    say(f'   spread        {spread}% ({round(min(warm) * 1000)} to {round(max(warm) * 1000)} ms)')
    say(f'   parallelism   {reached} of {cores} cores')
    say(f'   cache         {"cold" if first >= 1.5 else "warm"}, first run {first}x the warm mean')

    complaints = []
    if busy is not None and busy >= 10:
        complaints.append(f'background at {busy}%')
    if spread >= 15:
        complaints.append(f'spread at {spread}%')
    say('')
    if complaints:
        say(paint('red', f'not steady: {" and ".join(complaints)}.'))
        return 1
    say(paint('green', 'steady.'))
    return 0


def do_setup(tools: Path, corpus: Path, repo: Path, definition: Definition) -> None:
    header('== setup')
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
        self.warnings = []

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
        result = run(argv, env=merged, stderr=subprocess.PIPE,
                     encoding='utf-8', errors='replace')
        stderr_lines = [ANSI_ESCAPES.sub('', line).strip()
                        for line in (result.stderr or '').splitlines()]
        for text in stderr_lines:
            if text.startswith('Warning:'):
                message = text[len('Warning:'):].strip()
                warn(f'hyperfine on {name}: {message}')
                self.warnings.append(f'{name}: {message}')
        if result.returncode != 0:
            detail = next((t for t in stderr_lines if t.startswith('Error')),
                          next((t for t in stderr_lines if t), ''))
            warn(f'hyperfine reported a problem on {name}'
                 + (f': {detail}' if detail else ''))
            self.failures.append(name)

    def capture_output(self, name: str, command: list[str], as_json: bool) -> None:
        suffix = 'json' if as_json else 'txt'
        with open(self.res / 'out' / f'{name}.{suffix}', 'w', encoding='utf-8') as out:
            result = run(command, stdout=out,
                         stderr=subprocess.DEVNULL if as_json else subprocess.STDOUT)
        if result.returncode != 0:
            label = f'{name}.{suffix}'
            warn(f'{command[0]} exited {result.returncode} while writing {label}')
            self.failures.append(label)


def totals_from(tool: str, path: Path) -> Optional[Totals]:
    try:
        with open(path, encoding='utf-8') as handle:
            data = json.load(handle)
        return read_totals(tool, data)
    except Exception as error:
        warn(f'no counts from {path.name}: {error}')
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
    return binaries.get(first_token(command).replace('\\', '/'), 'unknown')


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
            warn(f'skipping {path.name}: {error}')
            continue
        for result in document.get('results', []):
            tier = path.stem[:2] if path.stem.startswith(('t1', 't2')) else None
            tool = tool_of(result['command'], binaries)
            counted = counts.get((tier, tool)) if tier else None
            cpu = (result.get('user') or 0) + (result.get('system') or 0)
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
                'parallelism': round(cpu / result['mean'], 2) if result['mean'] else None,
                'lines_per_cpu_s': round(counted['lines'] / cpu) if counted and cpu else None,
            })
    return {
        'format': RECORD_FORMAT,
        'machine': machine,
        'settings': settings,
        'counts': [dict(set=tier, tool=tool, **values)
                   for (tier, tool), values in sorted(counts.items()) if values],
        'measurements': measurements,
    }


def write_csvs(res: Path, record: dict[str, Any]) -> None:
    with open(res / 'summary.csv', 'w', newline='', encoding='utf-8') as handle:
        fields = ['set', 'tool', 'command', 'mean_s', 'stddev_s', 'median_s', 'min_s', 'max_s',
                  'user_s', 'system_s', 'runs', 'counted_files', 'counted_lines', 'lines_per_sec',
                  'parallelism', 'lines_per_cpu_s']
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(record['measurements'])
    with open(res / 'counts.csv', 'w', newline='', encoding='utf-8') as handle:
        fields = ['set', 'tool', 'model', 'files', 'lines', 'code', 'comments', 'third', 'value']
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(record['counts'])


def drift_of(measurements: list[dict[str, Any]]) -> Any:
    means = []
    for name in ('control-start', 'control-end'):
        phase = [m['mean_s'] for m in measurements if m['set'] == name and m['mean_s']]
        if not phase:
            return ''
        means.append(phase[0])
    return round(max(means) / min(means), 4)


def mean_of(measurements: list[dict[str, Any]], tier: str, tool: str) -> Any:
    means = [m['mean_s'] for m in measurements
             if m['tool'] == tool and m['set'] in (f'{tier}-fwd', f'{tier}-rev')
             and m['mean_s']]
    return round(sum(means) / len(means), 6) if means else ''


def pool_orders(m1: dict, m2: Optional[dict]) -> dict:
    if not m2:
        return dict(m1, single_order=True)
    mean = (m1['mean_s'] + m2['mean_s']) / 2
    spread = math.sqrt(((m1.get('stddev_s') or 0) ** 2 + (m2.get('stddev_s') or 0) ** 2) / 2
                       + (m1['mean_s'] - m2['mean_s']) ** 2 / 4)
    user = ((m1.get('user_s') or 0) + (m2.get('user_s') or 0)) / 2
    system = ((m1.get('system_s') or 0) + (m2.get('system_s') or 0)) / 2
    counted = m1.get('counted_lines')
    return {'tool': m1['tool'], 'mean_s': mean, 'stddev_s': spread,
            'user_s': user, 'system_s': system,
            'parallelism': round((user + system) / mean, 2) if mean else None,
            'lines_per_sec': round(counted / mean) if counted and mean else None,
            'counted_files': m1.get('counted_files'), 'counted_lines': counted}


def as_percent(ratio: Any) -> str:
    return f'{(float(ratio) - 1) * 100:.1f}%'


def format_wall(mean: Any, stddev: Any) -> str:
    if not mean:
        return ''
    text = f'{float(mean) * 1000:,.0f} ms'
    return f'{text} ± {float(stddev) * 1000:.0f}' if stddev else text


def format_versions(machine: dict[str, Any]) -> str:
    parts = []
    for tool in TOOLS:
        raw = str(machine.get(tool, ''))
        found = re.search(r'v?\d+\.\d+\S*( \([^)]*\))?', raw)
        parts.append(f'{tool} {found.group(0) if found else raw or "?"}')
    return ', '.join(parts)


def write_results_page(outroot: Path) -> None:
    entries = []
    for found in sorted(outroot.glob('*/*/*/run.json')):
        try:
            with open(found, encoding='utf-8') as handle:
                record = json.load(handle)
        except Exception:
            continue
        entries.append((record, found.parent.relative_to(outroot).as_posix()))
    if not entries:
        return

    latest = {}
    for record, rel in sorted(entries, key=lambda e: e[0]['settings']['stamp']):
        latest[(record['settings']['corpus'], record['machine']['platform'])] = (record, rel)

    lines = ['# Benchmark results', '',
             'Written by `benchmark.py` after every run, not edited by hand. What every term '
             'means and how this was measured: the two sections at the bottom.', '']

    newest_record = None
    single_order_seen = False
    for (corpus, platform), (record, rel) in sorted(
            latest.items(), key=lambda kv: kv[1][0]['settings']['stamp'], reverse=True):
        if newest_record is None:
            newest_record = record
        machine = record['machine']
        sett = record['settings']
        drift = drift_of(record['measurements'])
        prepared = bool(sett.get('machine_prepared'))
        ram_bytes = machine.get('ram_bytes')
        ram = f'{ram_bytes / 2 ** 30:.0f} GB usable RAM' if ram_bytes else 'RAM unknown'
        shown = {'windows': 'Windows', 'linux': 'Native Linux', 'wsl': 'WSL2',
                 'macos': 'macOS'}.get(platform, platform)
        lines += [f'## {corpus} corpus, {shown}, {sett["stamp"]}', '',
                  f'{machine.get("cpu", "?")}, {machine.get("logical_cores", "?")} threads, '
                  f'{ram}, '
                  f'{machine.get("os", "?")}  ',
                  f'corpus at `{str(machine.get("corpus_head", ""))[:9]}` on '
                  f'{machine.get("corpus_fs", "?")}, {machine.get("corpus_device", "?")}  ']
        if not sett.get('corpus_pinned', True):
            lines.append('not pinned, measured as it stands  ')
        lines += [format_versions(machine) + '  ',
                  f'{sett.get("warmup", "?")} warmups, {sett.get("runs", "?")} timed runs '
                  f'per command, {sett.get("settle", "?")} s pause between command series', '']
        order_moves = []
        record_single = False
        for tier, title in (('t1', 'Same work (all three pinned to the same languages and '
                                   'settings)'),
                            ('t2', 'Out of the box (each tool at its own defaults)')):
            first = {m['tool']: m for m in record['measurements'] if m['set'] == f'{tier}-fwd'}
            second = {m['tool']: m for m in record['measurements'] if m['set'] == f'{tier}-rev'}
            if not first:
                continue
            found = []
            for tool, m1 in first.items():
                m2 = second.get(tool)
                found.append(pool_orders(m1, m2))
                if m2 and min(m1['mean_s'], m2['mean_s']):
                    order_moves.append(abs(m1['mean_s'] - m2['mean_s'])
                                       / min(m1['mean_s'], m2['mean_s']))
            found.sort(key=lambda m: m['mean_s'])
            if any(m.get('single_order') for m in found):
                single_order_seen = True
                record_single = True
            fastest = found[0]['mean_s']
            lines += [f'#### {title}', '',
                      '| tool | wall | vs fastest | total cpu | parallelism | lines/s '
                      '| files | lines |',
                      '|---|---|---|---|---|---|---|---|']
            for m in found:
                cpu = (m.get('user_s') or 0) + (m.get('system_s') or 0)
                speed = m.get('lines_per_sec')
                speed = f'{speed / 1e6:.1f}M' if speed else ''
                files = f'{m["counted_files"]:,}' if m.get('counted_files') else ''
                counted = f'{m["counted_lines"]:,}' if m.get('counted_lines') else ''
                relative = f'{m["mean_s"] / fastest:.2f}x' if fastest else ''
                wall = format_wall(m.get('mean_s'), m.get('stddev_s'))
                if m.get('single_order'):
                    wall += ' (one order only)'
                lines.append(
                    f'| {m["tool"]} '
                    f'| {wall} '
                    f'| {relative} '
                    f'| {cpu:.2f} s '
                    f'| {m.get("parallelism") or ""} '
                    f'| {speed} | {files} | {counted} |')
            lines.append('')
        lines.append('Trust checks for this run:')
        if drift:
            lines.append(f'- **Machine steadiness**: the same binary, timed at the start of '
                         f'the run and again at the end, differed by {as_percent(drift)}.')
        if order_moves:
            scope = ('the tables that ran in both command orders pool the two'
                     if record_single else
                     'every table ran in both command orders and the numbers above pool '
                     'the two')
            lines.append(f'- **Command order**: {scope}. Swapping the order moved no tool '
                         f'by more than {max(order_moves) * 100:.1f}%.')
        lines.append('- **Power**: the cpu was set to its high-performance mode for the run '
                     'and restored after.' if prepared else
                     '- **Power**: the machine was measured as it was, with no settings '
                     'changed.')
        realtime = machine.get('defender_realtime')
        if realtime not in (None, 'not applicable'):
            state = f'real-time protection {"on" if realtime is True or realtime == "True" else realtime}'
            unequal = sett.get('unequal_exclusions')
            exclusions = [machine.get(f'defender_process_excluded_{t}') for t in TOOLS]
            if unequal:
                lines.append(f'- **Antivirus**: {state}, **unequal MS Defender exclusions '
                             f'({unequal}), measured with --allow-unequal-exclusions. The '
                             f'results may not be representative of real performance.**')
            else:
                if any(not isinstance(v, bool) for v in exclusions):
                    verdict = 'whether the tools are excluded from scanning could not be read'
                elif all(exclusions):
                    verdict = 'all three tools equally excluded from real-time scanning'
                elif not any(exclusions):
                    verdict = 'no tool excluded, all three scanned equally'
                else:
                    verdict = ('**unequal exclusions, the results may not be representative '
                               'of real performance**')
                lines.append(f'- **Antivirus**: {state}, {verdict}.')
        for warning in sett.get('hyperfine_warnings') or []:
            lines.append(f'- **hyperfine warning**: {warning.split(". ")[0].rstrip(".")}.')
        lines.append('')

    if len(entries) > len(latest):
        shown_names = {'windows': 'Windows', 'linux': 'Native Linux', 'wsl': 'WSL2',
                       'macos': 'macOS'}
        lines += ['## Every run', '',
                  'Same-work times, the sections above show only the latest run per '
                  'platform. Commits, machine state and everything else: inside each '
                  'run\'s directory.', '',
                  '| run | platform | corpus | mezura | scc | tokei | machine steadiness |',
                  '|---|---|---|---|---|---|---|']
        for record, rel in sorted(entries, key=lambda e: e[0]['settings']['stamp'],
                                  reverse=True):
            machine = record['machine']
            cells = [f'[{record["settings"]["stamp"]}]({rel}/)',
                     shown_names.get(machine['platform'], machine['platform']),
                     record['settings']['corpus']]
            for tool in TOOLS:
                cells.append(format_wall(mean_of(record['measurements'], 't1', tool), None))
            drift = drift_of(record['measurements'])
            cells.append(as_percent(drift) if drift else '')
            lines.append('| ' + ' | '.join(cells) + ' |')
        lines.append('')

    if newest_record is not None:
        twice = ('- Every table is measured twice, in one command order and then in the '
                 'reverse. The numbers shown average the two, and how far they disagreed is '
                 'printed in each run\'s trust checks.')
        if single_order_seen:
            twice += ' A row marked "one order only" has just the one.'
        lines += [
            '## Methodology', '',
            '- hyperfine, with no shell in between. Each section above states its own '
            'warmups, timed runs and pause.',
            '- The machine is restarted and otherwise idle. The harness\'s `noise` command '
            'verifies the background and the workload spread before anything is measured.',
            twice,
            '- A corpus definition pins a commit, and a checkout on any other commit '
            'refuses to run. A run on an unpinned tree says so beside its corpus line.',
            "- Counts come from each tool's own JSON output.",
            '- Same work: one language set for all three, generated and minified files '
            'counted by all, gitignore obeyed by all, any extra feature like keyword '
            'counting and complexity analysis turned off. '
            'The files and lines columns prove it held.',
            '- Out of the box: bare `tool <dir>`, nothing else.',
            '- The exact flags: [the harness README](../README.md).', '',
            '## Terms', '',
            '- **wall**: how long a run takes on the clock, in milliseconds: the mean of '
            'all the timed runs, both command orders together, ± their σ. That σ holds the '
            'run-to-run noise plus half the gap between the two orders.',
            "- **vs fastest**: this tool's wall divided by the fastest tool's wall in the "
            'same table.',
            '- **total cpu**: seconds of processor time, summed over every thread, user '
            'plus kernel. 16 threads busy for one second is 16 s.',
            '- **parallelism**: cpu seconds divided by wall seconds: 4.6 s of cpu inside a '
            '0.35 s run means 13 threads were busy on average.',
            '- **lines/s**: the lines this tool itself counted, divided by its wall time.',
            '- **files / lines**: what the tool reported counting. Under "Same work" the '
            'three must nearly agree. Out of the box they differ by design.',
            '- **machine steadiness**: the same binary timed at the start and at the end of '
            'the whole run. The percentage is how far apart the two means came out.', '']

    (outroot / 'README.md').write_text('\n'.join(lines), encoding='utf-8')


def write_notes(res: Path, stamp: str, machine: dict[str, Any], settings: dict[str, Any],
                record: dict[str, Any], definition: Definition) -> None:
    pin = ('pinned and verified' if settings['corpus_pinned']
           else 'not pinned, measured as it stands')
    prepared = ', '.join(settings['machine_prepared']) or 'not prepared, measured as it was'
    clean = machine['mezura_clean']
    provenance = 'clean' if clean is True else ('dirty' if clean is False else str(clean))
    drift = drift_of(record['measurements']) or 'n/a'
    exclusions = [machine.get(f'defender_process_excluded_{tool}') for tool in TOOLS]
    if PLATFORM != 'windows':
        alike = 'not applicable'
    elif settings.get('unequal_exclusions'):
        alike = (f'UNEQUAL, measured with --allow-unequal-exclusions: '
                 f'{settings["unequal_exclusions"]}')
    elif any(not isinstance(v, bool) for v in exclusions):
        alike = str(machine.get('defender_process_excluded_mezura'))
    elif all(exclusions):
        alike = 'all three excluded'
    elif not any(exclusions):
        alike = 'none excluded'
    else:
        alike = 'UNEQUAL'
    (res / 'notes.md').write_text(
        f'# Benchmark session notes {stamp}\n\n'
        f'corpus:   {settings["corpus"]} @ {str(machine["corpus_head"])[:9]}, {pin}\n'
        f'          {machine["corpus"]}\n'
        f'          {machine["corpus_fs"]}, {machine["corpus_device"]}\n'
        f'mezura:   {str(machine["mezura_head"])[:9]}, {provenance}\n'
        f'machine:  {prepared}\n'
        f'          control drift start to end: {drift}\n'
        f'MS Defender: realtime {machine["defender_realtime"]}, process exclusions: {alike}\n\n'
        f'- [ ] machine quiet during the run\n'
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


def require_locations(args: argparse.Namespace, script_dir: Path) -> None:
    if args.tools and args.corpus:
        return
    missing = ' and '.join(n for n, v in (('tools', args.tools), ('corpus', args.corpus)) if not v)
    sample_tools = 'C:/bench/tools' if PLATFORM == 'windows' else '~/bench/tools'
    sample_corpus = 'C:/bench/linux' if PLATFORM == 'windows' else '~/bench/linux'
    strays = sorted(p.name for p in script_dir.glob('*.conf') if p.name != 'benchmark.conf')
    hint = ''
    if strays and not (script_dir / 'benchmark.conf').is_file():
        hint = (f'\n\nFound {", ".join(strays)} beside this script. The file that is read is '
                f'benchmark.conf, nothing else.')
    raise SystemExit(
        f'{missing} not set, and there is no default. '
        f'Set them in one of three ways, strongest first:\n\n'
        f'  --tools <dir> --corpus <dir>                  for this invocation only\n'
        f'  MEZURA_BENCH_TOOLS / MEZURA_BENCH_CORPUS      environment\n'
        f'  {script_dir / "benchmark.conf"}\n'
        f'      copied from benchmark.conf.example and edited, e.g.\n\n'
        f'      tools  = {sample_tools}\n'
        f'      corpus = {sample_corpus}\n\n'
        f'benchmark.conf is gitignored and machine-local.{hint}')


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


class HelpFormat(argparse.HelpFormatter):
    def __init__(self, prog: str) -> None:
        super().__init__(prog, max_help_position=30)

    def format_help(self) -> str:
        text = super().format_help()
        painted = []
        for line in text.splitlines():
            if line.strip() == '<command>':
                continue
            if not COLORS_ON:
                painted.append(line)
                continue
            if line and not line.startswith(' ') and line.endswith(':'):
                line = paint('bold', line)
            else:
                flag = re.match(r'^(\s{2})(-\S[^\s].*?)(\s{2,}.*)$', line)
                alone = re.match(r'^(\s{2})(-\S.*)$', line)
                sub = re.match(r'^(\s{4})([a-z][\w-]*)(\s{2,}.*)?$', line)
                if flag:
                    line = flag.group(1) + paint('green', flag.group(2)) + flag.group(3)
                elif alone:
                    line = alone.group(1) + paint('green', alone.group(2))
                elif sub:
                    line = sub.group(1) + paint('blue', sub.group(2)) + (sub.group(3) or '')
            painted.append(line)
        return '\n'.join(painted) + '\n'


def parse_args(config: dict[str, str]) -> argparse.Namespace:
    def default_for(key: str, env_name: str, fallback: Any) -> str:
        return os.environ.get(env_name) or config.get(key) or fallback

    common = argparse.ArgumentParser(add_help=False)
    common.add_argument('--tools', type=Path,
                        default=default_for('tools', 'MEZURA_BENCH_TOOLS', None),
                        help='directory holding the mezura, scc and tokei binaries')
    common.add_argument('--corpus', type=Path,
                        default=default_for('corpus', 'MEZURA_BENCH_CORPUS', None),
                        help='the checkout that gets counted')
    common.add_argument('--corpus-def', default='linux',
                        help='name under corpora/, or a path to a corpus definition file')

    gate = argparse.ArgumentParser(add_help=False)
    gate.add_argument('--allow-unequal-exclusions', action='store_true',
                      help='proceed even when MS Defender does not treat the three tools '
                           'equally, and mark the results as possibly not representative')

    parser = argparse.ArgumentParser(description='Run the mezura benchmark suite.',
                                     epilog="benchmark.py <command> --help lists that "
                                            "command's flags.",
                                     formatter_class=HelpFormat)
    commands = parser.add_subparsers(dest='command', metavar='<command>', title='commands')

    def add_command(name: str, description: str,
                    parents: list) -> argparse.ArgumentParser:
        return commands.add_parser(name, parents=parents, help=description,
                                   description=description, formatter_class=HelpFormat)

    def add_out(target: argparse.ArgumentParser, what: str) -> None:
        target.add_argument('--out', type=Path,
                            default=Path(config['out']) if 'out' in config else None,
                            help=f'{what} (default: results/ beside this script)')

    run = add_command('run', 'measure mezura against scc and tokei on the corpus',
                      [common, gate])
    add_out(run, 'where the results directory is written')
    run.add_argument('--warmup', type=int, default=None,
                     help='untimed runs before the timed ones, per command (default 3)')
    run.add_argument('--runs', type=int, default=None,
                     help='timed runs per command (default 15)')
    run.add_argument('--settle', type=int, default=None,
                     help='seconds of pause between command series (default 3)')
    run.add_argument('--keep-raw', action='store_true',
                     help='keep the raw tool output instead of removing it once read')
    run.add_argument('--no-prep', action='store_true',
                     help='do not touch the cpu governor or power scheme, even as root')
    run.add_argument('--yes', action='store_true',
                     help='do not stop to ask when the machine could not be prepared')
    add_command('setup', 'fetch scc and tokei, clone the corpus, build mezura, and finish '
                         'with the readiness check', [common, gate])
    add_command('check', 'run each tool once and prove the numbers can be read',
                [common, gate])
    add_command('noise', 'sample the background and the workload, and say whether the '
                         'machine is steady enough to benchmark', [common])
    report = add_command('report', 'rewrite results/README.md from the recorded runs', [])
    add_out(report, 'the results directory to read and rewrite')

    args = parser.parse_args()
    if not args.command:
        parser.print_help()
        raise SystemExit(2)
    return args


def require_ready(tools: Path, corpus: Path, definition: Definition) -> Binaries:
    binaries = {name: tools / f'{name}{EXE}' for name in TOOLS}
    missing = [str(b) for b in binaries.values() if not b.exists()]
    if missing:
        raise SystemExit('these are not there, run the setup first:\n  '
                         + '\n  '.join(missing)
                         + f'\n\n  python3 {sys.argv[0]} setup')
    if not corpus.is_dir():
        raise SystemExit(f'the corpus is not there: {corpus}\n\n'
                         f'  python3 {sys.argv[0]} setup')
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
            f'  python3 {sys.argv[0]} setup\n\n'
            f'puts it right. To measure a tree as it stands, write a corpus definition for '
            f'it with no commit.')
    return binaries


def main() -> int:
    script_dir = Path(__file__).resolve().parent
    enable_colors()
    args = parse_args(read_config(script_dir / 'benchmark.conf'))

    if args.command == 'report':
        outroot = under_home(args.out) if args.out else script_dir / 'results'
        write_results_page(outroot)
        give_back_to_user(outroot)
        say(paint('green', 'wrote ') + str(outroot / 'README.md'))
        return 0

    require_locations(args, script_dir)
    definition = read_corpus_def(script_dir, args.corpus_def)
    repo = script_dir.parent
    tools = under_home(args.tools)
    corpus = under_home(args.corpus)

    for name in ('CLICOLOR_FORCE', 'MEZURA_PHASE_TIMING', 'SCC_CONFIG_PATH', 'RAYON_NUM_THREADS'):
        os.environ.pop(name, None)

    if args.command == 'setup':
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
        return do_check(corpus, require_ready(tools, corpus, definition), definition,
                        args.allow_unequal_exclusions)

    if args.command == 'check':
        return do_check(corpus, require_ready(tools, corpus, definition), definition,
                        args.allow_unequal_exclusions)

    if args.command == 'noise':
        return do_noise(corpus, require_ready(tools, corpus, definition), definition)

    plan = [] if args.no_prep else prep_plan()
    announce_prep(plan, args.yes)

    binaries = require_ready(tools, corpus, definition)
    mezura = binaries['mezura']
    scc, tokei = binaries['scc'], binaries['tokei']

    defender = defender_state(corpus, binaries)
    unequal = refuse_asymmetric_exclusions(defender, args.allow_unequal_exclusions)

    applied = []
    try:
        applied = apply_prep(plan, applied)
        return measure(args, tools, corpus, mezura, scc, tokei, applied, definition, defender,
                       unequal)
    finally:
        for step in reversed(applied):
            say(f'putting back: {step["what"]}')
            try:
                step['restore']()
            except Exception as error:
                warn(f'could not put back {step["what"]}: {error}')


def measure(args: argparse.Namespace, tools: Path, corpus: Path, mezura: Path,
            scc: Path, tokei: Path, applied: list[PrepStep], definition: Definition,
            defender: dict[str, Any], unequal: Optional[str]) -> int:
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
        return run_phases(args, tools, corpus, repo, mezura, scc, tokei, applied,
                          definition, res, outroot, target, warmup, runs, settle, stamp,
                          defender, unequal)
    finally:
        transcript.stop()
        give_back_to_user(outroot)


def run_phases(args: argparse.Namespace, tools: Path, corpus: Path, repo: Path, mezura: Path,
               scc: Path, tokei: Path, applied: list[PrepStep],
               definition: Definition, res: Path, outroot: Path, target: Path,
               warmup: int, runs: int, settle: int, stamp: str,
               defender: dict[str, Any], unequal: Optional[str]) -> int:
    header('== phase 0: machine state')
    machine = collect_machine(tools, corpus, repo)
    machine.update(defender)
    (res / 'machine.txt').write_text(
        '\n'.join(f'{k}: {v}' for k, v in machine.items()) + '\n', encoding='utf-8')
    for line in (res / 'machine.txt').read_text(encoding='utf-8').splitlines():
        say('   ' + line)

    runner = Runner(res, warmup, runs, settle)

    mezura_pinned, scc_pinned, tokei_pinned = pinned_flags(definition)
    m1 = join_cmd([mezura, target] + mezura_pinned)
    s1 = join_cmd([scc, target] + scc_pinned)
    k1 = join_cmd([tokei, target] + tokei_pinned)
    m2 = join_cmd([mezura, target])
    s2 = join_cmd([scc, target])
    k2 = join_cmd([tokei, target])

    header('== phase 0b: output and JSON captures (also the settling runs)')
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

    header('== phase 1: opening control run')
    runner.hyperfine('control-start', [join_cmd([mezura, target])])

    header('== phase 2: table 1, same work')
    runner.hyperfine('t1-fwd', [m1, s1, k1])
    runner.hyperfine('t1-rev', [k1, s1, m1])

    header('== phase 3: table 2, out of the box')
    runner.hyperfine('t2-fwd', [m2, s2, k2])
    runner.hyperfine('t2-rev', [k2, s2, m2])

    header('== phase 4: closing control run')
    runner.hyperfine('control-end', [join_cmd([mezura, target])])

    header('== summary')
    binaries = {str(p).replace('\\', '/'): name
                for p, name in ((mezura, 'mezura'), (scc, 'scc'), (tokei, 'tokei'))}
    counts = {}
    for tool in TOOLS:
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
        'unequal_exclusions': unequal,
        'hyperfine_warnings': runner.warnings,
    }
    try:
        record = build_record(res, machine, settings, counts, binaries)
        write_notes(res, stamp, machine, settings, record, definition)
        with open(res / 'run.json', 'w', encoding='utf-8') as handle:
            json.dump(record, handle, indent=1)
        write_csvs(res, record)
        write_results_page(outroot)
    except Exception as error:
        warn(f'the summary could not be written: {error}')
        say(f'         the raw output is kept in {res / "out"} and the hyperfine exports stand')
        return 1

    say(f'   drift         {drift_of(record["measurements"]) or "n/a"}')

    if not args.keep_raw:
        shutil.rmtree(res / 'out', ignore_errors=True)

    say('')
    say(f'done. everything is in {res}')
    if runner.failures:
        warn(f'hyperfine had trouble with: {", ".join(runner.failures)}')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        say('interrupted. anything that was changed on the machine has been put back.')
        sys.exit(130)
