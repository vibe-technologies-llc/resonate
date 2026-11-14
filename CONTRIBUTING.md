# Contributing

A contribution is licensed under the GNU Affero General Public License, version 3 or
later, the same terms as the program. See [LICENSE](LICENSE).

The [README](README.md) has the build. Before sending a change:

```
cargo nextest run --workspace --exclude resonate-ui --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
```

Tests run through [cargo-nextest](https://nexte.st), not `cargo test`: each test is a process
of its own, and a crate's integration tests are one binary, `suite`.

Format Rust with `rust-formatter`. It drives the nightly rustfmt this tree is written
with. `cargo fmt` produces different output.
