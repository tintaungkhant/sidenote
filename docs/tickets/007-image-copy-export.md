# 007 — Copy and export annotated images

Status: Todo
Dependencies: 006

## Outcome

Users can take a flattened screenshot with annotations into another application.

## Work

- Reuse the annotation renderer to compose original-resolution output.
- Add Copy Image through the native clipboard and Export PNG through a destination picker.
- Include the latest in-memory completed annotations, even if their save is pending.
- Handle clipboard/encoding/write errors and canceled destination selection.

## Acceptance

- [ ] Copied and exported images include arrows, highlights, and text matching the annotation view.
- [ ] Output retains source pixel dimensions and correct transparency/compositing.
- [ ] Copy/export does not flatten or mutate the editable source document.
- [ ] Destination cancellation leaves no partial output; overwrite follows native confirmation behavior.
- [ ] Failure leaves annotation work intact and reports an actionable error.

## Proof

Test composition commands, output dimensions, PNG decoding, clipboard payloads, and source immutability through non-browser tests. Do not compare rendered images, use visual snapshots, or inspect output visually. Record visual fidelity as unverified.

## Completion evidence

Pending.
