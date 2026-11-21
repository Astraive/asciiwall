# Current M1 gate receipt

The current immutable receipt is [`signoff-revision-5-2026-11-22.json`](signoff-revision-5-2026-11-22.json) (draft-8, recorded 2026-11-22).

[`signoff.json`](signoff.json) remains byte-for-byte historical draft-4; its QA-102/EC-010 `PASS` came from stale PR #305 evidence and is superseded. Revisions 2, 3, and 4 remain immutable historical snapshots; revision 4's PR #337 OPEN/DRAFT status is superseded by the closure recorded in revision 5.

Revision 5 records PR #337 CLOSED without merge at exact head `01e9a1f290526ca8748eac9411ece3ba84ad3d3f` (parent `6bb7c004adc2180436053d75d7896cc136ed3907`, based on main `7552713a9c8b4515904d3e562c8b1c188a9d8676`). Component run `44917321240` passed only at the parent, not the closed docs-only head. Preflight confirmed active replacement PR #341 OPEN/DRAFT at exact head `e283576e17b87769d21982bf4b07cbc6c15b9b32`, based on main `7552713a9c8b4515904d3e562c8b1c188a9d8676`; issue #78 remains OPEN.

EC-010 execution remains `NOT RUN` and acceptance `BLOCKED` until an execution receipt exists for the current fixture/scenario. Component CI run `44940619318` is not authenticated scenario acceptance.

Current main is `7552713a9c8b4515904d3e562c8b1c188a9d8676`; the historical candidate snapshot remains `539ed2bd3493cb97ded00820f3f8bb7e9e280bea` and its 2026-11-16 observations are unchanged. The global decision remains `no-go`; M1 completion is not claimed and required independent sign-offs remain pending.
