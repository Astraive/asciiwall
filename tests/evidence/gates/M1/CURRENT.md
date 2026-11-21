# Current M1 gate receipt

The current immutable receipt is [`signoff-revision-4-2026-11-22.json`](signoff-revision-4-2026-11-22.json) (draft-7, recorded 2026-11-22).

[`signoff.json`](signoff.json) remains byte-for-byte historical draft-4; its QA-102/EC-010 `PASS` came from stale PR #305 evidence and is superseded. [`signoff-revision-2-2026-11-21.json`](signoff-revision-2-2026-11-21.json) remains immutable and records `NOT RUN` for the evidence available at that snapshot. [`signoff-revision-3-2026-11-22.json`](signoff-revision-3-2026-11-22.json) also remains immutable historical draft-6; its PR #337 head `623ace74feaf70bd555398d51b4492a154fb8571` observation is superseded by the current head recorded in revision 4.

Revision 4 records PR #337 OPEN/DRAFT at current head `01e9a1f290526ca8748eac9411ece3ba84ad3d3f` (docs-only, parent `6bb7c004adc2180436053d75d7896cc136ed3907`, base main `7552713a9c8b4515904d3e562c8b1c188a9d8676`); execution at that exact head is `NOT RUN`. Actions run `44917321240` reports component tests `PASS` only at prior code/test head `6bb7c004adc2180436053d75d7896cc136ed3907`, not the current docs head. Replacement PR #341 remains OPEN/DRAFT at head `e283576e17b87769d21982bf4b07cbc6c15b9b32`; its component CI run `44940619318` passed, but this is not authenticated QA-102/EC-010 scenario acceptance. Issue #78 remains OPEN; no exact-head fixture-run receipt, QA-004 #35 receipt, independent scenario sign-off, or native/manual metadata exists, so EC-010 and M1 acceptance remain `BLOCKED`.

Current main snapshot: `7552713a9c8b4515904d3e562c8b1c188a9d8676` contains density/image/color sources but not `apps/cli`. The global decision remains `no-go`; M1 completion is not claimed and required independent sign-offs remain pending.
