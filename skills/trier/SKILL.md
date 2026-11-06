---
name: trier
description: Configure and use trier, a macOS CLI that opens each clicked link in the browser chosen by host rules in a TOML file. Use when the user wants to route URLs or domains to specific browsers, edit or debug trier's config.toml, or run trier commands (check, explain, register).
---

# trier

trier makes itself the macOS default browser and opens each clicked `http`/`https` link in the browser picked by host rules in a TOML config. macOS on Apple silicon only.

## Workflow

1. Find the config: `trier config path` (create an example with `trier config init`).
2. Edit it.
3. `trier check` — must pass. Read its warnings (shadowed rules, browsers not installed).
4. `trier explain <URL>` — confirm the intended rule and browser for a few real URLs.
5. `trier register` — only if trier is not yet the default browser (`trier status` shows it). It changes a system setting, so ask the user first. After `brew upgrade trier`, run it again.

Never guess the schema or a bundle id: validate with `trier check`.

## Config

Location, first match wins: `--config <PATH>`, `$TRIER_CONFIG`, `$XDG_CONFIG_HOME/trier/config.toml`, `~/.config/trier/config.toml`. Unknown keys are errors.

```toml
version = 1                      # required, must be 1
default_browser = "safari"       # optional: used when no rule matches

[browsers.example]               # optional: a browser trier does not know
bundle_id = "com.example.browser"

[[rules]]                        # evaluated in file order
hosts = ["github.com/ab-company/**"]
browser = "firefox@developer-edition"

[[rules]]
hosts = ["localhost", "127.0.0.1", "[::1]"]
browser = "example"

[[rules]]
hosts = ["google.com"]
browser = "google-chrome"
include_subdomains = true        # optional, default false
```

- `rules[].hosts` is a non-empty list; `rules[].browser` is a built-in or `[browsers]` name.
- Built-in names are Homebrew cask tokens: `brave-browser`, `firefox`, `firefox@developer-edition`, `firefox@nightly`, `google-chrome`, `google-chrome@dev`, `microsoft-edge`, `microsoft-edge@beta`, `microsoft-edge@dev`, `mullvad-browser`, `opera`, `orion`, `safari`, `tor-browser`, `ungoogled-chromium`, `vivaldi`, `waterfox`, `zen`. `trier browsers` lists them with install paths.
- Any other app needs `[browsers.<name>]` with its bundle id: `mdls -name kMDItemCFBundleIdentifier -raw /Applications/<App>.app`. Names use `a-z 0-9 @ + . _ -` and start with a letter or digit. Defining a built-in name overrides its bundle id.

## Which browser opens a URL

1. The first rule whose host pattern matches, in file order. There is no specificity scoring: put narrow rules before broad ones (`trier check` warns about a rule that an earlier one fully covers).
2. Otherwise `default_browser`.
3. Otherwise the system default recorded at `trier register`.
4. Otherwise an error (`explain`/`open`) or an alert (a clicked link).

Only `http`/`https` URLs are routed by rules.

## Host patterns

A pattern is `host[/path]`, with no scheme and no port (ports, queries and fragments of the URL are ignored).

| Pattern | Matches | Does not match |
| --- | --- | --- |
| `github.com` (same as `github.com/**`) | any path on `github.com` | `gist.github.com` |
| `github.com/ab-company/**` | `/ab-company`, `/ab-company/app` | `/ab-company-labs/x` |
| `github.com/ab-company` | `/ab-company`, `/ab-company/` | `/ab-company/app` |
| `github.com` + `include_subdomains = true` | `github.com`, `gist.github.com` | `evilgithub.com` |

- Hosts are case-insensitive; paths are case-sensitive and not percent-decoded.
- `www.` is not special: list `www.example.com` separately if needed.
- `**` only as the last path segment; `*` is rejected.
- IPv6 goes in brackets (`[::1]`); `127.1` means `127.0.0.1`; a trailing dot is ignored.
- `include_subdomains` has no effect on IP addresses.

## Commands

| Command | Purpose |
| --- | --- |
| `trier check` | Validate the config; warn about shadowed rules and missing browsers |
| `trier explain <URL>` | Show the matched rule, browser, bundle id and app path |
| `trier open <URL>...` | Open URLs in their decided browsers |
| `trier browsers` | List built-in and custom browsers with install paths |
| `trier register` | Make trier the default browser (system change, may show a macOS dialog) |
| `trier unregister` | Restore the previous default browser; run before uninstalling |
| `trier status` | Show config, bundle, registration and the current default browser |
| `trier completions <SHELL>` | Print a shell completion script (bash, zsh, fish, elvish) |
| `trier config path` / `config init [--force]` | Print the config path / write an example config |

Every command accepts `--config <PATH>`.

## Troubleshooting

- Links ignore the config after it was moved, or `TRIER_CONFIG`/`XDG_*` changed: the app keeps the paths from `trier register`; run it again.
- Nothing happens on click: `trier status` shows the default browser and the log path (`~/.local/state/trier/trier.log`).
- If the config fails to load, links still open in the recorded browser with an alert; fix it with `trier check`.
