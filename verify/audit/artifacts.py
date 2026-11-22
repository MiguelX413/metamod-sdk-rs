"""Pin the binary inputs reused across Ubuntu audit runs."""

import hashlib


def inside(root, name):
    path = root / name
    if not path.is_relative_to(root) or path.resolve(strict=True) != path:
        raise ValueError(f"invalid saved artifact path: {path}")
    return path


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def pin_runtime(row, installation, previous=None):
    fresh = previous is None
    previous = previous or {}
    for name in ("engine", "metamod", "gamedll"):
        component = row[name]
        original = previous.get(name, {})
        path = inside(installation, component["binary"])
        expected = component.get("sha256", original.get("sha256"))
        if expected != original.get("sha256", expected):
            raise ValueError(f"saved {name} matrix digest differs from the capture")
        if expected is not None and digest(path) != expected:
            raise ValueError(f"saved {name} artifact changed: {path}")
        if expected is None and fresh:
            expected = digest(path)
        if expected is not None:
            component["sha256"] = expected
    engine = row["engine"]
    expected = engine.get("module_sha256", previous.get("engine", {}).get("modules", {}))
    if expected != previous.get("engine", {}).get("modules", expected):
        raise ValueError("saved engine module digests differ from the capture")
    for name, sha in expected.items():
        path = inside(installation, name)
        if digest(path) != sha:
            raise ValueError(f"saved engine module changed: {path}")
    if expected:
        engine["module_sha256"] = expected
    elif fresh:
        engine["module_sha256"] = {
            name: digest(inside(installation, name)) for name in engine.get("modules", [])
        }
