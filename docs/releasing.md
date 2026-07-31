# Releasing DuckGooKey

DuckGooKey releases are built by `.github/workflows/release.yml`. The workflow
produces native Apple Silicon and Intel macOS packages, always uploads them as
GitHub Actions artifacts, and optionally publishes them to Cloudflare R2.

## Release contract

- A release tag must be SemVer prefixed with `v`, such as `v0.1.0`.
- The tag without `v` must exactly match `[package].version` in `Cargo.toml`.
- Both `aarch64-apple-darwin` and `x86_64-apple-darwin` are built with
  `cargo build --release --locked`.
- `cargo-packager` 0.11.8 creates a `.app` and `.dmg` for each architecture.
- The `.app` is preserved as a resource-safe `.app.zip` for download.
- Every architecture artifact includes a `SHA256SUMS` file.
- The release manifest advertises the DMG for each platform.
- The packaged app sets `LSUIElement` through `backgroundApp`, so the
  menu-bar launcher does not add a Dock icon.

The manifest publication date is derived from the annotated tag date, or from
the release commit date for a lightweight tag. This keeps publication metadata
deterministic across reruns; immutable uploads still reject any rebuilt
artifact whose bytes differ from an object already stored for that version.

The project currently has no application icon. The release configuration
therefore omits `icons`; cargo-packager creates valid app and DMG bundles
without a custom icon. Adding an icon later is a separate product change and
is not required to ship.

## Starting a release

### Automatic tag release

1. Update `Cargo.toml` to the intended version and merge the change.
2. Create and push the matching tag:

   ```bash
   git tag -a v0.1.0 -m "DuckGooKey v0.1.0"
   git push origin v0.1.0
   ```

Any pushed `v*` tag starts the workflow. Invalid tags or version mismatches
fail before compilation.

### Manual release

Open **Actions → Release → Run workflow**, enter an existing matching tag, and
choose whether R2 publishing is requested. The workflow checks out that tag;
it never releases an arbitrary untagged branch.

## Apple signing and notarization

Signing and notarization are optional gates. With no Apple secrets, the
workflow succeeds and uploads unsigned GitHub artifacts. Partial groups are
reported and skipped; a complete signing group enables signing, and a complete
notarization group additionally enables notarization and stapling.

Configure these GitHub Actions secrets to sign:

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | Full identity, for example `Developer ID Application: Company (TEAMID)` |

Configure all of these as well to notarize:

| Secret | Purpose |
| --- | --- |
| `APPLE_ID` | Apple developer account email |
| `APPLE_PASSWORD` | Apple app-specific password |
| `APPLE_TEAM_ID` | Apple Developer Team ID |

Encode the certificate without writing it to logs:

```bash
base64 < "Developer ID Application.p12" | pbcopy
```

`cargo-packager` imports the certificate into a temporary keychain, signs the
app and DMG, submits the app through `notarytool`, and staples the accepted
ticket. The workflow then verifies the code signature and stapled ticket.

## Cloudflare R2 configuration

R2 publishing needs two secrets:

| Secret | Purpose |
| --- | --- |
| `CLOUDFLARE_R2_ACCESS_KEY_ID` | R2 S3 API token access key |
| `CLOUDFLARE_R2_SECRET_ACCESS_KEY` | R2 S3 API token secret key |

It also needs three non-sensitive repository variables:

| Variable | Example |
| --- | --- |
| `CLOUDFLARE_R2_BUCKET` | `duckgookey-releases` |
| `CLOUDFLARE_R2_ENDPOINT` | `https://ACCOUNT_ID.r2.cloudflarestorage.com` |
| `CLOUDFLARE_R2_PUBLIC_BASE_URL` | `https://downloads.example.com` |

Scope the R2 token to the release bucket with object read/write and bucket list
permissions. Do not grant account-wide administration. The public base URL must
serve that bucket root through an R2 custom domain (or another public R2 URL).

If any R2 credential or variable is absent, the publish job exits successfully
without contacting R2. GitHub artifacts remain available.

## R2 object layout and immutability

For version `0.1.0`, the workflow writes:

```text
releases/v0.1.0/
  DuckGooKey-0.1.0-macos-aarch64.dmg
  DuckGooKey-0.1.0-macos-aarch64.app.zip
  DuckGooKey-0.1.0-macos-aarch64.SHA256SUMS
  DuckGooKey-0.1.0-macos-x86_64.dmg
  DuckGooKey-0.1.0-macos-x86_64.app.zip
  DuckGooKey-0.1.0-macos-x86_64.SHA256SUMS
  release.json
latest.json
```

Versioned objects use a one-year immutable cache policy. Re-running a release
accepts an existing object only when its stored SHA-256 metadata and size match;
otherwise publishing fails instead of overwriting history. New versioned
objects are created with `If-None-Match: *`, so concurrent runs cannot overwrite
one another between the existence check and upload. `latest.json` is the only
mutable object and is written last, after every immutable object has been
uploaded and verified.

The manifest schema is:

```json
{
  "version": "0.1.0",
  "pub_date": "2026-07-31T12:00:00Z",
  "platforms": {
    "macos-aarch64": {
      "url": "https://downloads.example.com/releases/v0.1.0/DuckGooKey-0.1.0-macos-aarch64.dmg",
      "sha256": "64 lowercase hexadecimal characters"
    },
    "macos-x86_64": {
      "url": "https://downloads.example.com/releases/v0.1.0/DuckGooKey-0.1.0-macos-x86_64.dmg",
      "sha256": "64 lowercase hexadecimal characters"
    }
  }
}
```

## Local script checks

Build a manifest from already-staged artifacts:

```bash
./scripts/build-release-manifest.sh \
  --version 0.1.0 \
  --pub-date 2026-07-31T12:00:00Z \
  --base-url https://downloads.example.com \
  --artifacts-dir "/path/to/release assets" \
  --output "/path/to/release assets/latest.json"
```

Publish those artifacts using AWS CLI v2 and temporary environment credentials:

```bash
AWS_ACCESS_KEY_ID=... \
AWS_SECRET_ACCESS_KEY=... \
AWS_DEFAULT_REGION=auto \
./scripts/publish-r2.sh \
  --version 0.1.0 \
  --bucket duckgookey-releases \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --base-url https://downloads.example.com \
  --artifacts-dir "/path/to/release assets" \
  --manifest "/path/to/release assets/latest.json"
```

Never place credential values in command arguments or committed files.
