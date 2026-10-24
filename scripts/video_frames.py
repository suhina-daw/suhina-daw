#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later
"""Look at a screen recording of a bug without watching it frame by frame (docs/debugging.md).

    python scripts/video_frames.py extract VIDEO OUT_DIR [--fps 6] [--from S] [--to S]
    python scripts/video_frames.py sheet FRAMES_DIR SHEET.jpg [--every N] [--cols C]
           [--size WxH] [--crop X0,Y0,X1,Y1] [--first A] [--last B] [--fps F] [--from S]
    python scripts/video_frames.py track FRAMES_DIR --strip X0,X1,Y0,Y1 [--fps F] [--from S]
           [--scale PX_PER_UNIT,X_AT,UNIT_AT] [--pointer Y0,Y1]

extract  pulls frames out of a video with ffmpeg (on the PATH) as f0001.jpg, f0002.jpg, ...
sheet    lays frames out as one contact sheet, each labelled with its number and second, so a
         run of a second or two can be read in one image; --crop takes the same part of every
         frame (the clip, the plug-in's timeline) at full size.
track    follows thin vertical lines (a playhead, an insert marker) in a horizontal strip of
         every frame and prints, at each change, where the lines are; with --scale in bars (or
         beats), with --pointer where the mouse pointer is. A playhead that jumps back for a
         frame shows as a line that comes and goes. Needs Pillow and numpy.
"""
import argparse
import pathlib
import subprocess
import sys

from PIL import Image, ImageDraw


def numbers(text, count):
    values = [float(v) for v in text.split(",")]
    if len(values) != count:
        raise SystemExit(f"expected {count} numbers, got {text!r}")
    return values


def frames_in(folder):
    return sorted(pathlib.Path(folder).glob("f*.jpg"))


def extract(args):
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    command = ["ffmpeg", "-v", "error"]
    if args.start is not None:
        command += ["-ss", str(args.start)]
    if args.end is not None:
        command += ["-to", str(args.end)]
    command += ["-i", args.video, "-vf", f"fps={args.fps}", "-q:v", "3", str(out / "f%04d.jpg")]
    subprocess.run(command, check=True)
    print(f"{len(frames_in(out))} frames in {out}")


def sheet(args):
    files = frames_in(args.frames)
    first = args.first if args.first is not None else 1
    last = args.last if args.last is not None else len(files)
    chosen = [f for i, f in enumerate(files, 1) if first <= i <= last and (i - first) % args.every == 0]
    if not chosen:
        raise SystemExit("no frames in that range")
    width, height = (int(v) for v in args.size.split("x"))
    box = tuple(int(v) for v in numbers(args.crop, 4)) if args.crop else None
    rows = (len(chosen) + args.cols - 1) // args.cols
    image = Image.new("RGB", (args.cols * width, rows * height), "black")
    draw = ImageDraw.Draw(image)
    for i, path in enumerate(chosen):
        frame = Image.open(path).convert("RGB")
        if box:
            frame = frame.crop(box)
        frame = frame.resize((width, height))
        x, y = (i % args.cols) * width, (i // args.cols) * height
        image.paste(frame, (x, y))
        number = int(path.stem[1:])
        label = f"{number} {args.start + (number - 1) / args.fps:.1f}s"
        draw.rectangle([x, y, x + 8 + 6 * len(label), y + 13], fill="black")
        draw.text((x + 3, y + 1), label, fill="yellow")
    image.save(args.sheet, quality=88)
    print(f"{args.sheet}: {len(chosen)} frames, {image.size[0]}x{image.size[1]}")


def track(args):
    import numpy as np

    x0, x1, y0, y1 = (int(v) for v in numbers(args.strip, 4))
    files = frames_in(args.frames)
    if not files:
        raise SystemExit("no frames")
    strips, pointers = [], []
    for path in files:
        grey = np.asarray(Image.open(path).convert("L"), dtype=np.int16)
        strips.append(grey[y0:y1, x0:x1].mean(axis=0))
        if args.pointer:
            p0, p1 = (int(v) for v in numbers(args.pointer, 2))
            pointers.append(grey[p0:p1, x0:x1])
    strips = np.array(strips)
    background = np.median(strips, axis=0)
    pointer_background = np.median(np.array(pointers), axis=0) if pointers else None
    if args.scale:
        per_unit, x_at, unit_at = numbers(args.scale, 3)
        place = lambda x: round(unit_at + (x - x_at) / per_unit, 2)
    else:
        place = lambda x: int(x)
    last = None
    for i, strip in enumerate(strips):
        # Lines darker than the frames' usual strip: what moves (grid lines stay in the median).
        columns = np.where(background - strip > args.contrast)[0]
        groups = []
        for c in columns:
            if groups and c - groups[-1][-1] <= 2:
                groups[-1].append(c)
            else:
                groups.append([c])
        lines = [place(x0 + float(np.mean(g))) for g in groups]
        pointer = None
        if pointer_background is not None:
            moved = np.where(np.abs(pointers[i] - pointer_background).max(axis=0) > 60)[0]
            pointer = place(x0 + float(np.median(moved))) if len(moved) else None
        state = (tuple(lines), pointer)
        if state != last:
            print(f"{args.start + i / args.fps:7.2f}s frame {i + 1:4d}  lines={lines}  pointer={pointer}")
            last = state


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    e = sub.add_parser("extract")
    e.add_argument("video")
    e.add_argument("out")
    e.add_argument("--fps", type=float, default=6)
    e.add_argument("--from", dest="start", type=float)
    e.add_argument("--to", dest="end", type=float)
    s = sub.add_parser("sheet")
    s.add_argument("frames")
    s.add_argument("sheet")
    s.add_argument("--every", type=int, default=1)
    s.add_argument("--cols", type=int, default=4)
    s.add_argument("--size", default="480x270")
    s.add_argument("--crop")
    s.add_argument("--first", type=int)
    s.add_argument("--last", type=int)
    s.add_argument("--fps", type=float, default=6, help="the rate the frames were extracted at, for the labels")
    s.add_argument("--from", dest="start", type=float, default=0.0, help="the --from they were extracted with")
    t = sub.add_parser("track")
    t.add_argument("frames")
    t.add_argument("--strip", required=True)
    t.add_argument("--fps", type=float, default=24)
    t.add_argument("--from", dest="start", type=float, default=0.0, help="the --from they were extracted with")
    t.add_argument("--scale")
    t.add_argument("--pointer")
    t.add_argument("--contrast", type=float, default=25)
    args = parser.parse_args()
    {"extract": extract, "sheet": sheet, "track": track}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
