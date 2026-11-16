# Implementation tickets

All tickets start as **Todo**. Each numbered file is one ticket. Dependencies indicate implementation order, not permission to expand scope.

All tickets follow the testing constraint: no browser tests or visual tests, including browser automation, screenshot/snapshot comparisons, and manual visual inspection. Use type checks, builds, unit tests, non-browser integration tests, and non-visual native functional checks. Record excluded coverage as unverified.

| Ticket | Outcome | Depends on |
| --- | --- | --- |
| [001](001-editor-document-model.md) | CodeMirror document and inline-image proof | None |
| [002](002-local-storage.md) | Durable notes and validated native commands | 001 |
| [003](003-autosave-lifecycle.md) | Autosave, ordered writes, and safe exit | 002 |
| [004](004-tabs-and-session.md) | VS Code style shell, tabs, session restore | 001, 003 |
| [005](005-clipboard-images.md) | Native screenshot paste and inline images | 001, 002, 003, 004 |
| [006](006-image-annotations.md) | Editable drawing and text on images | 005 |
| [007](007-image-copy-export.md) | Flattened image copy and PNG export | 006 |
| [008](008-note-library-trash.md) | All Notes search and recoverable deletion | 004 |
| [009](009-backup-recovery.md) | Automatic backups, export, safe restore | 003, 008 |
| [010](010-cross-platform-release.md) | Accessibility, security, performance, OS proof (postponed) | 001–009 |
| [011](011-compact-editor-ui.md) | Compact editor shell, menus, and backup settings | 001–009 |

Each ticket must record checks run, results, and unresolved limitations before moving to Done. Product behavior is defined in [the product specification](../specs/product.md); storage and invariants are in [the technical specification](../specs/technical.md).

Every ticket also requires a code-comment review after each completed feature, function, or cohesive set of working files. Follow the [commenting standard](../specs/technical.md#code-comments): document function contracts, module responsibilities, and non-obvious invariants near the code. Record that review in completion evidence before marking the ticket Done.
