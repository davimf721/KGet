# KGet browser extension

Send links, images, videos and (optionally) every browser download to KGet.
Works in Chrome, Chromium, Edge, Brave and Firefox (Manifest V3, plain
JS/HTML/CSS, no build step). The extension talks to KGet through
**native messaging**: the browser starts `kget native-host`, which forwards
requests to the KGet daemon (started automatically). Downloads keep running
after the browser closes.

## Features

- **Context menu** “Download with KGet” on links, images, video, audio and
  the page itself.
- **Toolbar popup** with live progress, speed and ETA, plus Pause, Resume,
  Cancel, Open, Show in folder and Remove. You can also paste a link there.
- **Intercept browser downloads** (off by default): matching downloads are
  cancelled in the browser and handed to KGet. You can filter them by file
  extension, minimum size and domain (include and exclude lists). If KGet is
  unavailable, the browser downloads the file itself.
- **Send cookies** (off by default): sends the site's cookies for that one
  URL as a `Cookie` header, so downloads that need a login work.
- **Turbo mode** default: parallel connections for HTTP downloads.
- A clear “KGet is not connected” screen with the command to run.

Files are saved in KGet's download directory (`daemon.download_dir` in
KGet's config, default `~/Downloads`). The extension can only suggest a file
*name*, never a folder.

## 1. Register the native host

Install KGet (1.8 or later), then run:

```bash
kget native-host install                    # every installed browser
kget native-host install --browser firefox  # or: chrome, chromium, edge, brave, all
```

This writes `com.kget.native_host.json` for each browser, plus a small
launcher script that runs `kget native-host`. It prints every path it
writes. Run it again after moving the `kget` binary. To remove everything:

```bash
kget native-host uninstall
```

If you load the extension with a different ID (for example a Web Store
build, or an unpacked copy without the `key` field), allow that ID:

```bash
kget native-host install --extension-id <id-from-chrome://extensions>
```

## 2. Load the extension

**Chrome, Edge, Brave and Chromium**

1. Open `chrome://extensions` (or `edge://extensions`, `brave://extensions`).
2. Turn on **Developer mode**.
3. Click **Load unpacked** and pick this `browser-extension/` folder.

The `key` in `manifest.json` pins the unpacked extension ID to
`eccghfaajdkmbbbccabhfnhhbllfmlkg`, which `kget native-host install` allows
by default.

**Firefox**

1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on…** and pick `browser-extension/manifest.json`.

The add-on ID is `kget@davimf721.github.io` (allowed by default). Temporary
add-ons are removed when Firefox quits. Firefox 128 or later is required.

Then open the extension's options page to turn on interception, cookies or
turbo mode. The browser asks for the extra permission when you turn one on.

## Permissions

| Permission | Why | When |
|------------|-----|------|
| `nativeMessaging` | Talk to `kget native-host` | always |
| `contextMenus` | “Download with KGet” | always |
| `storage` | Options | always |
| `downloads` | Intercept browser downloads | optional, asked when you turn interception on |
| `cookies` + access to sites | Send cookies with a download | optional, asked when you turn “send cookies” on |

## Security model

- The browser only starts the host for the extension IDs listed in the host
  manifest. The host also checks the caller it receives
  (`chrome-extension://<id>/` or the Firefox add-on ID) against
  `allowed_origins.json` and exits if it doesn't match.
- Only `http`, `https`, `ftp`, `ftps` and `magnet` links are accepted.
- Only a file name is forwarded. The daemon keeps every file inside its
  download directory.
- `Cookie` and `Authorization` headers are stripped unless you turn on
  “send cookies”. When it's on, the extension reads cookies only for the URL
  being downloaded.
- “Open” refuses programs and scripts. Use “Show in folder” for those.

## Packaging (not published)

```bash
./browser-extension/package.sh
```

This builds `dist/kget-extension-chrome-<version>.zip` (without the
development `key`, as the Chrome Web Store requires) and
`dist/kget-extension-firefox-<version>.zip` (for addons.mozilla.org). The
script uploads nothing.

After the Web Store assigns an ID, register it with
`kget native-host install --extension-id <store id>`. Alternatively, copy
the store item's public key into `manifest.json` so unpacked builds share
that ID.

## Safari

Safari Web Extensions must ship inside a macOS app. Apple's converter
creates an Xcode project from this folder:

```bash
xcrun safari-web-extension-converter browser-extension \
  --project-location /tmp/kget-safari --app-name "KGet Extension" \
  --bundle-identifier com.kget.safari-extension --macos-only --no-open
```

Safari has no `nativeMessaging` to an arbitrary host. Instead, the
extension's `runtime.sendNativeMessage` reaches the containing app's
`SafariWebExtensionHandler`. To support Safari, that handler would forward
messages to the daemon. It can do this by spawning `kget native-host` or by
speaking the daemon's JSON-RPC socket directly. A future version could add
this handler to the existing macOS app in `macos-app/`. Run the converter
into a separate directory so it doesn't modify `macos-app/`.

## Native message protocol

Each request is `{"id": <any>, "type": "...", ...}` and gets one reply,
`{"id", "type", "ok": true, "result"}` or
`{"id", "type", "ok": false, "error": {"code", "message"}}`. Job IDs travel
as `job`.

| Type | Fields |
|------|--------|
| `ping`, `version` | – |
| `download` | `url`, `filename?`, `referrer?`, `headers?`, `turbo?`, `send_cookies?`, `cookies?`, `tags?` |
| `list` | `limit?` |
| `get`, `pause`, `resume`, `cancel`, `remove`, `open`, `reveal` | `job` |
| `subscribe` | `ids?`, `progress?` (afterwards the host pushes `{"type":"event","event":<job event>}`) |

The full reference is in `src/native_host/mod.rs`.

## Tests

```bash
node --test browser-extension/tests/   # or: bun test ./browser-extension/tests/lib.test.js
cargo test --test native_host_tests
```
