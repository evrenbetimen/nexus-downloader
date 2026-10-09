# Installing the browser extension

1. Install NexusDL and **open it once**. On every launch the app registers
   itself with the browsers it finds, so the extension can reach it.
2. Download the extension for your browser from the
   [release page](https://github.com/evrenbetimen/nexus-downloader/releases)
   and unzip it into a folder you'll keep.

## Chrome, Edge, Brave, Vivaldi

1. Open `chrome://extensions` (Edge: `edge://extensions`).
2. Turn on **Developer mode**.
3. Click **Load unpacked** and pick the unzipped
   `NexusDL-extension-chromium-<version>` folder.
4. Click the NexusDL toolbar button: it should say "Connected to NexusDL".

## Firefox

Until the add-on is signed by Mozilla, Firefox only loads it until the next
restart:

1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on…** and pick `manifest.json` inside the
   unzipped `NexusDL-extension-firefox-<version>` folder.
3. When asked, allow access to all websites (needed to read the cookies of
   the site a download comes from).

## If the popup says "NexusDL app not found"

- Open NexusDL once and reopen the popup.
- Linux: the app registers in `~/.config/<browser>/NativeMessagingHosts`
  (and `~/.mozilla/native-messaging-hosts`) only for browsers whose profile
  folder exists. Start the browser once before opening NexusDL. Snap and
  Flatpak browsers can't start host programs outside their sandbox.
- Windows: the registration lives under
  `HKEY_CURRENT_USER\Software\<browser vendor>\NativeMessagingHosts\dev.nexusdl.host`.
