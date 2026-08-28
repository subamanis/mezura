#!/usr/bin/env bash
set -uo pipefail

TOOLS="${TOOLS:-$HOME/dev/tools}"
MEZURA="$TOOLS/mezura"
CONTROL="$TOOLS/control/mezura"
SCC="$TOOLS/scc"
TOKEI="$TOOLS/tokei"
TARGET="${TARGET:-$HOME/dev/bench/linux}"
OUTROOT="${OUTROOT:-/mnt/d/dev/Rusty/mezura/archive/benchmarks}"
WARMUP=3
RUNS=15
SETTLE=3
MEZURA_PINNED='--languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region'
SCC_PINNED='-i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config'
TOKEI_PINNED='-t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"'

SMOKE=0
if [ "${1:-}" = "--smoke" ]; then
    SMOKE=1
    WARMUP=0
    RUNS=2
    SETTLE=1
    TARGET="$TARGET/scripts"
fi

unset CLICOLOR_FORCE MEZURA_PHASE_TIMING SCC_CONFIG_PATH RAYON_NUM_THREADS

if [ ! -x "$CONTROL" ]; then
    echo 'creating the control copy of mezura'
    mkdir -p "$(dirname "$CONTROL")"
    cp "$MEZURA" "$CONTROL"
fi

STAMP=$(date +%Y%m%d-%H%M%S)
KIND="${KIND_PREFIX:-results-linux}"
[ $SMOKE -eq 1 ] && KIND="$KIND-smoke"
RES="$OUTROOT/$KIND-$STAMP"
mkdir -p "$RES/out"
cd "$RES"
exec > >(tee "$RES/transcript.txt") 2>&1

hf() {
    local name="$1"; shift
    echo ">> $name"
    hyperfine -N --warmup "$WARMUP" --runs "$RUNS" --setup "sleep $SETTLE" \
        --export-json "$RES/$name.json" --export-markdown "$RES/$name.md" "$@" \
        || echo "WARNING: hyperfine reported a problem on $name"
}

run_capture() { local name="$1" cmd="$2"; eval "$cmd" > "$RES/out/$name.txt" 2>&1; }
run_json()    { local name="$1" cmd="$2"; eval "$cmd" 2>/dev/null > "$RES/out/$name.json"; }

echo '== phase 0: machine state'
{
    echo "date: $(date -Is)"
    echo "uname: $(uname -a)"
    echo "distro: $(. /etc/os-release && echo "$PRETTY_NAME")"
    grep -qi microsoft /proc/version && echo 'environment: WSL2'
    echo "cpu: $(lscpu | grep 'Model name' | sed 's/Model name: *//'), $(nproc) logical"
    echo "ram: $(free -h | awk '/Mem:/ {print $2}')"
    echo "governor: $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo none)"
    echo "target-fs: $(df -T "$TARGET" | tail -1 | awk '{print $2, $1}')"
    echo "checkout: $TARGET @ $(git -C "$TARGET" rev-parse HEAD 2>/dev/null || echo 'not a git repo') ($(if [ -n "$(git -C "$TARGET" status --porcelain 2>/dev/null)" ]; then echo dirty; else echo clean; fi))"
    echo "global-gitignore: $(git config --get core.excludesFile || echo none)"
    echo "hyperfine: $(hyperfine --version)"
    echo "mezura: $("$MEZURA" --version | tr '\n' ' ')"
    echo "scc: $("$SCC" --version)"
    echo "tokei: $("$TOKEI" --version)"
} > "$RES/machine.txt"
cat > "$RES/notes.md" <<EOF
# Linux session notes $STAMP
- [ ] corpus and tools on ext4 inside the WSL filesystem, not /mnt
- [ ] same commit as the Windows session (0ff41df1c)
- [ ] Windows side quiet during the run (shared CPU)
- [ ] mezura built from the same working tree that the Windows session measured

observations:
-
EOF

echo '== phase 0b: output and JSON captures (also the settling runs)'
run_capture t1-mezura "$MEZURA $TARGET $MEZURA_PINNED"
run_capture t1-scc    "$SCC $TARGET $SCC_PINNED"
run_capture t1-tokei  "$TOKEI $TARGET $TOKEI_PINNED"
run_capture t2-mezura "$MEZURA $TARGET"
run_capture t2-scc    "$SCC $TARGET"
run_capture t2-tokei  "$TOKEI $TARGET"
run_json t1-mezura "$MEZURA $TARGET $MEZURA_PINNED --output json"
run_json t1-scc    "$SCC $TARGET $SCC_PINNED --format json"
run_json t1-tokei  "$TOKEI $TARGET $TOKEI_PINNED --output json"
run_json t2-mezura "$MEZURA $TARGET --output json"
run_json t2-scc    "$SCC $TARGET --format json"
run_json t2-tokei  "$TOKEI $TARGET --output json"

