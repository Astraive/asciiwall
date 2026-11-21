# Current M1 gate receipt

The current immutable receipt is [`signoff-revision-3-2026-11-22.json`](signoff-revision-3-2026-11-22.json) (draft-6, recorded 2026-11-22).

[`signoff.json`](signoff.json) remains byte-for-byte historical draft-4; its QA-102/EC-010 `PASS` came from stale PR #305 evidence and is superseded. [`signoff-revision-2-2026-11-21.json`](signoff-revision-2-2026-11-21.json) remains immutable and correctly records `NOT RUN` for the evidence available at that snapshot.

The current receipt records `PASS` only for automated decoder-budget component tests on replacement PR #341 exact head `e283576e17b87769d21982bf4b07cbc6c15b9b32`; overall QA-102/EC-010 scenario acceptance is `BLOCKED` because no current exact-head decoder fixture-run or QA-004 #35 scenario receipt exists. The stale PR #305 `PASS` is superseded historical evidence. PR #337 head `623ace74feaf70bd555398d51b4492a154fb8571` was unexecuted branch evidence whose earlier evidence predates that head, not a passing M1 result. The recorded PR #341 component evidence does not constitute independent scenario sign-off, native/manual observations, or the named host/session/GPU/permission tuple. The fresh Luna review was code review only, not scenario sign-off.

Current main snapshot: `7552713a9c8b4515904d3e562c8b1c188a9d8676` contains density/image/color sources but not `apps/cli`. The global decision remains `no-go`; M1 completion is not claimed and required independent sign-offs remain pending.
