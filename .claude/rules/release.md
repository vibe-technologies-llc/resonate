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

- **Releases are immutable**: the workflow creates the release as a draft, attaches the assets and
  only then publishes it, since a published release accepts no new asset. A tag name that a
  published release has used can never be used again, even after the release is deleted, so a
  release is deleted only with its version abandoned.

## Assets

- **All Linux packages target x86-64-v3**: RPM and SRPM (Fedora 44), DEB (Ubuntu 24.04) and
  `.pkg.tar.zst` (Arch), for the player and the sync server, built with
  `RUSTFLAGS=-C target-cpu=x86-64-v3` over the checkout's `native`. A machine older than v3 builds
  from source.
- **The APK is a signed release build**: `assembleRelease` under the repository's signing secrets,
  then `apksigner verify` (`build.md` has the job). The keystore is removed at the end of the job
  and its secrets are never echoed.
- `SHA256SUMS` lists every asset.
