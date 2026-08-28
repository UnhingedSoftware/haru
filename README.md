# haru

A wallpaper picker and Workshop browser. It finds Wallpaper Engine wallpapers,
installs them, and puts them on your screens — **without the Steam client
running**, on Linux, Windows and macOS.

貼る — "to put up", which is what you do with a wallpaper.

```sh
haru                    # the library, and what is on each screen
haru workshop           # browse
haru --search miku      # browse, already searching
```

## Why it can do that

Everything Steam-shaped goes through [tapline](https://github.com/UnhingedSoftware/tapline),
which speaks Steam's own CM protocol and SteamPipe CDN directly. So:

- **Browsing needs no login and no Steam client.** Measured 2026-08-27:
  3,182,822 Wallpaper Engine items reachable anonymously, a hundred results
  with two filters in 1.2 s.
- **Nothing pretends to play a game.** The other way to reach the Workshop is
  Steamworks, which counts the process as *playing Wallpaper Engine* and bills
  your account real playtime for every search.
- **Installing needs an account that owns Wallpaper Engine**, once:
  `tapline login --qr`. The token is saved and reused; haru stores no
  credentials of its own.

## The filters are Steam's own

Not a subset. Tag groups with Steam's real semantics — one tag from each axis,
so Type=Scene *and* Genre=Anime rather than all-or-any across the lot — plus
the period beside "Most Popular", content descriptors, published/revised date
windows, and searching titles or descriptions rather than both.

## Where wallpapers land

`<steam library>/steamapps/workshop/content/431960/<id>` — where kirie,
Wallpaper Engine and Steam itself already look, so what haru installs is
visible to all of them with no extra wiring. On a machine with no Steam,
`~/.local/share/haru/content/431960/<id>`.

## Applying

haru renders nothing. It hands a directory to whatever does:

| backend | where |
|---|---|
| [kirie](https://github.com/UnhingedSoftware/kirie) over its control socket | Linux |
| Wallpaper Engine itself | Windows (later) |

A wallpaper's own settings — its sliders, colours and switches, read straight
out of `project.json` — are edited beside the screen they are on, and take
effect as you move them.

## Layout

```
haru-core      filters, vocabulary, the installed-wallpaper index, config
haru-workshop  tapline: browse, page, count, install
haru-media     preview fetch, decode and cache
haru-apply     backends; the only crate allowed to know about a platform
haru-ui        the window
haru           the binary
```

`haru-core` and `haru-workshop` depend on no UI and no backend, which is what
lets a studio be added later as another view over the same crates.
