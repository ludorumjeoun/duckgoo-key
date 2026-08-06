# DuckGooKey

DuckGooKey is a fast, keyboard-first macOS launcher written in Rust. It is a
clean-room implementation inspired by the interaction model of
[SuperCmd](https://github.com/SuperCmdLabs/SuperCmd), with an independent Rust
codebase and no copied source or assets.

The production scope stays deliberately focused: launch applications, search
local files and the web, run a bounded set of native commands, and keep useful
text and links close at hand. Releases remain native macOS packages with a
reproducible Cloudflare R2 delivery path.

## What works

- Native Iced UI with no webview or JavaScript runtime
- Application discovery in standard macOS application folders, including
  user-facing CoreServices apps such as Finder and Keychain Access
- Installed application icons decoded directly from each macOS app bundle
- Case-insensitive prefix, substring, and subsequence search
- Pinned and frecency-based ranking
- Explicit `=` calculator results and a DuckDuckGo web-search fallback
- User-managed HTTP/HTTPS Quick Links
- Spotlight file and folder search within the current user's home directory
- Opt-in, local, text-only Clipboard History
- Typed macOS system commands with confirmation for destructive actions
- Configurable global launcher shortcut (`Option+Space` by default)
- Menu bar controls for showing, launching at login, and quitting
- In-app settings for the shortcut and launch-at-login behavior
- Selectable search input source with automatic restoration when the launcher hides
- Automatic application rediscovery every 15 seconds
- Context-aware keyboard actions for opening, copying, revealing in Finder,
  pinning, and deleting Clipboard History entries
- Atomic local history storage with damaged-file recovery
- Apple Silicon and Intel `.app`/`.dmg` release jobs
- Developer ID-signed, Apple-notarized public releases on immutable Cloudflare
  R2 objects with a checksummed `latest.json`
- User-approved automatic updates with SHA-256 and Apple-signing verification
- Separately isolated self-signed packages for private testers

## Requirements

- macOS 13 or newer
- [mise](https://mise.jdx.dev/) 2026.7.13 or newer for the pinned project
  toolchain

DuckGooKey currently targets macOS for end-user use. The domain and UI layers
compile cross-platform, while native integrations such as application and
Spotlight discovery, clipboard access, system commands, item launching, and
launch-at-login return explicit unsupported errors outside macOS.

`mise.toml` pins Rust, cargo-packager, Python, uv, jq, and AWS CLI. `mise.lock`
locks downloadable artifacts for macOS ARM64, macOS x86_64, and Linux x86_64.
After cloning the repository, prepare the toolchain once:

```bash
mise trust
mise install --locked
```

## Run locally

```bash
mise run dev
```

This uses the project-pinned Rust toolchain, compiles, and launches the debug
build directly. Creating an `.app` or DMG is not necessary for normal
development. On first launch, DuckGooKey opens in the center of the active
display and indexes installed applications. It continues running from the menu
bar when the launcher window is hidden.

If the configured shortcut is already registered by another application,
DuckGooKey continues running and reports the conflict in the launcher instead
of crashing. Open DuckGooKey from its menu bar item and choose the gear button
to select another shortcut.

## Build a local installer

```bash
mise run package
```

The task uses the mise-managed cargo-packager, builds the release app and DMG,
and opens `target/release/packages` in Finder. Open the DMG and drag DuckGooKey
to Applications to install it. These local packages are unsigned development
outputs, not files for user distribution. Build outputs remain ignored by Git.

For a build without opening Finder:

```bash
mise run package -- --no-open
```

The source brand artwork is normalized into a transparent 1024px PNG, an
in-app PNG, and a macOS ICNS file with:

```bash
mise run icons -- /path/to/source-image.png
```

The icon task runs with the pinned Python and uv versions and an exact Pillow
version, without modifying the project interpreter environment.

## Keyboard controls

| Input | Action |
| --- | --- |
| Configured shortcut (`Option+Space` by default) | Show or hide DuckGooKey globally |
| `Up` / `Down` | Move through results |
| `Return` | Run the selected result's primary action: open, copy, enter a search mode, or review a command |
| `Command+Return` | Show the selected application, file, or folder in Finder |
| `Command+C` | Copy the selected path, URL, calculator result, or Clipboard History text, then hide |
| `Command+P` | Pin or unpin a stable, pinnable result |
| `Command+D` | Delete the selected entry while in Clipboard History |
| `Command+Return` in the Quick Link editor | Save the link |
| `Return` / `Escape` on a confirmation screen | Confirm or cancel the reviewed action |
| `Escape` | Leave File Search or Clipboard History; otherwise hide or go back |

DuckGooKey automatically rescans installed applications every 15 seconds. Use
**Refresh Applications** from the result list or **Scan now** in Settings when
you want an immediate refresh.

## Search and commands

### Calculator and web search

Prefix a calculation with `=` to keep ordinary launcher searches free from
calculator false positives:

```text
= 2 + 3 * 4
= 1 hour to minutes
```

The result appears as a non-pinnable action. Press `Return` or `Command+C` to
copy it. Any non-empty query in the main launcher can also produce a
**Search DuckDuckGo for ...** fallback; opening it sends the URL-encoded query
to DuckDuckGo in the default browser.

### Quick Links

Choose **Manage Quick Links** from the launcher or **Manage** in Settings to
add, edit, and remove saved websites. A Quick Link has a title and a validated
HTTP or HTTPS URL. It behaves like any other stable result: `Return` opens it,
`Command+C` copies its URL, and `Command+P` can pin it. Deleting a Quick Link
requires confirmation.

### File search

Choose **Search Files** to enter a dedicated Spotlight-backed search mode, then
type at least two characters. DuckGooKey searches file and folder names within
the current user's home directory without building a second on-disk index.
`Return` opens a result, `Command+Return` reveals and selects it in Finder, and
`Command+C` copies its absolute path. Press `Escape` to return to all launcher
results.

### System commands

The searchable command catalog includes:

- **Open System Settings**
- **Sleep**
- **Lock Screen**
- **Toggle System Appearance**
- **Empty Trash**
- **Log Out**
- **Restart**
- **Shut Down**

Empty Trash, Log Out, Restart, and Shut Down always open a confirmation screen
before execution. The command set is typed and fixed; query text is never
passed to a shell.

### Clipboard History

Clipboard History is disabled by default. Enable it explicitly in Settings;
only new plain-text copies made after enabling are recorded. Entries stay in
DuckGooKey's local state file, are deduplicated, and are limited to 100 entries
and 64 KiB per entry. Blank text and larger payloads are ignored.
Clipboard values marked concealed, transient, or automatically generated—and
common password-manager pasteboard types—are skipped instead of being read or
persisted.

Choose **Clipboard History** to filter saved text. `Return` or `Command+C`
restores the selected entry to the system clipboard and hides DuckGooKey; the
feature does not synthesize a paste keystroke or request Accessibility access.
`Command+D` removes one entry. **Clear** in Settings removes all entries
after confirmation. Disabling capture stops new entries but does not silently
delete existing history.

## Local data

DuckGooKey stores only local launcher state: the configured shortcut, preferred
search input-source identifier, item identifiers, pin state, launch history,
Quick Links, the Clipboard History opt-in setting, and any retained text
entries. On macOS, the store is located under:

```text
~/Library/Application Support/com.DuckGoo.DuckGooKey/state.json
```

Writes use a complete temporary file followed by an atomic rename. If JSON is
damaged, DuckGooKey preserves it beside the store with a `.corrupt-*` suffix
and starts with clean state. Clipboard text and Quick Links are stored locally
as JSON; they are not encrypted, synchronized, or uploaded by DuckGooKey.

Enabling **Launch at Login** creates only:

```text
~/Library/LaunchAgents/com.duckgoo.key.plist
```

Login startup uses the internal `--hidden` argument, so it starts the menu bar
launcher without interrupting sign-in. Disabling the option removes that exact
file.

## Validate a change

```bash
mise run check
```

The main development tasks are:

| Task | Purpose |
| --- | --- |
| `mise run dev` | Compile and run the local debug build without packaging |
| `mise run check` | Check formatting, run all tests and Clippy, and make a release build |
| `mise run package` | Build the release `.app` and DMG, then open the output folder |
| `mise run package -- --no-open` | Build the same packages without opening Finder |
| `mise run release-configure` | Store local release settings and R2 credentials securely in Keychain |
| `mise run release-local -- --tag v0.1.0` | Build the signed, notarized local public release without publishing |

## Release

Push a SemVer tag matching `Cargo.toml`, such as `v0.1.0`. The release workflow
builds separate Apple Silicon and Intel packages, verifies their architecture
and checksums, and always retains GitHub Actions artifacts. Public R2
publication requires complete Developer ID signing and Apple notarization;
missing or partial Apple/R2 configuration fails instead of publishing an
unsigned fallback.

For pre-account private testing, the separate **Private Release** workflow uses
a pinned self-signed certificate, includes the public certificate and
fingerprint, and keeps the result in short-lived GitHub artifacts rather than
the public R2 bucket. Versioned R2 objects are never overwritten; `latest.json`
is updated last so it cannot advertise a partial release. Starting with the
first version that contains the updater, installed apps check this manifest
automatically and offer a verified in-place update.

See [docs/releasing.md](docs/releasing.md) for signing, notarization, R2
configuration, object layout, and local script usage.

## Anonymous release metrics

Cloudflare-only anonymous release tracking and the on-demand report commands
are documented in [docs/analytics.md](docs/analytics.md).

## Architecture

```text
Iced launcher UI
  ├── catalog: search, ranking, and typed actions
  ├── calculator / web_search: bounded dynamic results
  ├── commands: fixed macOS system command catalog
  ├── quick_link: validated persisted web shortcuts
  ├── clipboard_history: bounded persisted text entries
  ├── app_icon: bounded ICNS decoding for installed applications
  ├── shortcut: validated, persisted global shortcut model
  ├── store: atomic persisted settings and usage state
  ├── integrations: global shortcut and menu bar events
  ├── telemetry: opt-in anonymous release signals only
  └── platform
      ├── macOS: bundle/Spotlight discovery, clipboard, system commands,
      │          /usr/bin/open, and LaunchAgent
      └── fallback: explicit unsupported results
```

The catalog owns launch actions as typed data. Only the platform boundary
executes them, and application paths are passed directly to `/usr/bin/open`
without a shell.

## Current non-goals

The current release does not include plugins, arbitrary user script commands,
Quick Link query templates, non-text clipboard capture, automatic paste,
an auto-updater, or Windows/Linux end-user support. Those should be added only
after real launcher usage validates the current interaction and privacy model.
