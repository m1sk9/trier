# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`trier` is a macOS (arm64) command-line tool that becomes the system default browser and opens each clicked link in the browser chosen by host rules in a TOML file. `trier register` generates `~/Applications/Trier.app` (an `Info.plist` plus a copy of the binary), registers it with Launch Services and makes it the `http`/`https` handler; Launch Services then launches that bundle with `TRIER_HANDLER=1`, and the same binary runs as a background AppKit app that routes the URLs. Distributed through the Homebrew tap `m1sk9/tap` as a prebuilt `aarch64-apple-darwin` binary. See `README.md` for the user manual (config schema, pattern grammar, commands, troubleshooting).

## Common commands

```sh
cargo build
cargo test --verbose                 # all tests
cargo test <name>                    # filter by test name substring
cargo test --lib pattern::tests      # one module

cargo fmt --all -- --check           # CI gate: rustfmt
cargo clippy --all-targets --all-features -- -D warnings   # CI gate: clippy

# Coverage (matches CI; nightly so `coverage(off)` takes effect)
cargo +nightly llvm-cov --all-features --workspace
RUSTFLAGS='--cfg coverage_nightly' cargo +nightly check --all-targets   # the feature gate compiles

# Try it without touching the system
cargo run -- check --config config/config.toml.example
cargo run -- explain https://github.com/mk-system/trier --config config/config.toml.example
```

`cargo run -- register` / `unregister` change the real default browser and write `~/Applications/Trier.app`; run them only when you mean to. Toolchain pinned to stable via `rust-toolchain.toml`. Edition 2024. The crate refuses to compile on anything but macOS.

## Architecture

`src/main.rs` holds only the clap definitions and dispatch (excluded from coverage). Before parsing arguments it checks `TRIER_HANDLER=1` and enters handler mode.

