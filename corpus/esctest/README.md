# esctest2 results

esctest2 (Thomas Dickey's fork, GPL 2) is fetched and run by `tools/esctest/run.sh`; none of it is
copied here. This folder holds only Terminus's own files:

- `known-failures.txt`: the ratchet. Every esctest2 test Terminus fails today, with a reason. The
  run fails on a new failure and on a listed test that now passes, so the list only shrinks.
- `ghostty-1.4-pass-set.txt`: the 1.0 gate, every test Ghostty 1.4 passes (generated once by
  running esctest2 against Ghostty on Linux; milestone M5).
