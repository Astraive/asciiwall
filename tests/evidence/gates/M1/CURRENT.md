# Current M1 gate receipt

The current immutable receipt is [`signoff-revision-3-2026-11-22.json`](signoff-revision-3-2026-11-22.json) (draft-6, recorded 2026-11-22).

[`signoff.json`](signoff.json) remains byte-for-byte historical draft-4; its QA-102/EC-010 `PASS` came from stale PR #305 evidence and is superseded. [`signoff-revision-2-2026-11-21.json`](signoff-revision-2-2026-11-21.json) remains immutable and correctly records `NOT RUN` for the evidence available at that snapshot.

The current receipt records passing automated decoder-budget component tests on identity-correct replacement PR #341 exact head `e283576e17b87769d21982bf4b07cbc6c15b9b32`, with the tested source revision, fixture hash, and exact CI links. Full QA-102/EC-010 and M1 acceptance remain `BLOCKED`: QA-004 #35 integration, independent scenario sign-off, native/manual observations, and the named host/session/GPU/permission tuple remain missing. The fresh Luna review was code review only, not scenario sign-off.

The global decision remains `no-go`; M1 completion is not claimed and required independent sign-offs remain pending.
