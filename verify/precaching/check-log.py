#!/usr/bin/env python3
"""Validate the two-map precaching probe transcript, independently of server exit."""
import argparse
from pathlib import Path
import re


def validate(data):
    lines = [line for line in data.splitlines() if line.startswith("PRECACHE ")]
    positive = r"([1-9][0-9]*)"
    register = "PRECACHE REGISTER PASS visit={} model=" + positive + " sound=" + positive + " duplicates=equal"
    activate = "PRECACHE ACTIVATE PASS visit={} model=" + positive + " sound=" + positive
    patterns = [register.format(1), activate.format(1), "PRECACHE READY visit=1",
                "PRECACHE STATUS PASS visit=1", "PRECACHE DEACTIVATE PASS visit=1",
                register.format(2), "PRECACHE STALE PASS old_model=" + positive + " new_model=" + positive
                + " old_sound=" + positive + " new_sound=" + positive,
                activate.format(2), "PRECACHE READY visit=2", "PRECACHE STATUS PASS visit=2",
                "PRECACHE DEACTIVATE PASS visit=2"]
    if len(lines) != len(patterns):
        raise ValueError(f"expected {len(patterns)} precaching records, found {len(lines)}")
    matches = []
    for line, pattern in zip(lines, patterns):
        match = re.fullmatch(pattern, line)
        if match is None:
            raise ValueError(f"unexpected or out-of-order precaching record: {line}")
        matches.append(match.groups())
    first, second = matches[0], matches[5]
    if first != matches[1] or second != matches[7]:
        raise ValueError("registered indices changed before activation")
    if matches[6] != (first[0], second[0], first[1], second[1]):
        raise ValueError("stale-index evidence does not match the two registrations")
    return {"visits": 2, "first": list(map(int, first)), "second": list(map(int, second))}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    args = parser.parse_args()
    result = validate(args.log.read_text(errors="replace"))
    print(f"PRECACHE transcript passed: {result}")
