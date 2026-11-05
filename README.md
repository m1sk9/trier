# trier

[![CI](https://github.com/m1sk9/trier/actions/workflows/ci.yaml/badge.svg)](https://github.com/m1sk9/trier/actions/workflows/ci.yaml)
[![Release Trier](https://github.com/m1sk9/trier/actions/workflows/release.yaml/badge.svg)](https://github.com/m1sk9/trier/actions/workflows/release.yaml)
[![MIT License](https://img.shields.io/github/license/m1sk9/trier?color=%239944ee)](https://github.com/m1sk9/trier/blob/main/LICENSE)
[![codecov](https://codecov.io/github/m1sk9/trier/graph/badge.svg?token=QA075J11S8)](https://codecov.io/github/m1sk9/trier)

Control Which Browser Opens Based on the URL.

---

**trier** opens each link in the browser its rules pick. It registers itself as the macOS default browser, matches every clicked URL against host rules in a TOML file, and hands the URL to the chosen browser.

macOS on Apple silicon (arm64) only.

## Install

### Homebrew

```sh
brew tap m1sk9/tap
brew install m1sk9/tap/trier
```

### Build from source

```sh
cargo install --git https://github.com/m1sk9/trier

# -- or --
git clone https://github.com/m1sk9/trier.git
cd trier
cargo build --release
```

A binary built this way is at `target/release/trier`; put it on your `PATH` (or run it by that path) for the steps below.

### Setup

```sh
trier config init     # write an example config to ~/.config/trier/config.toml
$EDITOR "$(trier config path)"
trier check           # validate it
trier register        # make trier the default browser
```

`trier register` asks macOS to switch the default browser; macOS may show a confirmation dialog. Run it again after every upgrade (see [Upgrading](#upgrading)).

## Configuration

The config file is found at, first match wins:

1. `--config <PATH>`
2. `$TRIER_CONFIG`
3. `$XDG_CONFIG_HOME/trier/config.toml`
4. `~/.config/trier/config.toml`

`trier config path` prints the one in effect. Unknown keys are errors, so typos are caught by `trier check`.

```toml
version = 1
default_browser = "safari"

[browsers."firefox@dev"]
bundle_id = "org.mozilla.firefoxdeveloperedition"

[[rules]]
hosts = ["github.com/ab-company/**"]
browser = "firefox@developer-edition"

[[rules]]
hosts = ["localhost", "127.0.0.1", "[::1]"]
browser = "firefox@dev"

[[rules]]
hosts = ["google.com"]
browser = "google-chrome"
include_subdomains = true
```

### Keys

| Key                           | Required | Meaning                                              |
| ----------------------------- | -------- | ---------------------------------------------------- |
| `version`                     | yes      | Schema version. Must be `1`.                         |
| `default_browser`             | no       | Browser for URLs no rule matches.                    |
| `[browsers.<name>]`           | no       | A custom browser (or an override of a built-in one). |
| `[browsers.<name>].bundle_id` | yes      | The app's bundle identifier.                         |
| `[[rules]]`                   | no       | Rules, evaluated in file order.                      |
| `rules[].hosts`               | yes      | Non-empty list of host patterns (see below).         |
| `rules[].browser`             | yes      | A built-in or custom browser name.                   |
| `rules[].include_subdomains`  | no       | Also match subdomains of each host. Default `false`. |

Browser names may contain `a-z 0-9 @ + . _ -` and must start with a letter or digit.

### Which browser opens a URL

1. The first rule with a matching host pattern, in file order. A later, more specific rule never wins over an earlier one; `trier check` warns when a rule is completely covered by an earlier one and can never match.
2. Otherwise `default_browser`.
3. Otherwise the browser that was the system default when you ran `trier register`.
4. Otherwise nothing: `trier explain` / `trier open` report an error, and a clicked link shows an alert.

Only `http` and `https` URLs are routed by rules. `trier explain <URL>` shows the decision for any URL.

### Host patterns

A pattern is `host[/path]`:

| Pattern                                       | Matches                          | Does not match     |
| --------------------------------------------- | -------------------------------- | ------------------ |
| `github.com`                                  | `github.com` with any path       | `gist.github.com`  |
| `github.com/**`                               | same as `github.com`             |                    |
| `github.com/ab-company/**`                    | `/ab-company`, `/ab-company/app` | `/ab-company-labs/x` |
| `github.com/ab-company`                       | `/ab-company`, `/ab-company/`    | `/ab-company/app`    |
| `github.com` with `include_subdomains = true` | `github.com`, `gist.github.com`  | `evilgithub.com`   |

- No scheme and no port; ports, queries and fragments of the URL are ignored.
- Hosts are case-insensitive; `www.` has no special meaning (write `www.example.com` separately if you need it).
- `**` is allowed only as the last path segment; `*` is reserved and rejected.
- Patterns are normalized like URLs: international domain names may be written as is, IPv6 addresses go in brackets (`[::1]`), `127.1` means `127.0.0.1`, and a trailing dot is ignored.
- Paths are compared segment by segment, case-sensitively and without percent-decoding: `github.com/ab-company/**` does not match `github.com/AB-Company/app`.
- `include_subdomains` has no effect on IP addresses.

### Browsers

Built-in names are Homebrew cask tokens:

`brave-browser`, `firefox`, `firefox@developer-edition`, `firefox@nightly`, `google-chrome`, `google-chrome@dev`, `microsoft-edge`, `microsoft-edge@beta`, `microsoft-edge@dev`, `mullvad-browser`, `opera`, `orion`, `safari`, `tor-browser`, `ungoogled-chromium`, `vivaldi`, `waterfox`, `zen`

Any other app needs a `[browsers]` entry with its bundle identifier. `trier browsers` lists every known browser with its bundle id and install path; for an app it does not know, read the id with:

```sh
mdls -name kMDItemCFBundleIdentifier -raw /Applications/Example.app
```

### Recipes

Check any of these with `trier explain <URL>` before relying on it, and run `trier check` after every edit.

#### Work repositories in a separate browser

```toml
[[rules]]
hosts = ["github.com/ab-company/**", "gitlab.com/ab-company/**"]
browser = "google-chrome"
```

Paths are case-sensitive. If links to the organization appear with different capitalization, list each spelling (`"github.com/AB-Company/**"`).

#### A whole domain, including its subdomains

```toml
[[rules]]
hosts = ["ab-company.example"]
browser = "microsoft-edge"
include_subdomains = true
```

This matches `ab-company.example`, `mail.ab-company.example` and `wiki.internal.ab-company.example`, but not `not-ab-company.example`.

#### Local development servers

```toml
[[rules]]
hosts = ["localhost", "127.0.0.1", "[::1]"]
browser = "firefox@developer-edition"
```

Ports are ignored, so this covers `http://localhost:3000`, `http://127.0.0.1:8080` and every other port.

#### A browser trier does not know

Look up the app's bundle identifier, then give it a name of your choice:

```sh
mdls -name kMDItemCFBundleIdentifier -raw "/Applications/Example Browser.app"
```

```toml
[browsers.example]
bundle_id = "com.example.browser" # the value printed above

[[rules]]
hosts = ["example.com"]
browser = "example"
```

`trier browsers` then lists it as `custom` with its install path. Defining a built-in name (for example `[browsers.safari]`) replaces that built-in's bundle id; `trier check` reports the override.

#### Specific rules before broad ones

The first matching rule wins, so a narrow rule must come before a broad one for the same host:

```toml
[[rules]]
hosts = ["github.com/ab-company/**"]
browser = "google-chrome"

[[rules]]
hosts = ["github.com"]
browser = "firefox"
```

In the opposite order every GitHub link would open in Firefox, and `trier check` would warn:

```text
warning: rules[1] never matches: every host is already matched by rules[0]
```

## Commands

| Command       | Example                                             | What it does                                                                              |
| ------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `check`       | `trier check`                                       | Validate the config; also warns about shadowed rules and browsers that are not installed. |
| `explain`     | `trier explain https://github.com/ab-company/trier`  | Show the matched rule (or fallback), browser, bundle id and app path.                     |
| `open`        | `trier open https://example.com https://github.com` | Open URLs in their decided browsers.                                                      |
| `browsers`    | `trier browsers`                                    | List built-in and custom browsers and where they are installed. Works without a config.   |
| `register`    | `trier register`                                    | Generate `~/Applications/Trier.app` and make it the default browser.                      |
| `unregister`  | `trier unregister`                                  | Restore the previous default browser and remove `Trier.app`.                              |
| `status`      | `trier status`                                      | Show the config, the bundle, the registered version and the current default browser.      |
| `config path` | `trier config path`                                 | Print the config file path in effect.                                                     |
| `config init` | `trier config init`                                 | Write the example config there (`--force` to overwrite).                                  |

Every command accepts `--config <PATH>`. `trier --help` lists them all.

## How it works

- `trier register` writes `~/Applications/Trier.app`: an `Info.plist` that declares the app can open `http` and `https` URLs, plus a copy of the `trier` binary. It signs the bundle ad hoc, registers it with Launch Services, records the current default browser, and sets the app as the default for `http` and `https`.
- When you click a link, macOS launches `Trier.app`. The `LSEnvironment` key in its `Info.plist` sets `TRIER_HANDLER=1`, which makes the binary start as a background app without a Dock icon, receive the URLs, apply the rules, and open each one with `open -b <bundle id>`. It exits shortly after the last link.
- A clicked link is never dropped silently: if the config cannot be loaded, no browser can be decided, or the chosen browser fails to open, the link opens in the browser recorded at `register` and an alert explains why.
- Local HTML files open in `default_browser` (or the recorded browser), because macOS also makes the default browser the viewer for HTML documents.
- Launch Services does not pass your shell environment to `Trier.app`. `register` therefore writes the config path and state directory it resolved into `LSEnvironment`, so clicked links read the same files as the command line.
- State lives in `$XDG_STATE_HOME/trier` or `~/.local/state/trier`: `state.toml` (recorded browser, registered version) and `trier.log` (one line per clicked link; set `TRIER_LOG=debug` for more). The log is readable only by you, since URLs can carry login tokens.

### Upgrading

`Trier.app` contains a copy of the binary, so after `brew upgrade trier` run `trier register` again to update it. `trier status` warns when the bundle is older than the installed `trier`.

### Uninstalling

Run `trier unregister` **before** `brew uninstall trier`, so the previous default browser is restored and `Trier.app` is removed. Then delete `~/.config/trier` and `~/.local/state/trier` if you no longer need them.

## Troubleshooting

- **Nothing happens when I click a link.** Run `trier status`: it shows whether trier is still the default browser and, on its `log:` line, where the log of every click is (by default `~/.local/state/trier/trier.log`).
- **Links ignore my config after I moved it.** `Trier.app` uses the config path and state directory that were in effect at `trier register`. Run `trier register` again after changing `TRIER_CONFIG`, `XDG_CONFIG_HOME` or `XDG_STATE_HOME`.
- **`register` says the default is still another browser.** macOS asks for confirmation; once you accept, `trier status` shows `default: trier`. You can also pick Trier under System Settings > Desktop & Dock > Default web browser.
- **An alert says the config cannot be loaded.** The link still opened in the recorded browser. Fix the config with `trier check`.

## License

[MIT](LICENSE)
