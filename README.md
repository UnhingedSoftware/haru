# haru

A wallpaper picker and Workshop browser for Wallpaper Engine content — on
Linux, Windows and macOS, **without the Steam client running**.

貼る — "to put up".

```sh
haru                    # your wallpapers, and what is on each screen
haru workshop           # browse
haru --search miku      # browse, already searching
haru --item 884307090   # preview one, and edit its settings
```

| | |
|:-:|:-:|
| [![Library](docs/shots/library.jpg)](docs/shots/library.jpg)<br>**Library** — what is installed, and what is on each screen | [![Workshop](docs/shots/workshop.jpg)](docs/shots/workshop.jpg)<br>**Workshop** — 3.1M items, Steam's own filters, no Steam client |

[![Preview](docs/shots/preview.jpg)](docs/shots/preview.jpg)

**Preview** — the wallpaper rendered off-screen with its own properties beside
it, editable live. Nothing on your screens moves.

## What it does

- **Workshop** — search 3.1M items with Steam's own filters: tag groups, sort
  and period, content labels, date windows, numbered pages or endless scroll.
- **Library** — what is installed, what is on each screen, apply, delete, and
  the current wallpaper's own sliders and colours beside it.
- **Preview** — one wallpaper rendered off-screen with its settings editable
  live. Nothing on your screens moves.

## Requirements

Browsing needs nothing. **Installing** needs a Steam account that owns
Wallpaper Engine, once: `tapline login --qr`. **Applying and previewing** need
a renderer — [kirie](https://github.com/UnhingedSoftware/kirie), on Linux and
macOS.

Wallpapers install to `<steam library>/steamapps/workshop/content/431960/<id>`,
where kirie, Wallpaper Engine and Steam already look.

## Installing on macOS

Open `haru-macos-aarch64.dmg` and drag haru into Applications, as usual.

macOS will then refuse to open it:

> "haru.app" Not Opened — Apple could not verify "haru.app" is free of malware…

Nothing is wrong with the app. Releases are signed, but not *notarized* —
notarizing means paying Apple for a Developer Program membership and sending
every build to them for scanning. Until that happens macOS treats the app as
downloaded-and-unverified, and on macOS 15 the old right-click → Open way
around it is gone: the dialog only offers **Move to Trash** or **Done**. Press
**Done** — not Trash — and allow it one of these two ways.

**In System Settings.** Privacy & Security → scroll to Security → next to
"haru.app was blocked to protect your Mac", press **Open Anyway**. Confirm with
Touch ID or your password. Once is enough.

**In a terminal.** Clear the flag macOS set when the file was downloaded:

```sh
xattr -dr com.apple.quarantine /Applications/haru.app
open /Applications/haru.app
```

kirie needs the same treatment if you downloaded its binary rather than
building it:

```sh
xattr -d com.apple.quarantine ~/.local/bin/kirie
```

Neither step is needed for something you built yourself — the flag is only
attached to downloads.

## Build

```sh
cargo build --release        # target/release/haru
packaging/install.sh target/release/haru
```

`install.sh` puts the binary in `~/.local/bin` and the desktop entry and icons
in `~/.local/share`, so haru shows up in the application launcher. `PREFIX=/usr`
installs system-wide.

Needs [tapline](https://github.com/UnhingedSoftware/tapline) checked out beside
this repository.

## Layout

```
haru-core      filters, vocabulary, installed-wallpaper index, config
haru-workshop  tapline: browse, page, count, install
haru-media     preview art: fetch, decode, cache
haru-apply     renderer backends — the only crate that knows a platform
haru-ui        the window
haru           the binary
```

`haru-core` and `haru-workshop` depend on no UI and no backend, so a studio can
be added over the same crates rather than beside them.

MPL-2.0.
