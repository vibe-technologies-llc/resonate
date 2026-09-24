---
paths:
  - "packaging/**"
---

# Packaging

The Arch PKGBUILD and Fedora RPM spec build the default-feature `resonate` binary and install the
desktop entry, icon, metainfo, man pages and shell completions together. The man pages and
completions come from the binary crate's build script under `target/release/build/resonate-*/out`,
so a CLI change reaches both packages without a second command list.

Both package builds export `RUSTFLAGS` during build and test. The checkout's `.cargo/config.toml`
uses `target-cpu=native` for local performance, and the Arch PKGBUILD keeps that for an AUR build
made on the listener's machine. The Fedora spec appends `target-cpu=x86-64` for x86_64 and
`target-cpu=generic` for aarch64, after any Fedora flags, so a release runs on older machines of
the same architecture.
It builds from a source archive of the published release tag, whose version must match the spec,
takes development headers from Fedora 44, and uses Cargo's locked dependency graph. The release
workflow uploads the binary and source RPMs after the build and tests pass. Cargo fetches crates
while building; this is a source RPM recipe, not an offline Fedora distribution build.
