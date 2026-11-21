# Current M1 gate receipt

The current immutable receipt is [`signoff-revision-6-2026-11-22.json`](signoff-revision-6-2026-11-22.json) (draft-9, recorded 2026-11-22; SHA-256 `ebab58129748e7e0ea91356e702457390f86b8d0d1f2da4450736184ba24e977`).

[`signoff.json`](signoff.json) remains byte-for-byte historical draft-4; its QA-102/EC-010 `PASS` came from stale PR #305 evidence and is superseded. Revisions 2–5 remain immutable; revision 5 SHA-256 is `e5c816fc571ce7c233c34a8547f09e9fcad35b07086d6c198187498ace16230b`, and the pre-revision-6 `CURRENT.md` SHA-256 was `3981cb1c9354ff209368c1e492da62405bbdbdc8509ac506be39ee59b0714dac`.

Revision 6 records PR #313 pre-edit OPEN/DRAFT at `75443581d73f80ab32d5555ec1873b04031ffff8`, based on main `7552713a9c8b4515904d3e562c8b1c188a9d8676`; it records PR #341 OPEN/DRAFT at `e283576e17b87769d21982bf4b07cbc6c15b9b32` on the same base. Issue #78 and issue #211 remain OPEN.

Portable EC-010 decoder-budget execution at PR #341 head `e283576e17b87769d21982bf4b07cbc6c15b9b32` is a narrow `PASS`: rejection before full decoded-output allocation only, not before any allocation. QA-102 and M1 acceptance remain `BLOCKED`; EC-009 remains NOT RUN, and QA-004 integration, required reviewer/owner sign-off, native/manual evidence, and full scenario acceptance remain outstanding.

Current main is `7552713a9c8b4515904d3e562c8b1c188a9d8676`; the historical candidate snapshot remains `539ed2bd3493cb97ded00820f3f8bb7e9e280bea` and its 2026-11-16 observations are unchanged. Global disposition remains no-go, `m1_completion_claimed=false`, and required independent sign-offs remain pending.
