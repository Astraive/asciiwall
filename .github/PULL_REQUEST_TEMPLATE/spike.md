<!--
Disposable research/risk spike PR template.
PR title format: type(scope): subject [TICKET-ID], for example "chore(platform): record Windows WorkerW spike [WND-001]".
No Co-authored-by trailers are permitted anywhere in commit messages or squash-merge descriptions.
-->

**Ticket:** <!-- Stable ticket ID (e.g. WND-001) and `Refs #N` / `Closes #N` once published. -->

## Hypothesis and question

<!-- The single bounded question from docs/delivery/risks-and-spikes.md this spike evaluates. -->

## Environment tuple

- OS build / kernel:
- Session type / desktop / compositor:
- GPU / driver / graphics backend:
- Toolchain / dependency versions:

## Observed outcome and fallback decision

- **Result:** <!-- Pass / Fail / Inconclusive -->
- **Observed behavior:**
- **Fallback or architectural consequence:** <!-- Link ADR or doc update if a contract changes. -->

## Test evidence

<!-- Exact commands, fixtures, and observed measurements (or why manual native steps were needed). -->

## Checklist

- [ ] Spike remains explicitly scoped as an experiment; no untested platform support tier is promoted.
- [ ] No personal media, proprietary fonts, secrets, or unredacted private paths committed.
- [ ] Relevant risk register or ADR docs updated if findings change contracts.
- [ ] No `Co-authored-by` trailers in commits or PR description.
