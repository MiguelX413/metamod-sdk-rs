"""Materialize a server snapshot without dereferencing arbitrary source links."""

import argparse
from collections import deque
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import stat
import subprocess

from elf import ELF32_HEADER_SIZE, validate_shared_object

SDL_NAME = "libSDL2.so"
SDL_ABI = "libSDL2-2.0.so.0"
SDL_SYSTEM = f"/usr/lib/i386-linux-gnu/{SDL_ABI}"


def resolve_link(root, path):
    """Resolve one component at a time so even intermediate escapes fail."""
    pending = deque(path.relative_to(root).parts)
    resolved = root
    hops = 0
    while pending:
        part = pending.popleft()
        if not resolved.is_dir():
            raise NotADirectoryError(f"non-directory symlink component: {resolved}")
        if part == "..":
            if resolved == root:
                raise ValueError("symlink escapes the installation")
            resolved = resolved.parent
        else:
            entry = resolved / part
            if stat.S_ISLNK(entry.lstat().st_mode):
                hops += 1
                if hops > 40:
                    raise ValueError("symlink cycle or more than 40 link hops")
                raw_target = os.readlink(entry)
                target = Path(raw_target)
                if raw_target.endswith("/"):
                    pending.appendleft(".")
                if target.is_absolute():
                    try:
                        target = target.relative_to(root)
                    except ValueError:
                        raise ValueError("symlink escapes the installation") from None
                    resolved = root
                pending.extendleft(reversed(target.parts))
            else:
                resolved = entry
    return resolved


def snapshot_plan(root, replacement):
    def walk(directory, relative, ancestors):
        for entry in sorted(directory.iterdir()):
            destination = relative / entry.name
            target = entry
            if entry.is_symlink():
                link = os.readlink(entry)
                try:
                    if entry == root / SDL_NAME and link == SDL_SYSTEM:
                        target = replacement
                    else:
                        target = resolve_link(root, entry)
                except FileNotFoundError as error:
                    if entry == root / SDL_NAME and link in {SDL_ABI, f"./{SDL_ABI}"}:
                        target = replacement
                    else:
                        raise ValueError(f"Unresolved symlink: {entry} -> {link}") from error
                except (OSError, ValueError) as error:
                    raise ValueError(f"{entry} -> {link}: {error}") from error
                if target == replacement:
                    if not target.is_file():
                        raise ValueError(f"{entry} -> {link}. Install libsdl2-2.0-0:i386 or restore the original link target.")
                    with target.open("rb") as stream:
                        validate_shared_object(stream.read(ELF32_HEADER_SIZE), target)
                    dynamic = subprocess.check_output(["readelf", "-d", "--", str(target)], text=True,
                                                      env={**os.environ, "LC_ALL": "C"})
                    if not any("(SONAME)" in line and line.rstrip().endswith(f"[{SDL_ABI}]")
                               for line in dynamic.splitlines()):
                        raise ValueError(f"{entry} -> {link}. Replacement lacks the SDL2 ABI SONAME: {target}")
                    yield destination, target, "sdl"
                    continue
            mode = target.lstat().st_mode
            if stat.S_ISDIR(mode):
                if target in ancestors:
                    raise ValueError(f"directory symlink cycle at {entry} -> {target}")
                yield destination, target, "directory"
                yield from walk(target, destination, ancestors | {target})
            elif stat.S_ISREG(mode):
                yield destination, target, "file"
            else:
                raise ValueError(f"unexpected special file: {entry}")
    return list(walk(root, Path(), {root}))


@contextmanager
def open_source(root_fd, parts):
    # Every ancestor is opened relative to the pinned source, refusing symlinks
    # even when a path has changed since planning.
    directory = os.dup(root_fd)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = child
        fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(fd, "rb") as stream:
            if not stat.S_ISREG(os.fstat(fd).st_mode):
                raise ValueError("source file changed to a special file during staging")
            yield stream
    finally:
        os.close(directory)


def stage(source, destination, replacement):
    root = source.resolve(strict=True)
    destination = destination.resolve()
    if destination.is_relative_to(root):
        raise ValueError("staging destination must be outside the source installation")
    fd = os.open(root, os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        plan = snapshot_plan(root, replacement)
        destination.mkdir()
        for relative, target, kind in plan:
            output = destination / relative
            if kind == "directory":
                output.mkdir()
            elif kind == "sdl":
                shutil.copyfile(target, output)
                output.chmod(0o755)
            else:
                with open_source(fd, target.relative_to(root).parts) as stream, output.open("xb") as copy:
                    shutil.copyfileobj(stream, copy)
                    info = os.fstat(stream.fileno())
                    output.chmod(stat.S_IMODE(info.st_mode) & 0o777)
                    os.utime(output, ns=(info.st_atime_ns, info.st_mtime_ns))
    finally:
        os.close(fd)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("sdl", type=Path)
    args = parser.parse_args()
    try:
        stage(args.source, args.destination, args.sdl)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Staging failed: {error}\n")
