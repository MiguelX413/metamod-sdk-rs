#!/usr/bin/env python3
"""Exercise prebuilt relationship fixtures on isolated saved server copies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import time


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def validate_transcript(data):
    if b"RELATIONSHIP FAIL" in data:
        raise RuntimeError("fixture reported a failure")
    lines = data.splitlines()
    for phase, expected in (("LIVE", 2), ("RETIRED", 2), ("REUSED", 2), ("MAP", 1)):
        actual = lines.count(f"RELATIONSHIP {phase} PASS".encode())
        if actual != expected:
            raise RuntimeError(f"expected {expected} {phase} results, found {actual}")


def run(base, plugin, fixture, output, engines=None, metamod="classic"):
    engines = tuple(dict.fromkeys(engines or ("linux-hlds", "linux-rehlds")))
    if metamod == "metamod-r" and engines != ("linux-rehlds",):
        raise ValueError("Metamod-R requires --engine linux-rehlds for this fixture")
    if output == base or base in output.parents:
        raise ValueError("output must be outside source installations")
    output.mkdir()
    for name in engines:
        source = base / name
        if not source.is_dir() or source.is_symlink():
            raise ValueError(f"source must be an installation directory: {source}")
        for root, dirs, files in os.walk(source, followlinks=False):
            for entry in dirs + files:
                path = Path(root) / entry
                mode = path.lstat().st_mode
                if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                    raise ValueError(f"source contains a link or special file: {path}")
        stage = output / name
        shutil.copytree(source, stage)
        addon = stage / "ricochet/addons/mm_relationships"
        addon.mkdir(parents=True)
        shutil.copy2(plugin, addon / "probe.so")
        shutil.copy2(fixture, addon / "fixture.so")
        (addon / "plugins.ini").write_text(
            "linux addons/mm_relationships/fixture.so\nlinux addons/mm_relationships/probe.so\n")
        (addon / "config.ini").write_text(
            "gamedll dlls/ricochet.so\nplugins_file addons/mm_relationships/plugins.ini\n")
        manager_path = f"addons/{metamod}/dlls/metamod_i386.so"
        manager = stage / "ricochet" / manager_path
        if metamod == "classic":
            shutil.copy2(manager, manager.with_name("metamod.so"))
            manager_path = "addons/classic/dlls/metamod.so"
        liblist = stage / "ricochet/liblist.gam"
        contents, replacements = re.subn(r'^gamedll_linux[ \t]+.*$',
                                         f'gamedll_linux "{manager_path}"',
                                         liblist.read_text(), flags=re.M)
        if replacements != 1:
            raise ValueError(f"expected one gamedll_linux entry in {liblist}")
        liblist.write_text(contents)
        arguments = ["./hlds_linux", "-console", "-insecure", "-nomaster", "-game", "ricochet",
                     "-ip", "127.0.0.1", "-port", "27057", "+ip", "127.0.0.1", "+sv_lan", "1",
                     "+maxplayers", "2", "+deathmatch", "1",
                     "+localinfo", "mm_configfile", "addons/mm_relationships/config.ini",
                     "+localinfo", "mm_pluginsfile", "addons/mm_relationships/plugins.ini",
                     "+localinfo", "mm_gamedll", "dlls/ricochet.so", "+map", "rc_arena"]
        log = output / f"{name}.log"
        identity = {"argv": arguments, "environment": {"LD_LIBRARY_PATH": str(stage)},
                    "sha256": {str(p.relative_to(stage)): digest(p) for p in
                               [stage / "hlds_linux", stage / "engine_i486.so", manager,
                                stage / "ricochet/dlls/ricochet.so", addon / "probe.so", addon / "fixture.so"]}}
        (output / f"{name}.launch.json").write_text(json.dumps(identity, indent=2) + "\n")
        with log.open("wb") as stream:
            process = subprocess.Popen(arguments, cwd=stage, stdin=subprocess.PIPE,
                                       stdout=stream, stderr=subprocess.STDOUT,
                                       env={**os.environ, "LD_LIBRARY_PATH": str(stage)})

            def send(command):
                offset = log.stat().st_size
                process.stdin.write((command + "\n").encode())
                process.stdin.flush()
                return offset

            def wait(marker, offset=0):
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    data = log.read_bytes()[offset:]
                    if b"RELATIONSHIP FAIL" in data:
                        raise RuntimeError(f"fixture failed; see {log}")
                    if marker.encode() in data:
                        return
                    if process.poll() is not None:
                        raise RuntimeError(f"server exited {process.returncode}; see {log}")
                    time.sleep(.1)
                raise TimeoutError(f"missing {marker}; see {log}")

            try:
                wait("RELATIONSHIP READY")
                send("version; meta version; meta list; meta game")
                for visit in range(2):
                    for command, marker in [("prepare", "LIVE"), ("retire", "RETIRED"), ("reuse", "REUSED")]:
                        if command == "reuse":
                            time.sleep(.8)
                        offset = send(f"sdk_relationship_{command}")
                        wait(f"RELATIONSHIP {marker} PASS", offset)
                    if visit == 0:
                        offset = send("changelevel rc_deathmatch")
                        wait("RELATIONSHIP MAP PASS", offset)
                        wait("RELATIONSHIP READY", offset)
                send("status; quit")
                status = process.wait(timeout=15)
                if status != 0:
                    raise RuntimeError(f"server exited {status}; see {log}")
                validate_transcript(log.read_bytes())
            finally:
                if process.poll() is None:
                    try:
                        send("quit")
                        process.wait(timeout=10)
                    except (BrokenPipeError, subprocess.TimeoutExpired):
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                try:
                    process.stdin.close()
                except BrokenPipeError:
                    pass
                (output / f"{name}.exit").write_text(f"exit_status={process.returncode}\n")
        print(f"{name}: relationship sequence passed, exit zero", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("saved_installs", type=Path)
    parser.add_argument("plugin", type=Path)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--engine", action="append", choices=("linux-hlds", "linux-rehlds"),
                        help="select a saved engine; defaults to both")
    parser.add_argument("--metamod", choices=("classic", "metamod-r"), default="classic")
    args = parser.parse_args()
    run(*(path.resolve() for path in (args.saved_installs, args.plugin, args.fixture, args.output)),
        engines=args.engine, metamod=args.metamod)
