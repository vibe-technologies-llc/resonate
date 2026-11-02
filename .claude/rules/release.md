---
paths:
  - ".github/workflows/release.yml"
  - "packaging/**"
  - "Cargo.toml"
  - "docs/ROADMAP.md"
---

# Releases

## Versions

- **Semantic versioning.** A patch release (`0.1.1`) carries fixes only; a minor or major release
  (`0.2.0`, `1.0.0`) carries features. `-rcN` marks a candidate. The first version is `0.1.0`, released without a candidate.
- **The workspace manifest's `[workspace.package] version` is the one source.** Every crate
  inherits it, the internal path dependencies in `[workspace.dependencies]` repeat it, and Gradle
  reads it for `versionName` (`versionCode` is derived from it: `((major * 100 + minor) * 100 +
  patch) * 10 + candidate`, with `9` for a final release, so a candidate sorts below its release).
  Raising it also refreshes `Cargo.lock` (`cargo update -w`).
- **Each ecosystem spells a pre-release its own way**: Cargo and the tag, as `0.1.0-rc1`; RPM and Debian
  `0.1.0~rc1`, which sorts below `0.1.0`; Arch `0.1.0rc1`, since `pkgver` may not hold a hyphen;
  AppStream `0.1.0~rc1`. iOS's `CFBundleShortVersionString` allows digits only, so it stays
  `0.1.0` and `CFBundleVersion` counts builds. `crates/resonate/tests/packaging.rs` holds each
  recipe to the manifest's version.

## Branches, tags and releases

- **A `release/MAJOR.MINOR` branch is cut from `master` for each minor or major line**, when its
  first candidate is tagged. A fix to a released line lands on `master` and is cherry-picked to
  that branch; a patch release is tagged on it.
- **Every release is a tag `vMAJOR.MINOR.PATCH[-rcN]` and a GitHub release.** Pushing the tag runs
  `release.yml` (`build.md`), which checks the tag against the manifest's version and attaches the
  assets. The roadmap's targets (`docs/ROADMAP.md`) name what each release is meant to hold.

- **A release carrying the iPhone app is begun by `python3 ios/release.py vX.Y.Z`, not by pushing
  the tag**: the tag is made first on the commit checked out, already on the remote, and not yet
  pushed. The script builds the IPA in release (the SDK is the listener's Xcode, so no runner can),
  creates the draft release itself, puts the IPA (and, for a release that is not a candidate, the
  AltStore source) on it, and only then pushes the tag; `release.yml`'s `release` job finds the draft
  and leaves it, and `publish` attaches the rest. Pushing the tag alone still releases, without the
  IPA, since a published release takes no asset afterwards.
- **Releases are immutable**: the workflow creates the release as a draft, attaches the assets and
  only then publishes it, since a published release accepts no new asset. A tag name that a
  published release has used can never be used again, even after the release is deleted, so a
  release is deleted only with its version abandoned.

## Assets

- **All Linux packages target x86-64-v3**: RPM and SRPM (Fedora 44), DEB (Ubuntu 24.04) and
  `.pkg.tar.zst` (Arch), for the player and the sync server, built with
  `RUSTFLAGS=-C target-cpu=x86-64-v3` over the checkout's `native`. A machine older than v3 builds
  from source.
- **The Windows files are, per architecture, `resonate-<version>-x86_64_v3-windows.zip` and
  `resonate-<version>-x86_64_v3-windows-setup.exe`, and `resonate-<version>-aarch64-windows.zip`
  and `resonate-<version>-aarch64-windows-setup.exe`**: `resonate.exe` built for x86-64-v3, or for
  `aarch64-pc-windows-msvc` at the target's baseline, the licence and a README in the zip, the same
  in a per-user Inno Setup installer (Start menu entry, file associations; `windows.md`), from
  `packaging/windows/build.ps1 -Architecture`. They are signed (`sign.ps1`, Authenticode) only
  where the repository holds the secrets `WINDOWS_CERTIFICATE_BASE64` (a PFX, base64) and
  `WINDOWS_CERTIFICATE_PASSWORD`; none exists, so the files are unsigned and Windows SmartScreen
  warns on a first run.
- **`resonate-<version>-winget-manifests.zip` and `resonate-<version>-scoop.json` are filled
  from the templates** in `packaging/windows/` by `fill-manifests.sh` in the `publish` job, once
  every file above exists: the version, the repository URL of the running workflow, the asset URLs
  and their SHA-256 (the winget installer manifest also takes the Inno `AppId` as its product code).
  The zip holds `manifests/r/Resonate/Resonate/<version>/` as a pull request to winget-pkgs
  wants it; the JSON is a Scoop manifest for the zip archives (a Scoop install makes a Start menu
  shortcut with no `AppUserModelID`, so toasts need the installer). **Submitting either upstream
  is a manual step the maintainer takes after the release is published**: a pull request of the
  winget folder to winget-pkgs (`wingetcreate` or `winget validate` first), and the JSON
  added to a Scoop bucket. A candidate (`-rcN`) is not submitted.
- **The APK is a signed release build**: `assembleRelease` under the repository's signing secrets,
  then `apksigner verify` (`build.md` has the job). The keystore is removed at the end of the job
  and its secrets are never echoed.
- **The iPhone app is `resonate-<version>-ios.ipa`**, ad-hoc signed and carrying no provisioning
  profile, so whoever installs it signs it with their own Apple ID (SideStore, AltStore, Sideloadly,
  xtool). **`resonate-altstore.json` is the AltStore and SideStore source** (`ios/altstore.py`): the
  app's name, bundle, permissions (the entitlements and every `NS…UsageDescription`) and every
  version published, newest first, each read from the last published source and the IPA's own
  `Info.plist`; its URLs are the running repository's (`gh repo view`), the icon `Resonate/Icon.png`
  at the tag. Listeners add `<repository>/releases/latest/download/resonate-altstore.json` once,
  so every final release must carry the file: `publish` takes the IPA and source from the draft and,
  where the draft has no source, carries the last published one forward unchanged. A candidate
  carries the IPA alone, since `latest` never names a pre-release.
- `SHA256SUMS` lists every asset.
