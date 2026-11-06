#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Measures how plug-ins keep up on each audio output: drives `suhina-cli mcp --audio`, opens a
project, and for each output plays it from a beat for a while, then reads `audio.info` and every
plug-in device's `device.plugin` counts.

Usage:
  python scripts/measure-audio.py <suhina-cli> <read-root> <project> <outputs.json> <results.json>

  <project>       the project, relative to the read root
  <outputs.json>  a list of {"name": "...", "output": {audio.setOutput parameters}}; each step
                  changes only what it gives, so a list can set the host once and then the
                  buffer size, for example:
                  [{"name": "ASIO 256", "output": {"host": "ASIO", "device": "My Interface", "bufferFrames": 256}},
                   {"name": "ASIO 512", "output": {"bufferFrames": 512}}]

Environment: MEASURE_SECONDS (default 20) how long each output plays, MEASURE_BEAT (default
144) where it plays from. Build the CLI with `--features suhina-cli/asio` to measure ASIO.

Each output's result has the underruns while it played, the blocks every plug-in ran and how
many of them were late (`late_blocks`) or got no time (`no_time`), and the longest round trip
(`longest_us`); a line per output also goes to standard error as it is measured.
"""

import json
import os
import subprocess
import sys
import time

PLAY_SECONDS = float(os.environ.get("MEASURE_SECONDS", "20"))
START_BEAT = float(os.environ.get("MEASURE_BEAT", "144"))
# How long plug-ins may take to start (a sampler reading its library may need a minute).
START_LIMIT = 240


class Server:
    """An MCP server over stdin and stdout."""

    def __init__(self, exe, root, log):
        self.proc = subprocess.Popen(
            [exe, "mcp", "--read-root", root, "--audio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.next_id = 0
        self.rpc("initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
                                "clientInfo": {"name": "measure-audio", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def rpc(self, method, params):
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise SystemExit("the server stopped; see the log beside the results")
            reply = json.loads(line)
            if reply.get("id") == self.next_id:
                return reply

    def tool(self, name, arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments}).get("result", {})
        text = "".join(c.get("text", "") for c in result.get("content", []))
        if result.get("isError"):
            raise RuntimeError(f"{name} {arguments}: {text}")
        try:
            return json.loads(text)
        except ValueError:
            return text

    def run(self, command, params=None):
        return self.tool("command_run", {"id": command, "params": params or {}})

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=60)


def plugin_devices(server):
    """(track, index, name) of every plug-in device in the project."""
    found = []
    tracks = server.run("project.tracks")
    for track in tracks if isinstance(tracks, list) else tracks.get("tracks", []):
        devices = server.run("track.devices", {"track": track["id"]})
        for index, device in enumerate(devices if isinstance(devices, list) else devices.get("devices", [])):
            if device.get("plugin"):
                found.append((track["id"], index, device.get("name")))
    return found


def statuses(server, devices):
    out = {}
    for track, index, _ in devices:
        try:
            out[(track, index)] = server.run("device.plugin", {"track": track, "device": index})
        except RuntimeError as e:
            out[(track, index)] = {"status": "error", "error": str(e)}
    return out


def wait_started(server, devices):
    start = time.time()
    while time.time() - start < START_LIMIT:
        now = statuses(server, devices)
        if not any(s.get("status") == "loading" for s in now.values()):
            return now
        time.sleep(3)
    return statuses(server, devices)


def measure(server, devices, output):
    server.run("transport.stop")
    entry = {"opened": server.run("audio.setOutput", output)}
    before = wait_started(server, devices)
    entry["plugins_running"] = sum(1 for s in before.values() if s.get("status") == "running")
    info_before = server.run("audio.info")
    server.run("transport.setPosition", {"beats": START_BEAT})
    server.run("transport.play")
    time.sleep(PLAY_SECONDS)
    info = server.run("audio.info")
    after = statuses(server, devices)
    server.run("transport.stop")
    entry["info"] = info
    entry["underruns"] = info.get("underruns", 0) - info_before.get("underruns", 0)
    names = {(t, i): n for t, i, n in devices}
    plugins = []
    for key, now in after.items():
        then = before.get(key, {})

        def grew(field, now=now, then=then):
            return (now.get(field) or 0) - (then.get(field) or 0)

        plugins.append({"device": names[key], "status": now.get("status"), "blocks": grew("blocks"),
                        "late_blocks": grew("late_blocks"), "no_time": grew("no_time"),
                        "longest_us": now.get("longest_us")})
    entry["plugins"] = plugins
    entry["blocks"] = max((p["blocks"] for p in plugins), default=0)
    entry["late_blocks"] = sum(p["late_blocks"] for p in plugins)
    entry["no_time"] = sum(p["no_time"] for p in plugins)
    entry["longest_us"] = max((p["longest_us"] or 0 for p in plugins), default=0)
    return entry


def main():
    if len(sys.argv) != 6:
        raise SystemExit(__doc__)
    exe, root, project, outputs_path, results_path = sys.argv[1:6]
    with open(outputs_path, encoding="utf-8") as f:
        outputs = json.load(f)
    with open(results_path + ".log", "w", encoding="utf-8") as log:
        server = Server(exe, root, log)
        opened = server.tool("project_open", {"path": project})
        print(f"opened on {opened.get('audio_output')}", file=sys.stderr)
        devices = plugin_devices(server)
        print(f"{len(devices)} plug-in devices", file=sys.stderr)
        results = []
        for step in outputs:
            entry = {"name": step["name"]}
            try:
                entry.update(measure(server, devices, step["output"]))
            except (RuntimeError, KeyError) as e:
                entry["error"] = str(e)
            line = {k: entry.get(k) for k in ("name", "error", "underruns", "blocks", "late_blocks", "no_time",
                                              "longest_us", "plugins_running")}
            info = entry.get("info", {})
            line.update({k: info.get(k) for k in ("buffer_frames", "engine_block", "latency_ms")})
            print(json.dumps(line), file=sys.stderr)
            results.append(entry)
            with open(results_path, "w", encoding="utf-8") as f:
                json.dump(results, f, indent=1)
        server.close()


if __name__ == "__main__":
    main()
