# GUI source and asset baseline

- Date: 2026-09-24
- Product line: Chaos Web/Desktop
- Source decision: clean-room implementation
- Review scope: `apps/chaos-ui`, `crates/codegen/chaos-engine`,
  `crates/codegen/xai-grok-web`, and `crates/codegen/xai-grok-desktop`

## Source and license decision

The initial Chaos GUI implementation does not copy source code, UI assets, fonts,
icons, illustrations, Office preview assets, or test identifiers from another
product. The React walking skeleton and Rust host crates were written in this
repository and use only Apache-2.0 project dependencies already tracked by the
repository's third-party notice process. New files use the repository's Apache-2.0
license convention; the source revision is recorded by Git history.

The clean-room decision is the approved fallback where provenance for a reference
UI or its assets is not established. No reference repository, floating revision,
or third-party asset is a source dependency of the implementation.

## Asset and brand inventory

| Class | Current GUI use | Decision |
|---|---|---|
| Fonts | System font stack only | Use / no copied asset |
| Icons | No icon package or copied icons | Deferred / replace with owned assets |
| Illustrations and images | None | Deferred / replace with owned assets |
| Office previews | None | Unsupported in M0; later replacement design |
| Trademarks and product artwork | No copied artwork; `Chaos` name is repository product branding | Owned product use; no reference artwork |

Future assets must add a source URL, fixed revision, license, copyright notice,
third-party notice entry, and replacement decision here before being committed.

## Scope boundary

The first stable GUI scope is a local developer workbench: one local user,
loopback Web binding, desktop host boundary, text sessions, streaming events,
cancel, and resume. Public Web hosting, multi-user authentication, cloud sync,
remote PTY, embedded browser, voice, and CUA remain deferred until their own
security and product decisions are approved.
