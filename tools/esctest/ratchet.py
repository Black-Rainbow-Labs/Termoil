#!/usr/bin/env python3
"""Reads an esctest2 log and compares its failures with the ratchet file.

usage: ratchet.py LOG KNOWN_FAILURES BLESS SUMMARY [PASS_SET]

The ratchet file has one test per line, `TestName  # reason`, sorted. A failure that is not in it
fails the run, and so does a listed test that now passes, so the list only ever shrinks. With BLESS
set to 1 the file is rewritten from this run instead, keeping the reasons already written.

PASS_SET is the 1.0 gate (corpus/esctest/ghostty-1.4-pass-set.txt): every test Ghostty 1.4 passes.
Any of them that this run did not pass fails the run, even when blessing.
"""

import re
import sys


def parse_log(path):
    """Returns (passed, failed, skipped) sets of test names from an esctest log."""
    passed, failed, skipped = set(), set(), set()
    current = None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = re.search(r"Run test: (\S+)", line)
            if m:
                current = m.group(1)
                continue
            if current is None:
                continue
            if "Passed." in line or "Fails as expected" in line:
                passed.add(current)
                current = None
            elif re.search(r"\*\*\* TEST (\S+) FAILED", line):
                failed.add(current)
                current = None
            elif "Skipped because" in line:
                skipped.add(current)
                current = None
    return passed, failed, skipped


def read_ratchet(path):
    """Returns {name: reason} from the ratchet file."""
    known = {}
    try:
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.rstrip("\n")
                if not line.strip() or line.startswith("#"):
                    continue
                name, _, reason = line.partition("#")
                known[name.strip()] = reason.strip()
    except FileNotFoundError:
        pass
    return known


def read_pass_set(path):
    """Returns the set of test names in a pass set file (comments and blank lines skipped)."""
    with open(path, encoding="utf-8") as f:
        return {line.strip() for line in f if line.strip() and not line.startswith("#")}


def gate_lines(pass_set, passed):
    """The summary lines for the 1.0 gate, and the tests of the pass set this run did not pass."""
    missing = sorted(pass_set - passed)
    lines = [f"1.0 gate: {len(pass_set) - len(missing)} of the {len(pass_set)} tests Ghostty 1.4 passes also pass here"]
    if missing:
        lines.append("Tests Ghostty 1.4 passes that this run did not pass:")
        lines += [f"  {n}" for n in missing]
    return lines, missing


def write_ratchet(path, failed, known):
    """Writes the ratchet file for `failed`, keeping existing reasons."""
    header = "# esctest2 tests Terminus fails today, one per line with the reason. Maintained by\n# tools/esctest/run.sh: it may only shrink. Regenerate with TERMINUS_BLESS=1.\n"
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(header)
        for name in sorted(failed):
            reason = known.get(name) or "not yet investigated"
            f.write(f"{name}  # {reason}\n")


def main():
    log, ratchet, bless, summary = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4]
    pass_set = read_pass_set(sys.argv[5]) if len(sys.argv) > 5 else None
    passed, failed, skipped = parse_log(log)
    known = read_ratchet(ratchet)
    new_failures = sorted(failed - set(known))
    now_passing = sorted(set(known) & passed)
    total = len(passed) + len(failed)
    lines = [f"esctest2: {len(passed)} passed, {len(failed)} failed, {len(skipped)} skipped ({100.0 * len(passed) / max(total, 1):.1f}% of run tests pass)"]
    if new_failures:
        lines.append("New failures not in the ratchet file:")
        lines += [f"  {n}" for n in new_failures]
    if now_passing:
        lines.append("Listed in the ratchet file but passing now (remove them):")
        lines += [f"  {n}" for n in now_passing]
    gate_missing = []
    if pass_set is not None:
        more, gate_missing = gate_lines(pass_set, passed)
        lines += more
    text = "\n".join(lines)
    print(text)
    with open(summary, "w", encoding="utf-8") as f:
        f.write(text + "\n")
    if bless:
        write_ratchet(ratchet, failed, known)
        print(f"wrote {ratchet}")
        return 1 if gate_missing else 0
    return 1 if new_failures or now_passing or gate_missing else 0


if __name__ == "__main__":
    sys.exit(main())
