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
  `0.1.0` and `CFBundleVersion` counts builds; the macOS bundle's two keys are both that
  `major.minor.patch`, cut from the version by `packaging/macos/build.sh`. `crates/resonate/tests/packaging.rs` holds each
  recipe to the manifest's version.

## Branches, tags and releases

- **A `release/MAJOR.MINOR` branch is cut from `master` for each minor or major line**, when its
  first candidate is tagged. A fix to a released line lands on `master` and is cherry-picked to
  that branch; a patch release is tagged on it.
- **`master` names the oncoming release, and the release commit changes no version.** A `master`
  at `0.10.0` is released as `0.10.0`. Once that release's tag is pushed and its `release/`
  branch cut, `master` is raised to the next minor (`0.11.0`) in a commit of its own, so it again
  names the release it builds towards; the release branch keeps its own line's version and raises
  only its patch.
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
- **A release pins every store's new steps in the release commit**, so the steps it ships are never
  edited after: the phones' catalog appends to `STEPS_RELEASED` (`resonate-mobile`'s
  `catalog_released_tests.rs`, `android.md`), the ledger, the server's store and `kept.sqlite` to
  the `STEPS_RELEASED` of their own `released.rs` beside `ledger.rs`, `store.rs` and `kept.rs`, each
  step with the FNV-1a of its text (`fingerprint_of`), and the desktop catalog appends its stamps
  as shipped to `STAMPS_RELEASED` (`resonate-library`'s `schema_released_tests.rs`, `library.md`).
  Each also opens a store laid at every released step and carries it to the last.
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
  where the `release-signing` environment holds the secrets `WINDOWS_CERTIFICATE_BASE64` (a PFX,
  base64) and `WINDOWS_CERTIFICATE_PASSWORD` (*Signing apart from the build*, below); none exists,
  so the files are unsigned and Windows SmartScreen warns on a first run.
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
- **The macOS files are `resonate-<version>-aarch64-macos.dmg` and
  `resonate-<version>-x86_64_v3-macos.dmg`**: `Resonate.app` for Apple Silicon
  (`aarch64-apple-darwin`) or an Intel Mac (`x86_64-apple-darwin` at x86-64-v3), macOS 13 or
  later, beside a link to `/Applications`, the licence and a README, from `packaging/macos/build.sh`
  on `macos-latest`, one matrix leg each (`packaging.md`). The
  application is signed with a Developer ID (`sign.sh`, hardened runtime) only where the
  `release-signing` environment holds `MACOS_CERTIFICATE_BASE64` (a Developer ID Application `.p12`, base64) and
  `MACOS_CERTIFICATE_PASSWORD`, and the image notarised and stapled only where it also holds
  `MACOS_NOTARY_KEY_BASE64` (an App Store Connect API key, the `.p8`, base64),
  `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER`; none exists, so the application is signed ad
  hoc and Gatekeeper refuses a first run until the listener allows it under Privacy & Security,
  which the README walks through.
- **`resonate-<version>-cask.rb` is filled from `packaging/macos/resonate.rb`** by `fill-cask.sh`
  in the `publish` job, only where both images were built: the version, the repository URL of the
  running workflow and each image's SHA-256. **Adding it to a Homebrew tap is a manual step** the
  maintainer takes after the release is published, as winget and Scoop are; a candidate is not
  added.
- **The APK is a signed release build**: `assembleRelease` with no signing configuration (the
  unsigned `app-release-unsigned.apk`, held to `check-release.py`), then `zipalign -P 16` and
  `apksigner sign` with the keystore of the `release-signing` environment, then `apksigner verify`
  (`build.md` has the jobs). The keystore is written out only in the step that signs, removed
  whatever happens, and its secrets are never echoed; a release whose environment holds no
  keystore fails rather than shipping an unsigned APK.
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

## Signing apart from the build

- **No runner that builds holds a signing secret.** A build runs the build scripts and proc-macros
  of every package in the lock, gpui's git tree included, and anything it starts may outlive its
  step, rewrite a script a later step runs or append to `$GITHUB_ENV`, so a secret in a later step
  of the same job is as exposed as one in the build's own. Each signed platform is two jobs:
  one builds with no secret and uploads the unsigned output as an `unsigned-*` artifact
  (`windows-programs`: `build.ps1 -Stage Programs`, the two programs; `macos-bundle`:
  `build.sh VERSION OUTPUT ARCH bundle`, `Resonate.app` in a tar, which keeps its modes;
  `android-release`: `assembleRelease`), and a fresh runner downloads it and signs it with nothing
  of the build on it (`windows-zip`: `sign.ps1` over the programs, `build.ps1 -Stage Package`
  for the zip and the installer with no secret, `sign.ps1` over the installer; `macos-dmg`:
  `build.sh VERSION OUTPUT ARCH image`, which signs the application, makes the image and signs
  and notarises it; `android-apk`: `zipalign`, then `apksigner sign` with the keystore decoded in
  that step alone). `publish` collects `resonate-*` alone, so an `unsigned-*` artifact is never
  released. `build.ps1` and `build.sh` with no stage still do the whole, signing included, for a
  build by hand. `android-apk.yml` and `macos-dmg.yml` are the same two jobs each.
  `crates/resonate/tests/signing.rs` holds it: a job holding a `secrets.` value runs no `cargo`,
  `rustup`, Gradle, cache or build stage, names the environment, and hands each secret only to a
  signing step, and the keystore is decoded only where `apksigner` signs
  (`no_job_that_builds_holds_a_signing_secret_and_every_job_that_signs_names_the_environment`).
  `.gitattributes` keeps every `*.sh` at LF in a Windows checkout, since the test cuts `build.sh` into its
  stages at a line's end and a shell script with CRLF endings fails under a shell anyway.
- **Each signing job names the environment `release-signing` for a release alone.** `release.yml`
  asks for it where the run releases a tag (`needs.release.outputs.tag`), and the dispatch
  workflows where they run on a `v*` tag; any other run names no environment, so a branch builds
  unsigned: the Windows files unsigned, the application signed ad hoc, the APK uploaded as
  `resonate-<version>-unsigned.apk`. The environment's rule reads the ref the workflow runs on, so
  the `release` job refuses a `tag` input that is not that ref: a release dispatched by hand runs
  as `gh workflow run release.yml --ref vX.Y.Z`
  (`a_release_is_signed_only_from_its_own_tag_and_a_branch_is_built_unsigned`).
- **What the repository's settings must hold, which this tree cannot**: the environment
  `release-signing` (Settings, Environments; the first run creates it bare), its *Deployment
  branches and tags* set to *Selected branches and tags* with the one tag rule `v*` and no branch
  rule (`gh api -X PUT repos/OWNER/REPO/environments/release-signing -F
  'deployment_branch_policy[protected_branches]=false' -F
  'deployment_branch_policy[custom_branch_policies]=true'`, then `gh api -X POST
  repos/OWNER/REPO/environments/release-signing/deployment-branch-policies -f 'name=v*' -f
  type=tag`), and every signing secret moved into it: `ANDROID_KEYSTORE_BASE64`,
  `RESONATE_SIGNING_STORE_PASSWORD`, `RESONATE_SIGNING_KEY_ALIAS`, `RESONATE_SIGNING_KEY_PASSWORD`,
  and, once they exist, `WINDOWS_CERTIFICATE_BASE64`, `WINDOWS_CERTIFICATE_PASSWORD`,
  `MACOS_CERTIFICATE_BASE64`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_NOTARY_KEY_BASE64`,
  `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER`, each set with `gh secret set NAME --env
  release-signing` and its repository copy removed with `gh secret delete NAME`: a repository
  secret stays readable by every job of every branch, an environment's only shadowing it. The rule
  admits any `v*` tag, so who may push one is the remaining boundary: a tag ruleset restricting the
  creation of `v*` tags to the maintainer closes it, and required reviewers on the environment add
  a hand's approval before each signing.
