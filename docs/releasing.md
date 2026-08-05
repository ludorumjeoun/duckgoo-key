# Releasing DuckGooKey

DuckGooKey has two intentionally separate distribution channels:

- The public channel builds Apple Silicon and Intel packages with either
  `mise run release-local` or `.github/workflows/release.yml`. Both tracks call
  `scripts/package-macos-release-arch.sh`, produce the same artifact contract,
  and can publish to Cloudflare R2 only after Developer ID signing and Apple
  notarization succeed.
- `.github/workflows/private-release.yml` builds private-test packages with a
  pinned self-signed certificate. It uploads short-lived GitHub Actions
  artifacts only and never publishes to the public R2 bucket.

## Release contract

- A release tag must be SemVer prefixed with `v`, such as `v0.1.0`.
- The tag without `v` must exactly match `[package].version` in `Cargo.toml`.
- `mise.toml` and `mise.lock` provide the exact Rust toolchain, macOS targets,
  cargo-packager, Python, jq, and AWS CLI versions used by the workflow.
- Both `aarch64-apple-darwin` and `x86_64-apple-darwin` are built with
  `cargo build --release --locked`.
- Local and GitHub public builds use the same per-architecture packaging,
  notarization, signature verification, and staging script.
- The mise-managed cargo-packager creates a `.app` and `.dmg` for each
  architecture.
- The `.app` is preserved as a resource-safe `.app.zip` for download.
- Every architecture artifact includes a `SHA256SUMS` file.
- The release manifest advertises the DMG for each platform.
- The packaged app sets `LSUIElement` through `backgroundApp`, so the
  menu-bar launcher does not add a Dock icon.
- Public R2 artifacts must be signed with `Developer ID Application`, have the
  hardened runtime enabled, and have accepted Apple notarization tickets
  stapled to both the app and final DMG.
- A public publication request fails when any Apple or R2 setting is missing or
  only partially configured. It never silently falls back to an unsigned file.
- Unsigned output is permitted only when a manual **Release** run explicitly
  disables R2. Its artifact name contains `UNSIGNED-NOT-FOR-DISTRIBUTION`.
- Private-signed output has `Private` in every filename, includes the public
  certificate and its fingerprint, and is never treated as a public release.

The manifest publication date is derived from the annotated tag date, or from
the release commit date for a lightweight tag. This keeps publication metadata
deterministic across reruns; immutable uploads still reject any rebuilt
artifact whose bytes differ from an object already stored for that version.

The release configuration packages `assets/icons/DuckGooKey.icns` into both
the application and disk image. Rebuild the icon assets through the pinned
toolchain with `mise run icons -- /path/to/source-image.png`.

## Starting a release

The local and GitHub public tracks are alternatives for publishing a version.
Do not start both publishers for the same tag. Signed and notarized packages
contain timestamps and tickets, so two builds of one version are not expected
to be byte-identical.

### Create the release tag

1. Update `Cargo.toml` to the intended version and merge the change.
2. Create and push the matching tag:

   ```bash
   git tag -a v0.1.0 -m "DuckGooKey v0.1.0"
   git push origin v0.1.0
   ```

Any pushed `v*` tag starts the public workflow in **build and verify only**
mode. It does not mutate R2. Invalid tags, version mismatches, incomplete
Developer ID configuration, or failed notarization fail the run.

### Local public release

The local track performs the full two-architecture release on a Mac. It
requires a clean working tree, `HEAD` at the matching tag, a Developer ID
Application identity, and Apple notarization credentials. The default command
builds, signs, notarizes, verifies, writes `latest.json`, and opens the artifact
folder without changing R2:

```bash
export APPLE_SIGNING_IDENTITY='Developer ID Application: Legal Name (TEAMID)'
export APPLE_TEAM_ID='TEAMID'
export APPLE_KEYCHAIN_PROFILE='duckgookey-notary'

mise run release-local -- --tag v0.1.0
```

Add `--publish-r2` only when this machine should publish the release:

```bash
export CLOUDFLARE_R2_ACCESS_KEY_ID='...'
export CLOUDFLARE_R2_SECRET_ACCESS_KEY='...'
export CLOUDFLARE_R2_BUCKET='duckgookey-releases'
export CLOUDFLARE_R2_ENDPOINT='https://ACCOUNT_ID.r2.cloudflarestorage.com'
export CLOUDFLARE_R2_PUBLIC_BASE_URL='https://updates.key.duckgoo.net'

mise run release-local -- --tag v0.1.0 --publish-r2
```

Publication additionally requires the same tag on `origin` at the release
commit. If R2 or CDN verification fails after packaging, the exact signed bytes
and their `public-release.json` receipt remain in
`target/release/public/v0.1.0`. Retry those bytes instead of rebuilding:

