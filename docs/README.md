# Sidenote documentation

Status: planned. These documents describe the first release; features are not implemented yet.

- [Product specification](specs/product.md): behavior, scope, and acceptance conditions.
- [Technical specification](specs/technical.md): ownership, storage, invariants, and verification.
- [Tickets](tickets/README.md): ordered implementation work, one file per ticket.

The app is a personal, local-only desktop scratchpad with a VS Code style editor. Windows and macOS each keep their own data. No account, sync, or manual save workflow is required.

Implementation should follow ticket dependencies. Update each ticket's status and record verification evidence as work finishes. The specs define the release scope; ticket checklists make it reviewable.

Commenting requirement: after completing each feature, function, or cohesive set of working files, review and add appropriate code comments before moving on. Follow the [technical specification's commenting standard](specs/technical.md#code-comments).

Testing constraint: do not run browser tests or visual tests. This includes browser automation, browser-based end-to-end tests, screenshot/snapshot comparisons, and manual visual inspection. Use type checks, builds, unit tests, non-browser integration tests, and non-visual native functional checks instead. Record excluded coverage honestly; do not report it as passed.
