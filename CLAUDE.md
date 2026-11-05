# CLAUDE.md

## Default browser registration

`trier register` sets the default `http` / `https` handler with
`LSSetDefaultHandlerForURLScheme` (`src/macos.rs`, `set_default_handler`).

- Apple deprecated it in macOS 12. On macOS 27.0.1 (2026-11-05) a no-op call
  (setting the current handler again) returned `0`. Actually changing the
  handler (and the confirmation dialog macOS may show) is covered only by
  the manual end-to-end steps in the Phase 4 PR (#5), not by tests.
- If it stops working, switch to
  `NSWorkspace::setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler`
  (objc2-app-kit feature `block2`): pass the bundle URL and wait on the
  completion handler with a bounded run loop.

## Tests must not create a real `dev.m1sk9.trier` app bundle

macOS registers any complete app bundle it finds (whatever its extension)
with Launch Services. A test bundle carrying `dev.m1sk9.trier` then receives
the user's clicked links and fails with `-54`. Tests use
`test_support::fake_trier_bundle` (an `Info.plist` with only the identifier)
or render a complete bundle with a different identifier and delete it.
