# Fork distribution

This branch (`dist`) is what gets installed on my machines. It carries upstream
`master`, my feature branches, and the few changes needed to build and ship this
fork as its own release channel. Nothing here is meant to go upstream.

## What is different from upstream

| Change | Where | Why |
| --- | --- | --- |
| Fork build workflow | `.github/workflows/fork-build.yml` | Upstream's `release.yml` and `preview.yml` are gated on `github.repository == 'herdrdev/herdr'`, so they never run here. A separate file means upstream merges never conflict with it. |
| Manifest helpers | `scripts/fork_manifest.py`, `scripts/fork_protocol_version.py` | Small edits around `scripts/preview.py`, kept out of the workflow YAML so they can be run locally. |
| Preview manifest URL | `src/update.rs`, `src/remote/attach.rs` | Points `herdr update` at this fork's manifest instead of `herdr.dev/preview.json`. |
| `UpdateChannel::configured()` always returns `Preview` | `src/update.rs` | The stable channel resolves to upstream's `latest.json`. Without this, an unset or reset `update.channel` would replace a fork build with an upstream release. |

Fork builds are stamped `HERDR_BUILD_CHANNEL=preview` with a per-build
`HERDR_BUILD_ID`, so `herdr --version` reports `0.8.2-preview.<build id>`. The
updater treats *any* change of build id as an update
(`release_info_from_preview_manifest` in `src/update.rs`), which is why fork
builds do not need a version bump in `Cargo.toml`.

## Cutting a build

```sh
gh workflow run fork-build.yml --ref dist
gh run watch
```

It builds `x86_64-pc-windows-msvc` on a GitHub-hosted runner (free — this fork
is public), packages the ConPTY bundle with upstream's
`scripts/package_windows_conpty.ps1`, and publishes a `dist-<build id>` release
carrying three assets:

- `herdr-windows-x86_64.zip` — the build
- `preview.json` — the update manifest, pointing at that zip
- `install.ps1` — a copy of upstream's installer, so bootstrapping needs nothing
  from herdr.dev

The release is marked latest, which is what keeps
`https://github.com/McNultyyy/herdr/releases/latest/download/preview.json` a
stable URL.

## Installing on a new machine

```powershell
$env:HERDR_MANIFEST_URL = 'https://github.com/McNultyyy/herdr/releases/latest/download/preview.json'
irm https://github.com/McNultyyy/herdr/releases/latest/download/install.ps1 | iex
```

The installer takes every argument from the environment, so that is all it
needs; it reads `base_version` and `build_id` out of the manifest and installs
`0.8.2-preview.<build id>`.

This installs to the normal `%LOCALAPPDATA%\Programs\Herdr\bin`, replacing a
stock install. Detach and stop any running herdr server first, or the installer
will refuse to activate over locked files.

## Updating

```sh
herdr update
```

Run it outside herdr. It reads the fork manifest, downloads the asset itself and
hands it to the embedded installer, so nothing on the machine needs to remember
the manifest URL after the first install.

## Syncing with upstream

```sh
git fetch upstream
git checkout master && git merge --ff-only upstream/master   # master stays a clean mirror
git checkout dist && git merge master
```

Only `src/update.rs` and `src/remote/attach.rs` can conflict, and only in the few
lines noted above.
