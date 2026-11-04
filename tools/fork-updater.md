# Fork updater

The independent Windows worker polls `horizon-rust/horizon:main` every two minutes.
The Codex heartbeat named **Horizon upstream compatibility reports** checks its
results every fifteen minutes and reports meaningful changes into the originating
chat. The worker needs this PC online; chat reporting also needs Codex available.
After downtime it discovers every still-reachable commit since its saved cursor.

## Control and evidence

The private workspace `.local/updater/config.json` is the trusted gate configuration.
The installed control scripts live in `.local/updater/control`, outside all tested
worktrees. Do not load control scripts/configuration from an upstream candidate.
`state/queue.sqlite3` records upstream, fork base, candidate, gate configuration hash,
timestamps, verdicts and private log paths. `state/service-status.json` records the
supervisor's last poll and PID. `setup-status.json` distinguishes bootstrap from an
operational installation. Keep these files, prepared resources and account data
out of Git and PRs. Windows Task Scheduler restarts the hidden supervisor after
logon and periodically if it exits. Use its named task to stop/restart the worker.

From the workspace root:

```powershell
python .local/updater/control/fork_updater.py --config .local/updater/config.json status
python .local/updater/control/fork_updater.py --config .local/updater/config.json poll
python .local/updater/control/fork_updater.py --config .local/updater/config.json retry --upstream FULL_FAILED_SHA
```

## What is tested

Each newly reachable SHA, including merged branch commits, is queued atomically
with the discovery cursor. A failed check never discards subsequent commits.
One worker merges each queued SHA into a fresh worktree from the current fork
`origin/main`; the recorded base/candidate pair is the exact tested integration.
This is a test of integration with the current fork, not a reconstruction of a
historical pristine upstream build. No main checkout is reset or overwritten.
An upstream history rewrite fails discovery without advancing the cursor.
Interrupted tests retry in a fresh directory; separate OS locks permit polling
during a test but prevent two workers testing simultaneously.

Configured gates include protocol library regressions, selector and Prism binding
regressions, original Photon shader tests, exact optimized client/control builds,
offline selector UI plus real loopback handshakes, Photon GPU/dimension acceptance,
and an actual TerraForged constructor injection/native chunk comparison against
the independently captured Java prepared-terrain corpus. `fork_terra_gate.py`
retargets copies of the mod and its helpers to the candidate SHA; it does not test
the old pin while claiming to test the candidate. Runtime binaries are frozen
after building and identified by SHA-256. Package Python checks also run.

Released `.hnmod` archives remain immutable and retain their explicit source pins.
Porting and testing current source does not make an old archive accept a different
Horizon release. A replacement native package requires its own retargeted hash
index, exact-archive native checks and release process. Never disable pin guards.
The original standalone project directories and private authentication configuration
are preserved. Automatic tests use fresh player homes and offline test identities.

These gates do not establish authenticated multiplayer, arbitrary modpacks,
all older-server gameplay, all graphics backends, or procedural TerraForged parity.
Missing resources, skipped runtime tests, timeouts and zero selected regressions
are failures, not evidence that an update works.

## Promotion and repair

`validated` means the configured gates passed; it is not permission to bypass CI.
Successful candidates are retained as
`refs/fork-updater/validated/UPSTREAM_SHA`; their worktrees are removed, and logs
and evidence remain. Failed worktrees remain available for isolated repair.
The heartbeat rechecks the current origin base, exact candidate SHA, gate config
hash and all verdicts before pushing a candidate and opening a PR on the fork.
Merge only after applicable CI checks pass. If origin/main advanced, revalidate
the new integration instead of merging a stale candidate. Never force-push main.

For a clear failure, repair in the retained isolated worktree, commit, run the
same complete gate set on the exact repaired SHA, and open a repair PR. Do not
mark a failed SQLite row successful by hand. Once the repaired main is available,
use `retry` to obtain fresh recorded validation. A worker error or substantial
backlog is actionable and should be reported once, then only on meaningful change.

The initial catch-up validates a recorded upstream snapshot; future discovery
starts there. It does not claim to have individually tested the historical
1,449-commit backlog. New commits are never intentionally coalesced or canceled.
Testing can be slower than upstream's arrival rate; the queue exposes that backlog.
Low free disk space pauses new claims without losing pending commits. Review and
archive old private runtime evidence as needed; never blindly remove worktrees or
real player data. Stopping the supervisor does not erase queue history.
