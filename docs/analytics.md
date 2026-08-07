# Anonymous release metrics

DuckGooKey uses Cloudflare only for release metrics. The product does not use
PostHog, Google Analytics, session replay, or an internal dashboard.

## What is recorded

The website records `download_requested` before redirecting a user to a signed
DMG. Before the app records anything, its first-run welcome screen explains the
following required anonymous usage statistics and asks the user to select
**Get Started**. The setting is then shown as active in Settings;
there is no opt-out control.

- `first_launch`
- `active_daily` (at most once per UTC day)
- CPU architecture and DuckGooKey version
- a random installation UUID with no connection to an account or device name

Never send searches, selected application names, file paths, Quick Links,
clipboard entries, user names, email addresses, or IP addresses as event
properties. The random installation UUID is a persistent pseudonymous
identifier used only to count distinct active installations; it is not an
account, hardware serial number, or device name.

## Cloudflare setup

1. In the `duckgoo-net` Pages project, open **Settings → Bindings**.
2. Add an **Analytics Engine** binding named `DUCKGOOKEY_ANALYTICS` using the
   dataset `duckgookey_events`.
3. Redeploy the Pages project.
4. Create a Cloudflare API token scoped to the DuckGooNe account with only
   **Account Analytics: Read**.
5. Configure this Mac once:

   ```bash
   mise run analytics-configure
   ```

The account ID and read-only API token are stored in the default macOS Keychain,
not in the repository or shell configuration.

## On-demand reports

```bash
mise run analytics-report -- daily
mise run analytics-report -- weekly
mise run analytics-report -- monthly
```

Each report prints aggregate event totals by platform/version and the number of
distinct anonymous installations that sent `active_daily` in the selected
period. It is an active-installation estimate, not a claim of every installed
copy. Existing installations see the first-run disclosure once after upgrading
to this policy and do not send new events until they acknowledge it.
