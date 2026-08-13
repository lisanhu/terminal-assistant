#!/usr/bin/env python3
"""Overlay key-label badges onto assets/demo.mp4 and produce assets/demo.gif.

Usage (from repo root):  python3 assets/overlay.py

Pipeline:
  1. Render transparent full-canvas PNG badges (Pillow) for each shortcut.
  2. ffmpeg: fade each badge in/out at its recorded timestamp, overlay onto
     the mp4 master -> assets/demo_overlay.mp4
  3. Two-pass palette conversion -> assets/demo.gif

Timestamps match assets/demo.tape's pacing; if you change the tape, update
EVENTS (find the new times by sampling frames, e.g. one per second).
Requires: Pillow, ffmpeg.
"""

import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
MP4 = ROOT / "assets" / "demo.mp4"
OVERLAY_MP4 = ROOT / "assets" / "demo_overlay.mp4"
GIF = ROOT / "assets" / "demo.gif"

W, H = 1600, 900  # must match `Set Width/Height` in demo.tape
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

# (label text, start time in seconds); each badge holds HOLD secs then fades.
EVENTS = [
    ("F9 g  ·  focus toggle", 12.3),
    ("F9 t  ·  layout toggle", 18.1),
    ("F9 Left  ·  divider", 23.3),
    ("F9 Right  ·  divider", 27.1),
    ("F9 s  ·  scroll mode", 31.0),
    ("F9 n  ·  hide agent panel", 39.2),
    ("F9 n  ·  show agent panel", 41.9),
    ("Shift + drag  ·  select text", 44.9),
    ("F9 v  ·  borderless view", 47.9),
    ("F9 v  ·  restore split", 51.9),
    ("F9 q  ·  quit", 54.4),
]
HOLD, FADE = 2.0, 0.8


def make_badges(out_dir: Path) -> list[Path]:
    font = ImageFont.truetype(FONT, 64)
    paths = []
    for i, (text, _) in enumerate(EVENTS):
        img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        bbox = d.textbbox((0, 0), text, font=font)
        tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
        px, py = 48, 28
        bw, bh = tw + 2 * px, th + 2 * py
        x0, y0 = (W - bw) // 2, (H - bh) // 2 - bbox[1] // 2
        d.rounded_rectangle([x0, y0, x0 + bw, y0 + bh], radius=18, fill=(0, 0, 0, 178))
        d.text((x0 + px - bbox[0], y0 + py - bbox[1]), text, font=font,
               fill=(255, 255, 255, 255))
        p = out_dir / f"{i:02d}.png"
        img.save(p)
        paths.append(p)
    return paths


def overlay(badges: list[Path]) -> None:
    duration = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            str(MP4),
        ],
        text=True,
    ).strip()
    cmd = ["ffmpeg", "-v", "error", "-y", "-i", str(MP4)]
    for p in badges:
        cmd += ["-loop", "1", "-framerate", "30", "-t", duration, "-i", str(p)]
    parts = []
    for idx, (_, s) in enumerate(EVENTS, start=1):
        parts.append(
            f"[{idx}:v]format=rgba,fade=t=in:st={s}:d=0.15:alpha=1,"
            f"fade=t=out:st={s + HOLD}:d={FADE}:alpha=1[o{idx}]"
        )
    prev = "[0:v]"
    for idx, (_, s) in enumerate(EVENTS, start=1):
        parts.append(
            f"{prev}[o{idx}]overlay=0:0:"
            f"enable='between(t\\,{s}\\,{s + HOLD + FADE})'[v{idx}]"
        )
        prev = f"[v{idx}]"
    cmd += ["-filter_complex", ";".join(parts), "-map", prev,
            "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p",
            "-crf", "18", "-shortest", str(OVERLAY_MP4)]
    subprocess.run(cmd, check=True)


def to_gif(tmp: Path) -> None:
    palette = tmp / "palette.png"
    vf = "fps=12,scale=1280:-1:flags=lanczos"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(OVERLAY_MP4),
                    "-vf", f"{vf},palettegen=max_colors=256:stats_mode=diff",
                    str(palette)], check=True)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(OVERLAY_MP4),
                    "-i", str(palette),
                    "-lavfi", f"{vf} [x]; [x][1:v] paletteuse=dither=bayer:"
                              "bayer_scale=4:diff_mode=rectangle",
                    str(GIF)], check=True)


def main() -> None:
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        overlay(make_badges(tmp))
        to_gif(tmp)
    print(f"wrote {GIF}")


if __name__ == "__main__":
    main()
