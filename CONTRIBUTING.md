# Contributing

A contribution is licensed under the GNU Affero General Public License, version 3 or
later, the same terms as the program. See [LICENSE](LICENSE).

The [README](README.md) has the build. Before sending a change:

```
cargo test --workspace --exclude resonate-ui --no-default-features
cargo clippy --workspace --all-targets -- -D warnings
```

Format Rust with `rust-formatter`. It drives the nightly rustfmt this tree is written
with. `cargo fmt` produces different output.
