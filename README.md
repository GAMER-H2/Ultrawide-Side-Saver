# Ultrawide Side Saver

Animated side bars for the unused edges of an ultrawide OLED.

When you watch 16:9 content on a 21:9 panel, the outer columns of pixels sit black
while the centre works. Over enough hours that shows up as a centre-vs-edges
uniformity difference. This puts a dim, slowly moving gradient in those columns so
they age too, without touching the picture in the middle.

It draws two `wlr-layer-shell` surfaces on the **overlay** layer, so they sit above
fullscreen games and browsers, with an empty input region so every click, scroll
and hover falls straight through to whatever is underneath.

Built for KDE Plasma 6 on Wayland. Verified on Plasma 6.7.4 / KWin 6.7.4 /
Mesa 26.1 / RX 6800 XT / CachyOS.

```
 3440x1440 output, 16:9 content
 +--------+--------------------------------+--------+
 | shader |                                | shader |
 | 440px  |     2560px, untouched          | 440px  |
 |        |                                |        |
 +--------+--------------------------------+--------+
```

## Install

```sh
./install.sh              # builds, installs to ~/.local/bin, binds Meta+Shift+B
./install.sh 'Meta+F11'   # or pick your own shortcut
```

This installs the binary, an autostart entry, and a hidden command-shortcut
`.desktop` file bound through `kglobalshortcutsrc`.

**The shortcut only becomes active after you log out and back in.** In Plasma 6
there is no `khotkeys` and no standalone `kglobalacceld` — `kwin_wayland` itself
owns `org.kde.kglobalaccel` and only reads the `[services]` section at startup, and
KWin can't be restarted on Wayland without restarting the session. To get it working
immediately instead, add it by hand in
*System Settings → Keyboard → Shortcuts → Toggle Ultrawide Side Saver*.

> Don't bind `Ctrl+Alt+F1`–`F12`. The XKB `CTRL+ALT` key type maps that level to
> `XF86Switch_VT_n`, and KWin consumes it to switch virtual terminal before any
> global shortcut runs.

`./uninstall.sh` removes everything except your config.

## Use

| Command | What it does |
| --- | --- |
| `ultrawide-side-saver` / `run` | Start the daemon (tray icon + D-Bus control) |
| `ultrawide-side-saver toggle` | Toggle the bars — this is what the shortcut runs |
| `ultrawide-side-saver show` / `hide` | Force a state |
| `ultrawide-side-saver reload` | Re-read the config without restarting |
| `ultrawide-side-saver quit` | Stop the daemon |
| `ultrawide-side-saver outputs` | Show each output and the bar width it would get |
| `ultrawide-side-saver init-config` | Write a commented default config |

The tray icon toggles on left click and has a menu with *Side bars*, *Reload config*
and *Quit*. The bars' state is reflected in the icon.

Only one daemon can run at a time: it claims the D-Bus name
`com.gamerh2.UltrawideSideSaver`, and a second instance exits with an error rather
than fighting the first for the overlay.

## Config

`~/.config/ultrawide-side-saver/config.toml`, then `ultrawide-side-saver reload`.

| Key | Default | Notes |
| --- | --- | --- |
| `output` | `""` | Connector name, e.g. `"DP-1"`. Empty = widest connected output. |
| `content_aspect` | `"16:9"` | Bars fill whatever the output has left over. |
| `fps` | `30` | Low is the point. |
| `brightness` | `0.35` | `0.0`–`1.0`. See the note below. |
| `speed` | `1.0` | `1.0` = one full loop every 10 minutes. |
| `palette` | `"aurora"` | `aurora`, `ember`, `ocean`, `mono`, `forest` |
| `inner_fade` | `0.18` | Soft fade on the content-facing edge, as a fraction of bar width. |
| `start_visible` | `false` | Show the bars as soon as the daemon starts. |
| `cover_panels` | `true` | Draw over panel ends that reach into the bar area. See below. |
| `bar_width` | `0` | Force a pixel width; `0` derives it from `content_aspect`. |

### On `cover_panels`

If your Plasma panel is wide enough to reach into the outer 440px, the bars cover
its ends — on the reference setup that hides the system tray and clock while the
bars are on. That's the correct behaviour for the main use case, since during
fullscreen 16:9 content the panel isn't visible anyway, and it's the same
mechanism that puts the bars above fullscreen windows.