```bash
mise run release-local -- \
  --tag v0.1.0 \
  --publish-r2 \
  --reuse-artifacts
```

### GitHub public release

Open **Actions → Release → Run workflow**, enter an existing matching tag, and
choose whether R2 publishing is requested. This is the second full public
release track. The workflow checks out that tag; it never releases an arbitrary
untagged branch. Enabling R2 is an explicit publication action. Disabling R2
with no Apple secrets creates clearly labelled unsigned diagnostic artifacts.
Signing and notarization secret groups must always be either both complete or
both absent.

### Private release

Open **Actions → Private Release → Run workflow** and enter an existing matching
tag. This workflow requires the protected `private-release` environment and a
stable private signing identity. It produces 14-day GitHub Actions artifacts;
there is deliberately no R2 publishing job.

## Apple Developer account

An individual can join the paid Apple Developer Program without a registered
company or D-U-N-S Number. This is the quickest path for DuckGooKey. The annual
fee is USD 99, subject to local pricing, and the personal legal name is used as
the public developer identity. A free Apple Account supports local development
but does not provide the Developer ID certificate and notarization needed for
normal distribution outside the Mac App Store.

Apple's current references are:

- [Program enrollment](https://developer.apple.com/help/account/membership/program-enrollment)
- [Membership comparison](https://developer.apple.com/support/compare-memberships/)
- [Developer ID](https://developer.apple.com/support/developer-id/)
- [D-U-N-S requirements](https://developer.apple.com/help/account/membership/D-U-N-S/)
- [Account and membership updates](https://developer.apple.com/help/account/membership/updating-your-account-information/)

An organization enrollment requires a legal entity and D-U-N-S Number. Apple
allows an eligible founder or cofounder to request conversion from an
individual membership later, after supplying the organization information and
supporting documents.

## Public Developer ID signing and notarization

After joining the paid program, create a **Developer ID Application**
certificate through the Apple Developer account or Xcode and export the
identity with its private key as a password-protected `.p12`.

Configure these GitHub Actions secrets to sign:

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | Full identity, for example `Developer ID Application: Legal Name (TEAMID)` |

Configure all of these as well to notarize:

| Secret | Purpose |
| --- | --- |
| `APPLE_ID` | Apple developer account email |
| `APPLE_PASSWORD` | Apple app-specific password used by `notarytool` |
| `APPLE_TEAM_ID` | Apple Developer Team ID |

Encode the certificate without writing it to logs:

```bash
base64 < "Developer ID Application.p12" | pbcopy
```

For the local track, install the Developer ID Application certificate and its
private key in the login keychain. Confirm the full identity string with:

```bash
security find-identity -v -p codesigning
```

Store notarization credentials in Keychain once. Running `store-credentials`
without credential flags prompts interactively, so the app-specific password
does not enter shell history:

```bash
xcrun notarytool store-credentials "duckgookey-notary"
```

The default local release uses that profile through
`APPLE_KEYCHAIN_PROFILE`. As an alternative, pass `--notary-auth apple-id` and
provide `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` in the environment.
The local command can also use a base64 P12 by passing
`--signing-source p12-env`, but the installed keychain identity is preferred.

In GitHub, `cargo-packager` imports the P12 into a temporary keychain. Locally,
it uses the selected installed identity. In both tracks it enables the hardened
runtime, signs the app and DMG, submits the app through `notarytool`, and staples
the accepted app ticket. The shared packaging script submits the final DMG
separately, requires an `Accepted` result, staples that DMG ticket, and checks:

- strict app and DMG code-signature validity;
- configured signing authority and Team ID;
- app and DMG stapled-ticket validity;
- DMG filesystem integrity; and
- Gatekeeper assessment for both the app and DMG.

The R2 job can only download artifacts whose Actions name ends in
`public-notarized`.

## Private self-signed distribution

Private signing provides a stable integrity identity before the paid Apple
account is ready. It is not a replacement for Developer ID. Generate the
identity once, outside the repository:

```bash
./scripts/generate-private-signing-identity.sh \
  "$HOME/Documents/DuckGooKey-private-signing"
```

The script prompts for the P12 password without echoing it. For non-interactive
automation, provide `DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD` through a secret
environment rather than putting the value in a shell command or script.

The generator retains only an encrypted P12, public PEM/DER certificates, and
fingerprints. It removes the standalone private-key file before publishing the
output directory. Back up the P12 and password securely; reusing this stable
identity prevents testers from having to trust a new certificate every build.

Create a GitHub environment named `private-release`, ideally with required
reviewers, and configure:

| Kind | Name | Purpose |
| --- | --- | --- |
| Secret | `PRIVATE_SIGNING_CERTIFICATE` | Base64-encoded private signing `.p12` |
| Secret | `PRIVATE_SIGNING_CERTIFICATE_PASSWORD` | P12 export password |
| Variable | `PRIVATE_SIGNING_CERT_SHA256` | DER certificate SHA-256 from `signing-metadata.env` |

Encode the private P12 without printing it:

```bash
base64 < "$HOME/Documents/DuckGooKey-private-signing/DuckGooKey-Private-Code-Signing.p12" | pbcopy
```

The private workflow decodes the P12 into runner-temporary storage, pins the
exact DER certificate SHA-256, imports it into a temporary keychain, and adds a
code-signing trust setting only while signing. It signs every Mach-O file, the
outer app, and a freshly rebuilt DMG with hardened runtime and no Apple
timestamp. It verifies both signatures while the trust is active, removes the
temporary trust and keychain, and uploads only:

- the signed private DMG and resource-safe app ZIP;
- SHA-256 checksums;
- the public `.cer` certificate; and
- a `SIGNING.txt` file containing the certificate fingerprint and validity.

The workflow rejects any staged `.p12`, private key, or keychain file.

### Tester trust model

A private certificate proves that two builds were signed by the same pinned
identity only after the tester independently verifies and trusts its SHA-256
fingerprint. It is not trusted by Apple and cannot be notarized. Internet-downloaded
files can therefore still show a Gatekeeper warning even after the certificate
is added in Keychain Access. Testers may need macOS's normal **Open** or
**Privacy & Security → Open Anyway** approval. Do not instruct testers to remove
quarantine attributes or disable Gatekeeper.

For a managed organization, distribute the public certificate and trust policy
through MDM. Never give testers the P12 or its password.

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
| `CLOUDFLARE_R2_PUBLIC_BASE_URL` | `https://updates.key.duckgoo.net` |

Scope the R2 token to the release bucket with object read/write and bucket list
permissions. Do not grant account-wide administration. Connect
`updates.key.duckgoo.net` from the bucket's **Settings → Custom Domains** in
Cloudflare R2. This can coexist with `duckgoo.net` on Cloudflare Pages; do not
manually point the hostname at an `r2.dev` URL.

Cloudflare references:

- [Public buckets and custom domains](https://developers.cloudflare.com/r2/buckets/public-buckets/)
- [AWS CLI with R2](https://developers.cloudflare.com/r2/get-started/cli/)
- [S3 API compatibility](https://developers.cloudflare.com/r2/api/s3/api/)

When R2 publication is requested, any missing credential or variable fails the
workflow. Tag pushes never publish. A GitHub manual run publishes only with
`publish_r2=true`, and the local track publishes only with `--publish-r2`.

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
one another between the existence check and upload.

Before advertising a release, the publisher downloads both DMGs and
`release.json` through the public custom domain and requires byte-for-byte
matches. `latest.json` is the only mutable object. It is written last using an
ETag condition, rejects SemVer rollback, and rejects different data for an
already published version. The publisher finally downloads `latest.json`
through the custom domain and compares it with the local manifest.

The manifest schema is:

```json
{
  "version": "0.1.0",
  "pub_date": "2026-07-31T12:00:00Z",
  "platforms": {
    "macos-aarch64": {
      "url": "https://updates.key.duckgoo.net/releases/v0.1.0/DuckGooKey-0.1.0-macos-aarch64.dmg",
      "sha256": "64 lowercase hexadecimal characters"
    },
    "macos-x86_64": {
      "url": "https://updates.key.duckgoo.net/releases/v0.1.0/DuckGooKey-0.1.0-macos-x86_64.dmg",
      "sha256": "64 lowercase hexadecimal characters"
    }
  }
}
```

## Lower-level release utilities

`mise run release-local` is the normal local entry point. The scripts below are
useful when validating or recovering already-staged public artifacts.

Build a manifest from already-staged artifacts:

```bash
./scripts/build-release-manifest.sh \
  --version 0.1.0 \
  --pub-date 2026-07-31T12:00:00Z \
  --base-url https://updates.key.duckgoo.net \
  --artifacts-dir "/path/to/release assets" \
  --output "/path/to/release assets/latest.json"
```

Publish already Developer ID-signed and Apple-notarized public artifacts using
AWS CLI v2 and temporary environment credentials:

```bash
AWS_ACCESS_KEY_ID=... \
AWS_SECRET_ACCESS_KEY=... \
AWS_DEFAULT_REGION=auto \
./scripts/publish-r2.sh \
  --version 0.1.0 \
  --bucket duckgookey-releases \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --base-url https://updates.key.duckgoo.net \
  --artifacts-dir "/path/to/release assets" \
  --manifest "/path/to/release assets/latest.json"
```

`publish-r2.sh` requires AWS CLI v2, Python 3, jq, and curl. It fails when either
AWS credential is absent; it never reports a requested publication as a
successful skip. Never place credential values in command arguments or
committed files. Private-signed artifacts must not be passed to this script or
uploaded beneath the public release object layout.
