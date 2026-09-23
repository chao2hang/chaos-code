# GUI source and asset baseline

- Date: 2026-09-24
- Product line: Chaos Web/Desktop
- Source decision: clean-room implementation

The initial Chaos GUI implementation does not copy source code, UI assets, fonts,
icons, illustrations, or test identifiers from another product. The React
walking skeleton and Rust host crates were written in this repository and use
only Apache-2.0 project dependencies already tracked by the repository's
third-party notice process.

The first stable GUI scope is a local developer workbench: one local user,
loopback Web binding, desktop host boundary, text sessions, streaming events,
cancel, and resume. Public Web hosting, multi-user authentication, cloud sync,
remote PTY, embedded browser, voice, and CUA remain deferred until their own
security and product decisions are approved.

New GUI assets in this phase: no fonts, images, icons, or third-party artwork.
The UI uses system fonts and CSS only. Any future asset must add its source URL,
fixed revision, license, copyright notice, and replacement decision here before
being committed.
