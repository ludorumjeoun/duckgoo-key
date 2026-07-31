# DuckGooKey

DuckGooKey is a fast, keyboard-first macOS launcher written in Rust. It is a
clean-room implementation inspired by the interaction model of
[SuperCmd](https://github.com/SuperCmdLabs/SuperCmd), with an independent Rust
codebase and no copied source or assets.

The first production scope is deliberately focused: launch installed
applications quickly, keep frequently used and pinned items at the top, and
ship native macOS packages with a reproducible Cloudflare R2 release path.

## What works

- Native Iced UI with no webview or JavaScript runtime
- Application discovery in `/Applications`, `/System/Applications`, and
  `~/Applications`
- Case-insensitive prefix, substring, and subsequence search
- Pinned and frecency-based ranking
- Global `Option+Space` launcher shortcut
- Menu bar controls for showing, launching at login, and quitting
- Arrow-key navigation, Return to open, `Command+P` to pin, and Escape to hide
- Atomic local history storage with damaged-file recovery
- Apple Silicon and Intel `.app`/`.dmg` release jobs
- Immutable Cloudflare R2 release objects with a checksummed `latest.json`

## Requirements

- macOS 13 or newer
- Rust 1.94.1 for local development

DuckGooKey currently targets macOS for end-user use. The domain and UI layers
compile cross-platform, while application discovery, item launching, and
launch-at-login return explicit unsupported errors outside macOS.

## Run locally

```bash
cargo run --locked
```

On first launch, DuckGooKey opens in the center of the active display and
indexes installed applications. It continues running from the menu bar when
the launcher window is hidden.

If `Option+Space` is already registered by another application, DuckGooKey
continues running and reports the shortcut conflict in the launcher instead of
crashing.

## Keyboard controls

| Input | Action |
| --- | --- |
| `Option+Space` | Show or hide DuckGooKey globally |
| `Up` / `Down` | Move through results |
| `Return` | Open the selected result |
| `Command+P` | Pin or unpin the selected result |
| `Escape` | Hide the launcher |

Use **Refresh Applications** from the result list after installing or removing
an application.

## Local data

DuckGooKey stores only local ranking state: item identifiers, pin state, launch
count, and the latest launch timestamp. On macOS, the store is located under:

```text
~/Library/Application Support/com.DuckGoo.DuckGooKey/state.json
```

Writes use a complete temporary file followed by an atomic rename. If JSON is
damaged, DuckGooKey preserves it beside the store with a `.corrupt-*` suffix
and starts with clean ranking state.

Enabling **Launch at Login** creates only:

```text
~/Library/LaunchAgents/com.duckgoo.key.plist
```

Login startup uses the internal `--hidden` argument, so it starts the menu bar
launcher without interrupting sign-in. Disabling the option removes that exact
file.

## Validate a change

```bash
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

## Release

Push a SemVer tag matching `Cargo.toml`, such as `v0.1.0`. The release workflow
builds separate Apple Silicon and Intel packages, verifies their architecture
and checksums, and always retains GitHub Actions artifacts.

Cloudflare R2 publishing is enabled only when its repository variables and
secrets are configured. Versioned objects are never overwritten;
`latest.json` is updated last so it cannot advertise a partial release.

See [docs/releasing.md](docs/releasing.md) for signing, notarization, R2
configuration, object layout, and local script usage.

## Architecture

```text
Iced launcher UI
  ├── catalog: search, ranking, launch actions
  ├── store: atomic persisted usage state
  ├── integrations: global shortcut and menu bar events
  └── platform
      ├── macOS: bundle discovery, /usr/bin/open, LaunchAgent
      └── fallback: explicit unsupported results
```

The catalog owns launch actions as typed data. Only the platform boundary
executes them, and application paths are passed directly to `/usr/bin/open`
without a shell.

## Current non-goals

The initial release does not include plugins, clipboard history, file-system
search, an auto-updater, or Windows/Linux end-user support. Those should be
added only after real launcher usage validates the core interaction and
ranking behavior.
