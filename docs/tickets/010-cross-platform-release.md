# 010 — Verify the first desktop release

Status: Postponed at user request (2026-11-16). UI enhancement proceeds separately in ticket 011.
Dependencies: 001–009

## Outcome

The complete offline workflow is proven on Windows and macOS, with recorded resource usage and remaining limits.

## Work

- Replace starter branding and remove unused assets, commands, dependencies, and capabilities.
- Set a restrictive CSP and verify local images/clipboard work without broad filesystem or network permissions.
- Check accessible control metadata, focus-state logic, annotation keyboard handlers, IME event handling, and native shortcut conflicts without browser or visual tests. Record screen-reader and rendered-focus coverage as unverified where it cannot be checked this way.
- Measure release cold startup, idle CPU/memory, and image-heavy tab cycling on both OSes against the starter baseline.
- Fix unbounded resource retention; document measured results rather than claim an untested memory budget.
- Build native distribution artifacts on each target OS. Document platform trust/signing limitations honestly; paid signing/notarization is a separate release decision.
- Add user-facing instructions for storage location, backup/restore, shortcuts, and forced-termination guarantees.

## Acceptance

- [ ] Every release acceptance flow has non-browser, non-visual evidence on both OSes; appearance and rendered alignment are explicitly recorded as unverified.
- [ ] No account, network, sync, or manual note save is needed.
- [ ] Normal close/quit, forced kill, disk-write failure, and backup restore have recorded results.
- [ ] Many inactive tabs do not retain one editor/canvas each; image cycling does not show unbounded memory growth.
- [ ] No starter null CSP or unnecessary opener permission remains.
- [ ] Unsupported or untested behavior is listed explicitly; a missing platform run is not reported as a pass.

## Proof

Run `bun run check`, `bun run build`, Rust formatting/checks/tests, and native release builds. Do not run browser tests or visual tests. Record OS versions, hardware, datasets, measurements, test results, excluded coverage, and artifact paths.

## Completion evidence

Pending.