- **`paths`** — XDG-style resolution: config `--config` > `TRIER_CONFIG` > `$XDG_CONFIG_HOME/trier/config.toml` > `~/.config/trier/config.toml`; state `$XDG_STATE_HOME/trier` > `~/.local/state/trier` (`state.toml`, `trier.log`). Empty and relative `XDG_*` values count as unset. `dirs`-style crates are deliberately not used: they map macOS to `~/Library/Application Support`.
- **`config`** — the TOML model (`Config`, `Rule`, `CustomBrowser`). Every struct has `deny_unknown_fields`; invariants live in `TryFrom` newtypes (`ConfigVersion`, `BrowserName`, `BundleId`, `HostPattern`). `FromStr for Config` adds the cross-field checks (non-empty `hosts`, every browser name resolves) and reports all violations at once.
- **`pattern`** — `HostPattern` parsing, `matches` and `covers`. Patterns are canonicalized through the `url` crate (IDN → punycode, IP literal spellings, percent-encoding, dot segments, trailing dot) so they compare equal to the URLs being matched. A bare host is stored as `host/**`. `covers` drives the shadowed-rule warning in `check`.
- **`browser`** — `BrowserName`, `BundleId`, the `BUILTIN` table (Homebrew cask token → bundle id) and `BrowserRegistry` (built-in merged with `[browsers]`, custom wins). Only verified bundle ids go into `BUILTIN` (`mdls` on a real install, or the cask's `uninstall quit:` stanza); never from memory.
- **`resolve`** — `decide(url, config, registry, recorded_default)`: first matching rule in file order, then `default_browser`, then the recorded system default, else `NoFallback`. Only `http`/`https`.
- **`state`** — `state.toml` (`previous_default_browser_bundle_id`, `registered_version`). Missing file = default; unknown fields tolerated; `save` is atomic (temp file + rename); `record_previous_default` never records trier itself.
- **`macos`** — every call into Launch Services / AppKit lookups and every macOS executable (`open -b`, `osascript`, `codesign`, `lsregister`). argv builders are pure and tested; the functions with side effects are `coverage(off)`. Child processes never inherit `TRIER_HANDLER`.
- **`register`** — bundle generation (`render_info_plist`, `write_bundle`, identity check `is_trier_bundle`, `remove_bundle` / `retire_bundle`) and the `register` / `unregister` step sequences. `LSEnvironment` carries `TRIER_HANDLER=1` plus the `TRIER_CONFIG` / `XDG_STATE_HOME` that `register` resolved, because Launch Services launches do not inherit the shell environment. A bundle is touched only if its `Info.plist` names `dev.m1sk9.trier`.
- **`handler`** — handler mode: AppKit app with `Prohibited` activation policy, `define_class!` delegate receiving `application:openURLs:`, log to `trier.log` (mode `0600`), watchdog (exit 5 s after launch without URLs, 1 s after the last batch; decided under the same lock that marks a batch as routing).
- **`dispatch`** — routing for handler mode, kept out of `handler` so it is testable: config and state read per event; on any failure the URL opens in the recorded default with an alert; `file://` URLs open in `default_browser`.
- **`cli`** — one module per subcommand (`check`, `explain`, `open`, `browsers`, `register`, `unregister`, `status`, `config`) plus shared pieces: `Report` (output buffered in order and written by `main`, or `Report::live()` for `register`/`unregister`, whose progress must show before macOS's dialog), `UrlResolver` (reads state at most once and only when needed), and the injected `AppLookup` / `Opener` so tests neither depend on installed apps nor launch browsers.

## Conventions specific to this repo

- **Errors are typed per module** with `thiserror` (`PathsError`, `ConfigError`, `PatternError`, `ResolveError`, `StateError`, `MacosError`, `RegisterError`). `anyhow` only in `main.rs`.
- **Config invariants are types.** New config fields get a `TryFrom` newtype or a cross-field check in `Config::from_str`, never a string parsed later. Keep `deny_unknown_fields` on config structs (not on `State`).
- **Rule order is the semantics.** First match wins; do not add specificity scoring. Shadowing is reported by `check` instead.
- **macOS APIs stay in `macos.rs`**, except the AppKit application loop in `handler.rs`, which cannot live elsewhere.
- **Never drop a click silently.** Any failure in handler mode must still open the URL somewhere or show an alert.
- **Subcommands do no I/O on stdout/stderr directly**; they write to `Report`.
- **Env tests** use the `EnvGuard` in `paths.rs` (`unsafe { std::env::set_var }` on edition 2024, serialized by a `static Mutex`).
- **Test temp files** go under `target/tmp/<module>` via `test_support`, never `std::env::temp_dir()` (it reads `TMPDIR` while env tests mutate the environment). Each test uses its own file names; parallel tests sharing one file are flaky.
- **Tests must not create a real `dev.m1sk9.trier` app bundle.** macOS registers any complete app bundle it finds, whatever its extension, and one carrying `dev.m1sk9.trier` receives the user's clicked links and fails with `-54`. Use `test_support::fake_trier_bundle` (an `Info.plist` holding only the identifier) or render a complete bundle with another identifier and delete it.
- **Coverage:** test modules and side-effecting macOS functions are `#[cfg_attr(coverage_nightly, coverage(off))]`; `codecov.yml` ignores `main.rs`, `macos.rs`, `handler.rs`, `register.rs`. `dispatch.rs` stays measured.
- **Comments say why not**, in English; code says how. **Commit messages: Conventional Commits, English**; releases are automated by release-please.

## Default browser registration

`register` sets the default `http` / `https` handler with `LSSetDefaultHandlerForURLScheme` (`macos::set_default_handler`). Apple deprecated it in macOS 12; on macOS 27.0.1 (2026-11-05) it still worked, switching the default after the system confirmation dialog (verified manually in #13). If it stops working, switch to `NSWorkspace::setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler` (objc2-app-kit feature `block2`): pass the bundle URL and wait on the completion handler with a bounded run loop.

## CI and release

`.github/workflows/ci.yaml`, all on `macos-15`, required before merge:

1. `check` — `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --verbose`.
2. `coverage` — `cargo +nightly llvm-cov` → Codecov (`fail_ci_if_error: true`).
3. `build (aarch64-apple-darwin)` — `cargo build`.

`.github/workflows/release.yaml`: release-please opens the release PR; merging it tags `vX.Y.Z`, then `assets` builds `trier-aarch64-apple-darwin.tar.gz` (`trier`, `LICENSE`, `README.md`; versionless name), `release-assets` uploads it with `.sha256` and `SHA256SUMS`, and `homebrew` dispatches `publish` to `m1sk9/homebrew-tap`, whose workflow opens the `Formula/trier.rb` bump PR. The formula is `depends_on :macos` / `depends_on arch: :arm64`, and its caveats tell users to run `trier register` after every install or upgrade and `trier unregister` before uninstalling.

Run `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test` locally before pushing.