Set `cover_panels = false` if you also toggle the bars on the plain desktop. The
bars then stop short of the panel rather than covering it, at the cost of not
quite reaching the screen edge on that side.

### On `brightness`

There is a real trade-off here and no setting is obviously correct.

The edge pixels only catch up to the centre if they do comparable work, but bars
bright enough to truly match the centre would be distracting and would age those
pixels in their own fixed pattern. `0.35` is deliberately conservative — it lights
the columns to roughly a dim ambient glow rather than matching centre output. It
narrows the gap; it doesn't close it. Raise it if you want more evening-out and can
live with more light in your peripheral vision.

Two things the design already does for you: the pattern never holds still, so no
pixel sits at a fixed value, and the field is continuous across both bars rather
than mirrored, so the two sides don't wear identically either.

## Cost

Measured on the reference machine (release build, 3440×1440, 2×440px bars, 30fps),
with `RSS` from `/proc` and CPU from `utime+stime`:

| State | CPU | RSS |
| --- | --- | --- |
| Hidden | 0.00% | 74 MB |
| Visible | 0.65% | 89 MB |

Hidden really is zero: the frame timer is removed from the event loop, so the
process blocks in `poll` indefinitely rather than waking up to do nothing.

Most of that RSS is the Mesa driver, mapped once the EGL context exists.

GPU cost wasn't isolated — a game was running during measurement and
`gpu_busy_percent` is system-wide. The shader is one fullscreen triangle over
1.27M pixels at 30fps with ~10 `sin` calls per pixel and no textures, no vertex
buffers and no render targets, which is a rounding error on this class of GPU.

## How it works

- **Two surfaces, not one with a hole.** A single fullscreen surface with a
  transparent centre would make the compositor blend 3440×1440 of mostly-nothing
  over every frame the game draws. Two 440px surfaces are ~26% of that area.
- **`Layer::Overlay`** puts the bars above fullscreen windows. No KWin rules needed.
- **`set_exclusive_zone(-1)`** means "reserve nothing, and ignore everyone else's
  exclusive zone", so the bars cover panels instead of being pushed around by them.
  `cover_panels = false` uses `0` instead, which reserves nothing but does respect
  other surfaces' exclusive zones.
- **One D-Bus name, requested with `DoNotQueue` and nothing else.** zbus's default
  request flags include `ReplaceExisting`/`AllowReplacement`, so a second daemon
  would quietly steal the name from the first and both would keep drawing.
- **Empty `wl_region` as the input region** makes them click-through.
- **Frame pacing comes from a `calloop` timer**, not frame callbacks, with
  `eglSwapInterval(0)` — rendering deliberately runs far below the 165Hz refresh
  rate, and blocking in `eglSwapBuffers` would stall the Wayland event loop.
- **The animation phase is wrapped, not the raw clock.** Every time term in the
  shader is an integer multiple of one phase uniform, so the CPU can wrap it at
  `TAU` with no visible discontinuity and `float` precision never degrades, however
  long the daemon has been up.
- **Output changes are handled.** Mode, rotation or scale changes rebuild the bars
  at the new width; if the output disappears the bars are torn down and the target
  is re-picked.

## Not implemented: reacting to centre content

Sampling the middle of the screen to tint the bars would need the
`xdg-desktop-portal` ScreenCast API and a PipeWire stream with dmabuf import — a
permission prompt at startup, a second render path, and a per-frame GPU download.
That's a much larger piece of work than the rest of this program combined, and it
would undercut the "minimal cost" goal. The shader already takes its colours from
uniforms pushed every frame, so a capture source could feed `u_c0`/`u_c1`/`u_c2`
later without touching anything else.

## Layout

```
src/main.rs      CLI dispatch
src/app.rs       Wayland client, layer-shell surfaces, event loop, output selection
src/render.rs    EGL context, GLES3 program, per-bar draw
src/config.rs    TOML config + bar geometry maths
src/palette.rs   Colour palettes
src/ipc.rs       D-Bus control interface and client
src/tray.rs      StatusNotifierItem tray entry (icon drawn in code)
shaders/         GLSL ES 3.00 vertex + fragment
```
