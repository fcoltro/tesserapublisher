# Releasing Tessera

## What a release is

A tag. `packaging/build.sh` turns a release build into an installer, the
`Release` workflow runs it on all three platforms when a `v*` tag is pushed, and
the artefacts come back attached to that run.

Installers are built from a tag and never from a branch, because a build
somebody can download has to correspond to a commit somebody can name. "The
artefact from run 4471" is not a version anybody can report a bug against.

## Before tagging

The profiles must be checked in. `assets/profiles/*.icc` are part of the
product, not part of the build: a release carries exactly the profiles the tests
ran against, rather than whatever a registry served that morning. The workflow
refuses to package without them, because an installer that starts and then
cannot soft-proof or export PDF/X is worse than one that was never built —
nothing about it looks wrong until somebody sends a job to a printer.

Run `tools/vendor-profiles.py --list` to see what is present. If it says
`MISSING`, see the note at the top of that script: on a locked-down machine the
network is reached with `curl --ssl-no-revoke` and the files are installed with
`--from DIR`.

## Building one locally

```
cargo build --release -p tessera_app
packaging/build.sh msi        # or dmg, or appimage
```

Everything lands in `packaging/out`. Packaging is written as a script rather
than as three blocks of workflow YAML for exactly this reason: it is the part of
a build somebody has to be able to run when it goes wrong, and a step that only
exists inside CI can only be debugged by pushing.

## What each platform carries

| | Icon | Association | Profiles |
| --- | --- | --- | --- |
| Windows | from the executable | `ProgId` + `Extension` + `Verb` in `apps/tessera_app/wix/main.wxs` | `profiles\` beside the exe |
| macOS | `Contents/Resources` | `CFBundleDocumentTypes` + an exported UTI | `Contents/Resources/profiles` |
| Linux | hicolor 512×512 | `.desktop` **and** a MIME package | `profiles/` beside the binary |

Two things on that table are easy to get half right:

- **Linux needs both files.** The `.desktop` says which application opens the
  type; `tessera-publisher.xml` says what the type *is*. With only the first,
  the desktop has nothing to associate. It is validated with
  `desktop-file-validate` before packaging, because a malformed one installs
  without complaint and the application simply is not in the menu, with nothing
  anywhere saying why.
- **The macOS UTI conforms to `public.data`, not `public.json`.** A Tessera
  document *is* JSON, and saying so would let every text editor claim it — and
  be offered ahead of the application that made it.

## Signing and notarization

**Not done by CI, and that is a decision rather than an omission.**

Both need certificates that belong to a person or an organisation, not to a
repository. A workflow that pretended to sign would produce something users are
told to trust and should not, and secrets that can sign a release are secrets
that can sign anything else that reaches the same runner.

What is owed, and by whom:

- **macOS** — a Developer ID Application certificate, `codesign --timestamp
  --options runtime` over the `.app`, then `xcrun notarytool submit --wait` on
  the `.dmg` and `xcrun stapler staple`. Without it Gatekeeper refuses to open
  the application at all on a machine that did not build it; "right-click and
  Open" is not an instruction anybody should have to give.
- **Windows** — an Authenticode certificate and `signtool sign /fd sha256 /tr`
  with a timestamp server. Without it SmartScreen warns on every download until
  enough people install it anyway, which is a reputation nobody can wait for.
- **Linux** — nothing is required. An AppImage is run as it is downloaded.

Until those exist, say so in the release notes rather than leaving people to
discover it from an operating system warning.

## Version numbers

One `version` in the workspace `Cargo.toml`, and every crate reads it. The
packaging script reads the same line, so the installer, the bundle and the tag
cannot disagree about what was built.