echo '== phase 1: opening control run (gate: 1.00x inside the interval, or stop here)'
hf control-start "$MEZURA $TARGET" "$CONTROL $TARGET"

echo '== phase 2: table Γ, same work'
M1="$MEZURA $TARGET $MEZURA_PINNED"
S1="$SCC $TARGET $SCC_PINNED"
K1="$TOKEI $TARGET $TOKEI_PINNED"
hf t1-fwd "$M1" "$S1" "$K1"
[ $SMOKE -eq 0 ] && hf t1-rev "$K1" "$S1" "$M1"

echo '== phase 3: table Β, out of the box'
M2="$MEZURA $TARGET"
S2="$SCC $TARGET"
K2="$TOKEI $TARGET"
hf t2-fwd "$M2" "$S2" "$K2"
[ $SMOKE -eq 0 ] && hf t2-rev "$K2" "$S2" "$M2"

echo '== phase 4: thread sweep'
if [ $SMOKE -eq 0 ]; then
    hf sweep-scc "$S1" \
        "$S1 --file-process-job-workers 32" \
        "$S1 --file-process-job-workers 64" \
        "$S1 --file-process-job-workers 32 --directory-walker-job-workers 16" \
        "$S1 --file-process-job-workers 64 --directory-walker-job-workers 16"
    hf sweep-mezura "$M1" \
        "$M1 --threads 4 64" \
        "$M1 --threads 16 64" \
        "$M1 --threads 8 32" \
        "$M1 --threads 8 128" \
        "$M1 --threads 16 128"
else
    hf sweep-scc "$S1" "$S1 --file-process-job-workers 64"
    hf sweep-mezura "$M1" "$M1 --threads 8 128"
fi
hf sweep-tokei-default "$M1" "$K2"
RAYON_NUM_THREADS=32 hf sweep-tokei-rayon32 "$M1" "$K2"
if [ $SMOKE -eq 0 ]; then
    RAYON_NUM_THREADS=64 hf sweep-tokei-rayon64 "$M1" "$K2"
fi

echo '== phase 5: closing control run (gate: still 1.00x, or the machine drifted)'
hf control-end "$MEZURA $TARGET" "$CONTROL $TARGET"

echo '== summary'
python3 - "$RES" <<'PY'
import csv, glob, json, os, sys

res = sys.argv[1]

def totals(tool, path):
    try:
        with open(path) as f:
            d = json.load(f)
    except Exception:
        return None
    if tool == 'mezura':
        return d['total']['files'], d['total']['lines']
    if tool == 'scc':
        return sum(x['Count'] for x in d), sum(x['Lines'] for x in d)
    if tool == 'tokei':
        t = d['Total']
        files = sum(len(v.get('reports', [])) for k, v in d.items() if k != 'Total')
        return files, t['code'] + t['comments'] + t['blanks']

tot = {}
for tool in ('mezura', 'scc', 'tokei'):
    for tier in ('t1', 't2'):
        tot[(tier, tool)] = totals(tool, os.path.join(res, 'out', f'{tier}-{tool}.json'))

rows = []
for f in sorted(glob.glob(os.path.join(res, '*.json'))):
    base = os.path.basename(f)[:-5]
    with open(f) as fh:
        doc = json.load(fh)
    for r in doc.get('results', []):
        cf = cl = lps = ''
        if base.startswith(('t1-', 't2-')):
            tier = base[:2]
            cmd = r['command']
            tool = 'tokei' if '/tokei' in cmd else 'scc' if '/scc' in cmd else 'mezura'
            t = tot.get((tier, tool))
            if t and r['mean'] > 0:
                cf, cl = t
                lps = round(t[1] / r['mean'])
        rows.append([base, r['command'], f"{r['mean']:.4f}", f"{(r.get('stddev') or 0):.4f}",
                     f"{r['median']:.4f}", f"{(r.get('user') or 0):.4f}",
                     f"{(r.get('system') or 0):.4f}", len(r['times']), cf, cl, lps])

with open(os.path.join(res, 'summary.csv'), 'w', newline='') as f:
    w = csv.writer(f)
    w.writerow(['set', 'command', 'mean_s', 'stddev_s', 'median_s', 'user_s', 'system_s',
                'runs', 'counted_files', 'counted_lines', 'lines_per_sec'])
    w.writerows(rows)
PY
echo "done. everything is in $RES"
echo 'fill in notes.md, and check both control runs before believing anything else.'
