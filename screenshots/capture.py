#!/usr/bin/env python3
"""
Interactive and automated screenshot generator for mezura's README.md.

Usage:
    python screenshots/capture.py                      # opens the interactive capture frame
    python screenshots/capture.py --preset table-layout --dry-run
    python screenshots/capture.py --linux-dir ~/Documents/dev/bench/linux
"""

import argparse
import atexit
import ctypes
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Tuple

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

if sys.platform == "win32":
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass

ANSI_RE = re.compile(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07")
EXEC_TIME_RE = re.compile(r"(Exec time:\s*)[0-9.]+(\s*secs.*)")

REPO_ROOT = Path(__file__).resolve().parent.parent
SCREENSHOTS_DIR = REPO_ROOT / "screenshots"
OUTPUT_DIR = SCREENSHOTS_DIR / "new"
CONFIG_PATH = SCREENSHOTS_DIR / ".capture-config.json"

HERO_DARK_THEMES = [
    "Mezura",
    "Gruvbox",
    "Ocean",
    "Dracula",
    "Meadow",
]

HERO_LIGHT_THEMES = [
    "Catppuccin-light",
    "Gruvbox-light",
    "Ocean-light",
]

HERO_ALL_THEMES = HERO_DARK_THEMES + HERO_LIGHT_THEMES

ALL_BUNDLED_THEMES = [
    "Preset default",
    "Mezura",
    "Mezura-classic",
    "Catppuccin",
    "Dracula",
    "Forest",
    "Gruvbox",
    "Meadow",
    "Ocean",
    "Catppuccin-light",
    "Dracula-light",
    "Forest-light",
    "Gruvbox-light",
    "Meadow-light",
    "Ocean-light",
    "Uncolored",
]

LIGHT_THEME_PALETTES: Dict[str, Tuple[str, str]] = {
    "Catppuccin-light": ("#eff1f5", "#4c4f69"),
    "Gruvbox-light": ("#fbf1c7", "#3c3836"),
    "Ocean-light": ("#f5f7fa", "#2b303b"),
    "Dracula-light": ("#fffbeb", "#282a36"),
    "Forest-light": ("#f7f4e9", "#2d353b"),
    "Meadow-light": ("#f4f8f2", "#253328"),
}

DARK_DEFAULT_BG = "#0c0c0c" if sys.platform == "win32" else "#101010"
DARK_DEFAULT_FG = "#cccccc"


@dataclass
class Preset:
    name: str
    output: str
    default_size: Tuple[int, int]
    section: str
    args: List[str] = field(default_factory=list)
    uses_linux_repo: bool = False
    is_hero_gif: bool = False
    hero_themes: List[str] = field(default_factory=list)
    force_light_bg: Optional[str] = None
    batch_members: List[str] = field(default_factory=list)
    description: str = ""


PRESETS: Dict[str, Preset] = {
    "hero": Preset(
        name="hero",
        output="hero.gif",
        default_size=(1187, 1255),
        section="full_with_prompt",
        args=["{linux}", "--top", "10"],
        uses_linux_repo=True,
        is_hero_gif=True,
        hero_themes=HERO_ALL_THEMES,
        description="Linux kernel in 5 dark + 3 light themes -> new/hero.gif + 3 light PNGs",
    ),
    "hero-classic": Preset(
        name="hero-classic",
        output="hero-classic.gif",
        default_size=(1187, 1255),
        section="full_with_prompt",
        args=["{linux}", "--top", "10"],
        uses_linux_repo=True,
        is_hero_gif=True,
        hero_themes=["Mezura-classic", "Gruvbox", "Ocean", "Dracula", "Meadow"] + HERO_LIGHT_THEMES,
        description="Linux kernel (starting with Mezura-classic) + 3 light themes -> new/hero-classic.gif",
    ),
    "hero-dark-only": Preset(
        name="hero-dark-only",
        output="hero-dark.gif",
        default_size=(1187, 1255),
        section="full_with_prompt",
        args=["{linux}", "--top", "10"],
        uses_linux_repo=True,
        is_hero_gif=True,
        hero_themes=HERO_DARK_THEMES,
        description="Linux kernel in 5 dark themes only -> new/hero-dark.gif",
    ),
    "modules-with-unnamed": Preset(
        name="modules-with-unnamed",
        output="modules-with-unnamed.png",
        default_size=(1297, 680),
        section="details",
        args=[
            "drivers={linux}/drivers",
            "kernel={linux}/kernel",
            "{linux}/init,{linux}/ipc,{linux}/certs",
        ],
        uses_linux_repo=True,
        description="Linux modules (drivers, kernel, unnamed) -> new/modules-with-unnamed.png",
    ),
    "modules-matrix": Preset(
        name="modules-matrix",
        output="modules-matrix.png",
        default_size=(867, 537),
        section="details",
        args=[
            "drivers={linux}/drivers",
            "kernel={linux}/kernel",
            "{linux}/init,{linux}/ipc,{linux}/certs",
            "--layout",
            "matrix",
            "--top",
            "4",
        ],
        uses_linux_repo=True,
        description="Linux modules in matrix layout -> new/modules-matrix.png",
    ),
    "table-layout": Preset(
        name="table-layout",
        output="table-layout.png",
        default_size=(1211, 492),
        section="details",
        args=["--layout", "table"],
        description="mezura workspace in table layout -> new/table-layout.png",
    ),
    "boxed-layout": Preset(
        name="boxed-layout",
        output="boxed-layout.png",
        default_size=(1215, 642),
        section="details",
        args=["--layout", "boxed"],
        description="mezura workspace in boxed layout -> new/boxed-layout.png",
    ),
    "list-layout": Preset(
        name="list-layout",
        output="list-layout.png",
        default_size=(1311, 582),
        section="details",
        args=["--layout", "list"],
        description="mezura workspace in list layout -> new/list-layout.png",
    ),
    "matrix-layout": Preset(
        name="matrix-layout",
        output="matrix-layout.png",
        default_size=(685, 352),
        section="details",
        args=["--layout", "matrix"],
        description="mezura workspace in matrix layout -> new/matrix-layout.png",
    ),
    "tests-share": Preset(
        name="tests-share",
        output="tests-share.png",
        default_size=(1011, 644),
        section="details",
        args=["--tests-breakdown", "share"],
        description="mezura test breakdown (share) -> new/tests-share.png",
    ),
    "tests-split": Preset(
        name="tests-split",
        output="tests-split.png",
        default_size=(1011, 739),
        section="details",
        args=["--tests-breakdown", "split"],
        description="mezura test breakdown (split) -> new/tests-split.png",
    ),
    "compare": Preset(
        name="compare",
        output="compare.png",
        default_size=(1361, 406),
        section="history",
        args=["--compare", "2"],
        description="mezura logged history (--compare 2) -> new/compare.png",
    ),
    "diff": Preset(
        name="diff",
        output="diff.png",
        default_size=(1485, 821),
        section="details_and_keywords",
        args=["--diff", "v3.2.0"],
        description="mezura diff against v3.2.0 -> new/diff.png",
    ),
    "theme-showcase": Preset(
        name="theme-showcase",
        output="theme-showcase.png",
        default_size=(1132, 950),
        section="themes_dark",
        args=["--show-themes"],
        description="Dracula, Gruvbox, and Ocean previews -> new/theme-showcase.png",
    ),
    "theme-showcase-light": Preset(
        name="theme-showcase-light",
        output="theme-showcase-light.png",
        default_size=(1132, 950),
        section="themes_light",
        args=["--show-themes"],
        force_light_bg="Catppuccin-light",
        description="Catppuccin-light, Gruvbox-light, and Ocean-light previews -> new/theme-showcase-light.png",
    ),
    "batch: layouts": Preset(
        name="batch: layouts",
        output="4 layout PNGs",
        default_size=(1350, 700),
        section="details",
        batch_members=["table-layout", "boxed-layout", "list-layout", "matrix-layout"],
        description="Captures table, boxed, list, and matrix layouts sequentially into new/",
    ),
    "batch: tests": Preset(
        name="batch: tests",
        output="2 test PNGs",
        default_size=(1060, 780),
        section="details",
        batch_members=["tests-share", "tests-split"],
        description="Captures tests-share and tests-split sequentially into new/",
    ),
    "batch: all-mezura": Preset(
        name="batch: all-mezura",
        output="10 mezura PNGs",
        default_size=(1520, 980),
        section="details",
        batch_members=[
            "table-layout",
            "boxed-layout",
            "list-layout",
            "matrix-layout",
            "tests-share",
            "tests-split",
            "compare",
            "diff",
            "theme-showcase",
            "theme-showcase-light",
        ],
        description="Captures all 10 mezura-repo screenshots sequentially into new/",
    ),
    "batch: all-linux": Preset(
        name="batch: all-linux",
        output="hero.gif + 2 module PNGs",
        default_size=(1320, 1255),
        section="full_with_prompt",
        uses_linux_repo=True,
        batch_members=["hero", "modules-with-unnamed", "modules-matrix"],
        description="Captures hero.gif (dark + light) and both Linux module screenshots into new/",
    ),
}


def strip_ansi(text: str) -> str:
    return ANSI_RE.sub("", text)


def find_mezura_bin(explicit_bin: Optional[str] = None) -> str:
    if explicit_bin:
        return explicit_bin
    env_bin = os.environ.get("MEZURA_BIN")
    if env_bin and Path(env_bin).exists():
        return env_bin
    exe_name = "mezura.exe" if sys.platform == "win32" else "mezura"
    for profile in ("release", "debug"):
        candidate = REPO_ROOT / "target" / profile / exe_name
        if candidate.exists():
            return str(candidate)
    which_bin = shutil.which("mezura")
    if which_bin:
        return which_bin
    return str(REPO_ROOT / "target" / "release" / exe_name)


_IN_ALT_SCREEN = False


def set_terminal_palette(theme_name: Optional[str]) -> None:
    """Switches terminal background/foreground in-memory via OSC 10 / OSC 11."""
    if theme_name and theme_name in LIGHT_THEME_PALETTES:
        bg, fg = LIGHT_THEME_PALETTES[theme_name]
        sys.stdout.write(f"\x1b]11;{bg}\x07\x1b]10;{fg}\x07")
    elif theme_name and theme_name.endswith("-light"):
        sys.stdout.write("\x1b]11;#f5f5f5\x07\x1b]10;#1e1e2e\x07")
    else:
        sys.stdout.write(f"\x1b]11;{DARK_DEFAULT_BG}\x07\x1b]10;{DARK_DEFAULT_FG}\x07\x1b]110\x07\x1b]111\x07")
    sys.stdout.flush()


def restore_terminal_state() -> None:
    """Restores original screen buffer, dark terminal colors, and visible cursor."""
    global _IN_ALT_SCREEN
    try:
        seq = f"\x1b]11;{DARK_DEFAULT_BG}\x07\x1b]10;{DARK_DEFAULT_FG}\x07\x1b]110\x07\x1b]111\x07\x1b[0m\x1b[?25h"
        if _IN_ALT_SCREEN:
            seq += "\x1b[?1049l"
            _IN_ALT_SCREEN = False
        sys.stdout.write(seq)
        sys.stdout.flush()
    except Exception:
        pass


atexit.register(restore_terminal_state)


def extract_section(raw_output: str, section: str, linux_dir: str = "./linux") -> str:
    lines = raw_output.replace("\r\n", "\n").split("\n")

    if section == "full_with_prompt":
        prompt = (
            "\x1b[1;32mpetros@Petra\x1b[0m:\x1b[1;34m~/Documents/dev/bench\x1b[0m"
            "$ mezura ./linux --top 10"
        )
        cleaned = list(lines)
        while cleaned and strip_ansi(cleaned[-1]).strip() == "":
            cleaned.pop()
        return prompt + "\n" + "\n".join(cleaned)

    if section == "details":
        start_idx = None
        end_idx = len(lines)
        for i, line in enumerate(lines):
            plain = strip_ansi(line).strip()
            if start_idx is None and plain == "Details.":
                start_idx = i
            elif start_idx is not None and plain in (
                "Keywords.",
                "Overview.",
                "History.",
            ):
                end_idx = i
                break
            elif start_idx is not None and (
                plain.startswith("Test presets applied:")
                or plain.startswith("Exec time:")
            ):
                end_idx = i
                break
        if start_idx is None:
            return raw_output.strip("\n")
        selected = lines[start_idx:end_idx]
        while selected and strip_ansi(selected[-1]).strip() == "":
            selected.pop()
        return "\n".join(selected)

    if section == "details_and_keywords":
        start_idx = None
        end_idx = len(lines)
        for i, line in enumerate(lines):
            plain = strip_ansi(line).strip()
            if start_idx is None and plain == "Details.":
                start_idx = i
            elif start_idx is not None and (
                plain in ("Overview.", "History.")
                or plain.startswith("Test presets applied:")
                or plain.startswith("Exec time:")
            ):
                end_idx = i
                break
        if start_idx is None:
            return raw_output.strip("\n")
        selected = lines[start_idx:end_idx]
        while selected and strip_ansi(selected[-1]).strip() == "":
            selected.pop()
        return "\n".join(selected)

    if section == "history":
        start_idx = None
        end_idx = len(lines)
        for i, line in enumerate(lines):
            plain = strip_ansi(line).strip()
            if start_idx is None and plain == "History.":
                start_idx = i
            elif start_idx is not None and plain.startswith("Exec time:"):
                end_idx = i
                break
        if start_idx is None:
            return raw_output.strip("\n")
        selected = lines[start_idx:end_idx]
        while selected and strip_ansi(selected[-1]).strip() == "":
            selected.pop()
        return "\n".join(selected)

    if section in ("themes_dark", "themes_light"):
        wanted = (
            ("Dracula", "Gruvbox", "Ocean")
            if section == "themes_dark"
            else ("Catppuccin-light", "Gruvbox-light", "Ocean-light")
        )
        blocks: Dict[str, List[str]] = {}
        current_name: Optional[str] = None
        current_lines: List[str] = []

        for line in lines:
            plain = strip_ansi(line).strip()
            if plain in ALL_BUNDLED_THEMES:
                if current_name in wanted:
                    while current_lines and strip_ansi(current_lines[-1]).strip() == "":
                        current_lines.pop()
                    blocks[current_name] = list(current_lines)
                current_name = plain
                current_lines = [line]
            elif current_name is not None:
                if plain.startswith("Apply one with"):
                    if current_name in wanted:
                        while current_lines and strip_ansi(current_lines[-1]).strip() == "":
                            current_lines.pop()
                        blocks[current_name] = list(current_lines)
                    current_name = None
                else:
                    current_lines.append(line)

        if current_name in wanted and current_name not in blocks:
            while current_lines and strip_ansi(current_lines[-1]).strip() == "":
                current_lines.pop()
            blocks[current_name] = list(current_lines)

        out_blocks = ["\n".join(blocks[name]) for name in wanted if name in blocks]
        return "\n\n\n".join(out_blocks) if out_blocks else raw_output.strip("\n")

    return raw_output.strip("\n")


def run_mezura_for_preset(
    preset: Preset,
    mezura_bin: str,
    linux_dir: str,
    theme_override: Optional[str] = None,
) -> str:
    env = os.environ.copy()
    env["CLICOLOR_FORCE"] = "1"
    env.pop("NO_COLOR", None)
    env.pop("CLICOLOR", None)

    cmd = [mezura_bin]
    for arg in preset.args:
        cmd.append(arg.replace("{linux}", linux_dir))

    if "--show-themes" not in cmd:
        cmd.append("--no-default-config")
        if preset.uses_linux_repo:
            cmd.append("--no-local")

    effective_theme = theme_override
    if effective_theme in (None, "", "Preset default"):
        effective_theme = None

    if effective_theme and "--show-themes" not in cmd:
        cmd.extend(["--theme", effective_theme])

    proc = subprocess.run(
        cmd,
        cwd=str(REPO_ROOT),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    raw = proc.stdout
    if proc.returncode != 0 and proc.stderr.strip():
        raw = (raw.rstrip() + "\n" + proc.stderr.strip()).strip()
    elif not raw.strip() and proc.stderr.strip():
        raw = proc.stderr

    return extract_section(raw, preset.section, linux_dir=linux_dir)


def normalize_exec_time(rendered: str, reference_time_str: Optional[str]) -> Tuple[str, Optional[str]]:
    """Keeps the 'Exec time: X.XX secs' line identical across all hero.gif frames while preserving ANSI codes."""
    lines = rendered.split("\n")
    for i in range(len(lines) - 1, -1, -1):
        plain = strip_ansi(lines[i])
        m = EXEC_TIME_RE.search(plain)
        if m:
            if reference_time_str is None:
                reference_time_str = m.group(0).strip()
            else:
                lines[i] = re.sub(
                    r"Exec time:\s*[0-9.]+\s*secs[^\x1b\n]*",
                    reference_time_str,
                    lines[i],
                )
            break
    return "\n".join(lines), reference_time_str


def print_in_terminal(
    ansi_block: str,
    theme_for_bg: Optional[str] = None,
    scroll_offset: int = 0,
) -> Tuple[int, int]:
    """Switches to the alternate screen buffer, sets palette, hides cursor, and prints the block slice."""
    global _IN_ALT_SCREEN
    if not _IN_ALT_SCREEN:
        sys.stdout.write("\x1b[?1049h")
        _IN_ALT_SCREEN = True
    set_terminal_palette(theme_for_bg)
    all_lines = [" " + line if line else "" for line in ansi_block.split("\n")]
    term_rows = shutil.get_terminal_size((120, 50)).lines
    max_total_rows = max(5, term_rows - 1)
    base_top_pad = 3
    min_scroll = -10
    max_scroll = max(0, len(all_lines) - 5)
    clamped_scroll = max(min_scroll, min(scroll_offset, max_scroll))
    effective = clamped_scroll - base_top_pad
    if effective < 0:
        top_pad = -effective
        content_rows = max(1, max_total_rows - top_pad)
        visible = ([""] * top_pad) + all_lines[:content_rows]
    else:
        visible = all_lines[effective : effective + max_total_rows]
    sys.stdout.write("\x1b[2J\x1b[H\x1b[?25l" + "\n".join(visible))
    sys.stdout.flush()
    return clamped_scroll, max_scroll


def grab_screen_bbox(bbox: Tuple[int, int, int, int]):
    from PIL import Image, ImageGrab

    left, top, right, bottom = bbox
    try:
        return ImageGrab.grab(bbox=(left, top, right, bottom), all_screens=True).convert("RGB")
    except TypeError:
        return ImageGrab.grab(bbox=(left, top, right, bottom)).convert("RGB")
    except Exception:
        # Fallback for Linux environments where Pillow ImageGrab has no XCB/Wayland helper
        tmp_png = Path(tempfile.gettempdir()) / f"mezura-grab-{os.getpid()}.png"
        w = max(1, right - left)
        h = max(1, bottom - top)
        if shutil.which("grim"):
            subprocess.run(["grim", "-g", f"{left},{top} {w}x{h}", str(tmp_png)], check=True)
            img = Image.open(tmp_png).convert("RGB")
            tmp_png.unlink(missing_ok=True)
            return img
        if shutil.which("import"):
            subprocess.run(["import", "-window", "root", "-crop", f"{w}x{h}+{left}+{top}", str(tmp_png)], check=True)
            img = Image.open(tmp_png).convert("RGB")
            tmp_png.unlink(missing_ok=True)
            return img
        raise


def compute_content_bbox(img, tolerance: int = 8) -> Optional[Tuple[int, int, int, int]]:
    """Finds the bounding box of pixels differing from the corner background color."""
    from PIL import Image, ImageChops

    w, h = img.size
    if w <= 2 or h <= 2:
        return None
    # Sample bottom-right or top-left corner for the terminal background color
    bg_color = img.getpixel((w - 2, h - 2))
    bg_img = Image.new("RGB", (w, h), bg_color)
    diff = ImageChops.difference(img, bg_img).convert("L")
    diff = diff.point(lambda p: 255 if p > tolerance else 0)
    return diff.getbbox()


def auto_trim_single(img, padding: int = 14):
    bbox = compute_content_bbox(img)
    if not bbox:
        return img
    w, h = img.size
    left = max(0, bbox[0] - padding)
    top = max(0, bbox[1] - padding)
    right = min(w, bbox[2] + padding)
    bottom = min(h, bbox[3] + padding)
    return img.crop((left, top, right, bottom))


def auto_trim_frames(frames: List, padding: int = 14) -> List:
    """Crops all frames of hero.gif to the exact same union bounding box."""
    if not frames:
        return frames
    union_box: Optional[Tuple[int, int, int, int]] = None
    for frame in frames:
        bbox = compute_content_bbox(frame)
        if bbox:
            if union_box is None:
                union_box = bbox
            else:
                union_box = (
                    min(union_box[0], bbox[0]),
                    min(union_box[1], bbox[1]),
                    max(union_box[2], bbox[2]),
                    max(union_box[3], bbox[3]),
                )
    if union_box is None:
        return frames
    w, h = frames[0].size
    left = max(0, union_box[0] - padding)
    top = max(0, union_box[1] - padding)
    right = min(w, union_box[2] + padding)
    bottom = min(h, union_box[3] + padding)
    return [f.crop((left, top, right, bottom)) for f in frames]


def save_animated_gif(frames: List, out_path: Path, duration_ms: int = 1800) -> None:
    """Saves an animated GIF preserving crisp terminal colors per frame."""
    from PIL import Image

    quantized = [
        f.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
        for f in frames
    ]
    quantized[0].save(
        str(out_path),
        save_all=True,
        append_images=quantized[1:],
        duration=duration_ms,
        loop=0,
        optimize=False,
    )


def _load_full_config() -> Dict[str, dict]:
    if CONFIG_PATH.exists():
        try:
            data = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            if isinstance(data, dict):
                return data
        except Exception:
            pass
    return {}


def _save_full_config(data: Dict[str, dict]) -> None:
    try:
        SCREENSHOTS_DIR.mkdir(parents=True, exist_ok=True)
        CONFIG_PATH.write_text(json.dumps(data, indent=2), encoding="utf-8")
    except Exception:
        pass


def load_os_config() -> dict:
    full = _load_full_config()
    os_cfg = full.get(sys.platform, {})
    return os_cfg if isinstance(os_cfg, dict) else {}


def save_os_config(os_cfg: dict) -> None:
    full = _load_full_config()
    full[sys.platform] = os_cfg
    _save_full_config(full)


def load_saved_boxes() -> Dict[str, List[int]]:
    boxes = load_os_config().get("boxes", {})
    return boxes if isinstance(boxes, dict) else {}


def save_box_for_preset(preset_name: str, x: int, y: int, w: int, h: int) -> None:
    os_cfg = load_os_config()
    boxes = os_cfg.get("boxes", {})
    if not isinstance(boxes, dict):
        boxes = {}
    boxes[preset_name] = [x, y, w, h]
    os_cfg["boxes"] = boxes
    save_os_config(os_cfg)


def save_linux_dir(path_str: str) -> None:
    cleaned = path_str.strip()
    if not cleaned:
        return
    os_cfg = load_os_config()
    os_cfg["linux_dir"] = cleaned
    save_os_config(os_cfg)


def resolve_linux_dir(explicit: Optional[str] = None) -> str:
    env_or_cli = explicit or os.environ.get("MEZURA_LINUX_DIR")
    if env_or_cli:
        save_linux_dir(env_or_cli)
        return env_or_cli

    saved = load_os_config().get("linux_dir")
    if isinstance(saved, str) and saved.strip():
        p = Path(saved.strip()).expanduser()
        if p.is_dir():
            return str(p)

    home = Path.home()
    candidates = [
        REPO_ROOT.parent.parent / "bench-corpora" / "linux",
        REPO_ROOT.parent.parent / "bench" / "linux",
        REPO_ROOT.parent.parent / "linux",
        REPO_ROOT.parent / "linux",
        home / "Documents" / "dev" / "bench-corpora" / "linux",
        home / "Documents" / "dev" / "bench" / "linux",
        home / "dev" / "bench-corpora" / "linux",
        home / "dev" / "bench" / "linux",
        home / "dev" / "linux",
        home / "linux",
    ]
    if sys.platform == "win32":
        candidates.extend(
            [
                Path("D:/dev/bench-corpora/linux"),
                Path("D:/dev/bench/linux"),
                Path("D:/dev/linux"),
                Path("D:/linux"),
                Path("C:/dev/linux"),
            ]
        )
    for cand in candidates:
        try:
            if cand.is_dir():
                found = str(cand)
                save_linux_dir(found)
                return found
        except Exception:
            continue
    return ""


def preset_needs_linux(preset: Preset) -> bool:
    if preset.batch_members:
        return any(preset_needs_linux(PRESETS[m]) for m in preset.batch_members)
    return preset.uses_linux_repo or any("{linux}" in a for a in preset.args)


def run_gui(mezura_bin: str, initial_preset: str, linux_dir: str) -> int:
    import tkinter as tk
    from tkinter import filedialog, ttk

    try:
        import PIL  # noqa: F401
    except ImportError:
        sys.stderr.write("Pillow is required for screen capture. Install it with: pip install pillow\n")
        return 1

    root = tk.Tk()
    root.title("mezura screenshot capture")
    root.attributes("-topmost", True)

    transparent_key = "#010203"
    use_transparent_hole = False
    if sys.platform == "win32":
        try:
            root.wm_attributes("-transparentcolor", transparent_key)
            use_transparent_hole = True
        except Exception:
            use_transparent_hole = False
    else:
        try:
            root.wait_visibility(root)
            root.wm_attributes("-alpha", 0.55)
        except Exception:
            pass

    toolbar = tk.Frame(root, bg="#1e1e2e", padx=8, pady=4)
    toolbar.pack(side=tk.TOP, fill=tk.X)

    row1 = tk.Frame(toolbar, bg="#1e1e2e")
    row1.pack(side=tk.TOP, fill=tk.X, pady=(0, 4))

    row2 = tk.Frame(toolbar, bg="#1e1e2e")
    row2.pack(side=tk.TOP, fill=tk.X)

    tk.Label(row1, text="Preset:", fg="#cdd6f4", bg="#1e1e2e", font=("Segoe UI", 9, "bold")).pack(
        side=tk.LEFT, padx=(0, 4)
    )
    preset_var = tk.StringVar(value=initial_preset if initial_preset in PRESETS else "table-layout")
    preset_combo = ttk.Combobox(
        row1,
        textvariable=preset_var,
        values=list(PRESETS.keys()),
        state="readonly",
        width=21,
    )
    preset_combo.pack(side=tk.LEFT, padx=(0, 8))

    tk.Label(row2, text="Theme:", fg="#cdd6f4", bg="#1e1e2e", font=("Segoe UI", 9, "bold")).pack(
        side=tk.LEFT, padx=(0, 4)
    )
    theme_var = tk.StringVar(value="Preset default")
    theme_combo = ttk.Combobox(
        row2,
        textvariable=theme_var,
        values=ALL_BUNDLED_THEMES,
        state="readonly",
        width=16,
    )
    theme_combo.pack(side=tk.LEFT, padx=(0, 8))

    tk.Label(row2, text="Linux dir:", fg="#cdd6f4", bg="#1e1e2e", font=("Segoe UI", 9)).pack(
        side=tk.LEFT, padx=(0, 4)
    )
    linux_var = tk.StringVar(value=linux_dir)
    linux_entry = tk.Entry(
        row2,
        textvariable=linux_var,
        width=16,
        bg="#313244",
        fg="#cdd6f4",
        insertbackground="#cdd6f4",
    )
    linux_entry.pack(side=tk.LEFT, padx=(0, 3))

    def on_linux_entry_commit(*_args):
        val = linux_var.get().strip()
        if val:
            save_linux_dir(val)
            status_var.set(f"Saved Linux dir ({sys.platform}): {val}")
        root.focus_set()
        return "break"

    linux_entry.bind("<FocusOut>", lambda _e: save_linux_dir(linux_var.get().strip()) if linux_var.get().strip() else None)
    linux_entry.bind("<Return>", on_linux_entry_commit)

    def browse_linux_dir() -> Optional[str]:
        cur = linux_var.get().strip()
        init_dir = str(Path(cur).expanduser()) if cur and Path(cur).expanduser().is_dir() else str(REPO_ROOT.parent)
        root.attributes("-topmost", False)
        try:
            chosen = filedialog.askdirectory(
                parent=root,
                initialdir=init_dir,
                title="Select Linux kernel repository directory",
            )
        finally:
            root.attributes("-topmost", True)
            root.lift()
        if chosen:
            linux_var.set(chosen)
            save_linux_dir(chosen)
            status_var.set(f"Saved Linux dir ({sys.platform}): {chosen}")
            return chosen
        return None

    btn_browse_linux = tk.Button(
        row2,
        text="...",
        command=browse_linux_dir,
        bg="#45475a",
        fg="#cdd6f4",
        font=("Segoe UI", 8, "bold"),
        padx=5,
        pady=0,
    )
    btn_browse_linux.pack(side=tk.LEFT, padx=(0, 8))

    auto_trim_var = tk.BooleanVar(value=True)
    trim_check = tk.Checkbutton(
        row2,
        text="Auto-trim (14px)",
        variable=auto_trim_var,
        fg="#cdd6f4",
        bg="#1e1e2e",
        selectcolor="#313244",
        activebackground="#1e1e2e",
        activeforeground="#cdd6f4",
    )
    trim_check.pack(side=tk.LEFT, padx=(0, 8))

    status_var = tk.StringVar(value="Ready")
    status_bar = tk.Label(
        root,
        textvariable=status_var,
        fg="#a6e3a1",
        bg="#181825",
        anchor="w",
        padx=8,
        pady=4,
        font=("Consolas", 9),
    )
    status_bar.pack(side=tk.BOTTOM, fill=tk.X)

    hole_bg = transparent_key if use_transparent_hole else "#11111b"
    capture_hole = tk.Frame(root, bg=hole_bg)
    capture_hole.pack(side=tk.TOP, fill=tk.BOTH, expand=True)

    def current_hole_bbox() -> Tuple[int, int, int, int]:
        root.update_idletasks()
        x = capture_hole.winfo_rootx()
        y = capture_hole.winfo_rooty()
        w = max(10, capture_hole.winfo_width())
        h = max(10, capture_hole.winfo_height())
        return (x, y, x + w, y + h)

    last_dims: List[int] = [0, 0]

    scroll_state: Dict[str, object] = {
        "offset": 0,
        "max": 0,
        "rendered": None,
        "bg_theme": None,
    }

    def update_status_dimensions(event=None) -> None:
        preset = PRESETS[preset_var.get()]
        if event is not None and getattr(event, "widget", None) is capture_hole:
            w = max(10, int(event.width))
            h = max(10, int(event.height))
        else:
            w = max(10, capture_hole.winfo_width())
            h = max(10, capture_hole.winfo_height())
        if [w, h] != last_dims or event is None:
            last_dims[0], last_dims[1] = w, h
            off = int(scroll_state["offset"])
            mx = int(scroll_state["max"])
            scroll_info = f" [scroll {off}/{mx}]" if mx > 0 else ""
            status_var.set(f"[{w}x{h}]{scroll_info} {preset.description}")

    def apply_preset_geometry(preset_name: str) -> None:
        preset = PRESETS[preset_name]
        saved = load_saved_boxes().get(preset_name)
        root.update_idletasks()
        extra_w = max(0, root.winfo_width() - capture_hole.winfo_width())
        extra_h = root.winfo_height() - capture_hole.winfo_height()
        if extra_h < 30:
            extra_h = 68

        if saved and len(saved) == 4:
            x, y, w, h = saved
            root.geometry(f"{w + extra_w}x{h + extra_h}+{x}+{y}")
        else:
            w, h = preset.default_size
            cur_x = max(40, root.winfo_x())
            cur_y = max(40, root.winfo_y())
            root.geometry(f"{w + extra_w}x{h + extra_h}+{cur_x}+{cur_y}")
        root.update_idletasks()
        update_status_dimensions()

    def ensure_linux_dir_for(preset: Preset) -> Optional[str]:
        cur = linux_var.get().strip()
        if cur:
            save_linux_dir(cur)
        if not preset_needs_linux(preset):
            return cur
        if cur and Path(cur).expanduser().is_dir():
            return str(Path(cur).expanduser())
        picked = browse_linux_dir()
        if picked and Path(picked).expanduser().is_dir():
            return str(Path(picked).expanduser())
        status_var.set(f"Preset '{preset.name}' requires a valid Linux kernel directory.")
        return None

    def scroll_by(delta: int) -> None:
        rendered = scroll_state.get("rendered")
        if not isinstance(rendered, str):
            return
        bg_theme = scroll_state.get("bg_theme")
        new_off = int(scroll_state["offset"]) + delta
        cur_off, max_off = print_in_terminal(
            rendered,
            theme_for_bg=bg_theme if isinstance(bg_theme, str) else None,
            scroll_offset=new_off,
        )
        scroll_state["offset"] = cur_off
        scroll_state["max"] = max_off
        update_status_dimensions()

    def preview_current(*_args) -> None:
        preset = PRESETS[preset_var.get()]
        if preset.batch_members:
            first_member = PRESETS[preset.batch_members[0]]
            preview_preset = first_member
        else:
            preview_preset = preset

        linux_target = ensure_linux_dir_for(preview_preset)
        if linux_target is None:
            return

        chosen_theme = theme_var.get()
        if preview_preset.is_hero_gif and chosen_theme == "Preset default":
            chosen_theme = preview_preset.hero_themes[0]

        bg_theme = preview_preset.force_light_bg or chosen_theme
        status_var.set(f"Rendering preview for '{preview_preset.name}'...")
        root.update_idletasks()
        rendered = run_mezura_for_preset(
            preview_preset,
            mezura_bin=mezura_bin,
            linux_dir=linux_target,
            theme_override=chosen_theme,
        )
        cur_off, max_off = print_in_terminal(
            rendered,
            theme_for_bg=bg_theme,
            scroll_offset=int(scroll_state["offset"]),
        )
        scroll_state["rendered"] = rendered
        scroll_state["bg_theme"] = bg_theme
        scroll_state["offset"] = cur_off
        scroll_state["max"] = max_off
        update_status_dimensions()

    def capture_single_preset(preset: Preset, bbox: Tuple[int, int, int, int], linux_target: str) -> List[Path]:
        OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
        saved_paths: List[Path] = []
        cur_scroll = int(scroll_state["offset"])

        if preset.is_hero_gif:
            frames = []
            ref_exec_time: Optional[str] = None
            for idx, thm in enumerate(preset.hero_themes):
                rendered = run_mezura_for_preset(
                    preset,
                    mezura_bin=mezura_bin,
                    linux_dir=linux_target,
                    theme_override=thm,
                )
                rendered, ref_exec_time = normalize_exec_time(rendered, ref_exec_time)
                print_in_terminal(rendered, theme_for_bg=thm, scroll_offset=cur_scroll)
                time.sleep(0.22)
                frame_img = grab_screen_bbox(bbox)
                frames.append((thm, frame_img))

            raw_imgs = [img for _, img in frames]
            if auto_trim_var.get():
                trimmed_imgs = auto_trim_frames(raw_imgs, padding=14)
            else:
                trimmed_imgs = raw_imgs

            gif_path = OUTPUT_DIR / preset.output
            save_animated_gif(trimmed_imgs, gif_path, duration_ms=1800)
            saved_paths.append(gif_path)

            # Also save the light mode frames as standalone PNGs
            for (thm, _), final_img in zip(frames, trimmed_imgs):
                if thm.endswith("-light"):
                    slug = thm.lower()
                    light_png = OUTPUT_DIR / f"hero-{slug}.png"
                    final_img.save(str(light_png))
                    saved_paths.append(light_png)

            set_terminal_palette(None)
            return saved_paths

        chosen_theme = theme_var.get()
        bg_theme = preset.force_light_bg or (
            chosen_theme if chosen_theme != "Preset default" else None
        )
        rendered = run_mezura_for_preset(
            preset,
            mezura_bin=mezura_bin,
            linux_dir=linux_target,
            theme_override=chosen_theme,
        )
        cur_off, max_off = print_in_terminal(rendered, theme_for_bg=bg_theme, scroll_offset=cur_scroll)
        scroll_state["rendered"] = rendered
        scroll_state["bg_theme"] = bg_theme
        scroll_state["offset"] = cur_off
        scroll_state["max"] = max_off
        time.sleep(0.20)
        img = grab_screen_bbox(bbox)
        if auto_trim_var.get():
            img = auto_trim_single(img, padding=14)

        out_path = OUTPUT_DIR / preset.output
        img.save(str(out_path))
        saved_paths.append(out_path)
        if bg_theme and bg_theme.endswith("-light"):
            set_terminal_palette(None)
        return saved_paths

    def on_capture() -> None:
        preset_name = preset_var.get()
        preset = PRESETS[preset_name]
        linux_target = ensure_linux_dir_for(preset)
        if linux_target is None:
            return

        bbox = current_hole_bbox()
        save_box_for_preset(
            preset_name,
            root.winfo_x(),
            root.winfo_y(),
            bbox[2] - bbox[0],
            bbox[3] - bbox[1],
        )

        members = [PRESETS[m] for m in preset.batch_members] if preset.batch_members else [preset]
        root.withdraw()
        root.update()
        time.sleep(0.18)

        all_saved: List[Path] = []
        try:
            for item in members:
                all_saved.extend(capture_single_preset(item, bbox, linux_target))
        finally:
            root.deiconify()
            root.lift()
            root.attributes("-topmost", True)

        names = ", ".join(f"new/{p.name}" for p in all_saved[:4])
        if len(all_saved) > 4:
            names += f" (+{len(all_saved) - 4} more)"
        status_var.set(f"Saved: {names}")

    def on_preset_changed(*_args) -> None:
        scroll_state["offset"] = 0
        scroll_state["max"] = 0
        apply_preset_geometry(preset_var.get())

    def on_theme_changed(*_args) -> None:
        update_status_dimensions()

    def on_restore_terminal() -> None:
        restore_terminal_state()
        status_var.set("Restored terminal screen and history.")

    def close_cleanly(*_args) -> None:
        restore_terminal_state()
        try:
            root.quit()
            root.destroy()
        except Exception:
            pass

    btn_preview = tk.Button(
        row1,
        text="Preview",
        command=preview_current,
        bg="#89b4fa",
        fg="#11111b",
        font=("Segoe UI", 9, "bold"),
        padx=8,
    )
    btn_preview.pack(side=tk.LEFT, padx=(0, 6))

    btn_capture = tk.Button(
        row1,
        text="Capture",
        command=on_capture,
        bg="#a6e3a1",
        fg="#11111b",
        font=("Segoe UI", 9, "bold"),
        padx=10,
    )
    btn_capture.pack(side=tk.LEFT, padx=(0, 6))

    btn_scroll_up = tk.Button(
        row1,
        text="▲",
        command=lambda: scroll_by(-1),
        bg="#45475a",
        fg="#cdd6f4",
        font=("Segoe UI", 8, "bold"),
        padx=5,
    )
    btn_scroll_up.pack(side=tk.LEFT, padx=(0, 2))

    btn_scroll_down = tk.Button(
        row1,
        text="▼",
        command=lambda: scroll_by(1),
        bg="#45475a",
        fg="#cdd6f4",
        font=("Segoe UI", 8, "bold"),
        padx=5,
    )
    btn_scroll_down.pack(side=tk.LEFT, padx=(0, 6))

    btn_restore = tk.Button(
        row1,
        text="Restore Term",
        command=on_restore_terminal,
        bg="#f9e2af",
        fg="#11111b",
        font=("Segoe UI", 9),
        padx=6,
    )
    btn_restore.pack(side=tk.LEFT, padx=(0, 6))

    preset_combo.bind("<<ComboboxSelected>>", on_preset_changed)
    theme_combo.bind("<<ComboboxSelected>>", on_theme_changed)
    capture_hole.bind("<Configure>", update_status_dimensions)
    root.bind("<Up>", lambda e: None if e.widget in (preset_combo, theme_combo) else scroll_by(-1))
    root.bind("<Down>", lambda e: None if e.widget in (preset_combo, theme_combo) else scroll_by(1))
    root.bind("<MouseWheel>", lambda e: scroll_by(-1 if e.delta > 0 else 1))
    root.bind("<Return>", lambda e: None if e.widget is linux_entry else on_capture())
    root.bind("<Escape>", close_cleanly)
    root.protocol("WM_DELETE_WINDOW", close_cleanly)

    signal.signal(signal.SIGINT, lambda _sig, _frame: close_cleanly())

    def poll_signals() -> None:
        try:
            root.after(100, poll_signals)
        except Exception:
            pass

    root.geometry("1220x590+80+80")
    root.after(100, lambda: apply_preset_geometry(preset_var.get()))
    root.after(100, poll_signals)
    try:
        root.mainloop()
    except KeyboardInterrupt:
        pass
    finally:
        restore_terminal_state()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Automated README screenshot tool for mezura.")
    parser.add_argument(
        "--preset",
        default="table-layout",
        choices=list(PRESETS.keys()),
        help="Initial preset to select (or render in --dry-run mode).",
    )
    parser.add_argument(
        "--theme",
        default=None,
        help="Optional theme override (e.g. Mezura, Mezura-classic, Dracula, Catppuccin-light).",
    )
    parser.add_argument(
        "--linux-dir",
        default=None,
        help="Path to the Linux kernel checkout for hero and modules-* presets.",
    )
    parser.add_argument(
        "--bin",
        default=None,
        help="Path to the mezura binary (defaults to target/release/mezura or target/debug/mezura).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run the preset command, print the extracted section to stdout, and exit without opening the GUI.",
    )
    args = parser.parse_args()

    mezura_bin = find_mezura_bin(args.bin)
    if not Path(mezura_bin).exists() and not shutil.which(mezura_bin):
        sys.stderr.write(f"mezura binary not found at '{mezura_bin}'. Build with 'cargo build --release' first.\n")
        return 1

    linux_dir = resolve_linux_dir(args.linux_dir)

    if args.dry_run:
        preset = PRESETS[args.preset]
        if preset_needs_linux(preset) and (not linux_dir or not Path(linux_dir).expanduser().is_dir()):
            sys.stderr.write(
                f"Preset '{preset.name}' requires a Linux kernel repository. Pass --linux-dir <path>.\n"
            )
            return 1
        members = [PRESETS[m] for m in preset.batch_members] if preset.batch_members else [preset]
        for item in members:
            if item.is_hero_gif:
                ref_time = None
                for thm in item.hero_themes:
                    out = run_mezura_for_preset(item, mezura_bin, linux_dir, theme_override=thm)
                    out, ref_time = normalize_exec_time(out, ref_time)
                    sys.stdout.write(f"=== {item.name} [{thm}] ===\n{out}\n\n")
            else:
                out = run_mezura_for_preset(item, mezura_bin, linux_dir, theme_override=args.theme)
                sys.stdout.write(f"=== {item.name} ===\n{out}\n\n")
        return 0

    return run_gui(mezura_bin=mezura_bin, initial_preset=args.preset, linux_dir=linux_dir)


if __name__ == "__main__":
    sys.exit(main())
