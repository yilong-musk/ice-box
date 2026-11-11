# Switch logo

A pixel-art logo for ice-box: a network switch cut from ice crystal. Its port
lights show the core state. It is drawn in code with
[pixelfx](https://github.com/patch-gg/pixelfx), in the palette and finish of
pixelfx's `ice/ice-crystal` effect.

<img src="assets/logo@4x.png" alt="ice-box switch logo" width="192">

This directory holds the source and the rendered assets. The app does not use
them yet: the app icons, favicon, and the Home status logo still use
`apps/desktop/src/assets/logo.png`.

## Core states

Each state is a 3-second loop. The port lights change with the state, while
the body, glints, cold light, and frost motes stay the same.

| State | Still | Port lights | Power light |
| --- | --- | --- | --- |
| `stopped` | <img src="assets/states/stopped-still@4x.png" alt="" width="96"> | All off | Dim |
| `starting` | <img src="assets/states/starting-still@4x.png" alt="" width="96"> | An amber light runs across the ports, left to right, twice a loop | Blinks amber |
| `running` | <img src="assets/states/running-still@4x.png" alt="" width="96"> | Green link lights hold steady on plugged-in ports; activity lights blink pale green with traffic | Green |
| `stopping` | <img src="assets/states/stopping-still@4x.png" alt="" width="96"> | Amber lights go out column by column from the right, then all dark for a beat | Amber |
| `error` | <img src="assets/states/error-still@4x.png" alt="" width="96"> | Every port flashes red together | Flashes red |

Amber is kept for the transitions, so the running activity lights are pale
green, not amber. With a state, each light is two pixels wide so the state
reads at a glance; the plain logo uses one-pixel ice-blue lights.

## Assets

Everything under `assets/` is transparent PNG at the native 48 × 48 pixel
size, plus 4× nearest-neighbour copies of the stills.

| File | Contents |
| --- | --- |
| `logo.png`, `logo@4x.png` | The plain logo, still |
| `states/<state>.png` | The state's loop as one horizontal strip: 45 frames of 48 × 48, left to right |
| `states/<state>-still.png`, `@4x` | One still frame of the state |
| `manifest.json` | Frame size, frame count, fps, loop duration, and state names |

The strips play at 15 fps (45 frames over 3 s). In CSS, show one frame and
step the background across the strip:

```css
.switch-logo {
  width: 48px;
  height: 48px;
  background: url("assets/states/running.png") 0 0 / 2160px 48px no-repeat;
  image-rendering: pixelated;
  animation: switch-logo 3s steps(45) infinite;
}
@keyframes switch-logo {
  to { background-position: -2160px 0; }
}
```

Scale by whole numbers (96, 144, 192 px) to keep the pixels crisp. At 32 and
16 px the art has to be shrunk and goes soft; small icons will need a
hand-drawn version.

## Regenerating

The effect is `fx/switch-logo.js`. It runs inside a pixelfx checkout. The
assets here were rendered with pixelfx at commit `044e12a` (2026-11-09).

```bash
git clone https://github.com/patch-gg/pixelfx /path/to/pixelfx
cp docs/brand/switch-logo/fx/switch-logo.js /path/to/pixelfx/src/fx/brand/switch-logo.js
node docs/brand/switch-logo/tools/export.mjs /path/to/pixelfx
```

`export.mjs` rewrites everything under `assets/`. To look at the effect on
its own, use pixelfx's tools from the checkout, for example
`node tools/render.mjs brand/switch-logo --opts '{"state":"running"}' --gif`
(the GIF needs ffmpeg).

Options the effect takes (`--opts` JSON):

| Option | Default | Meaning |
| --- | --- | --- |
| `state` | `""` | `stopped`, `starting`, `running`, `stopping`, `error`, or empty for the plain logo |
| `still` | `-1` | A loop phase from 0 to 1 to freeze at, for a still frame; `-1` animates |
| `motes` | `1` | `0` turns off the frost motes |
| `palette` | ice-crystal ramp | Eight colors, dark to bright |
| `seed` | `21` | Which ports are plugged in and how their traffic blinks |

## License

`fx/switch-logo.js` and `tools/export.mjs` are GPL-3.0-or-later, like the
rest of ice-box. The palette, frost motes, and glints in `fx/switch-logo.js`
follow pixelfx's `ice/ice-crystal`, which is MIT licensed (Copyright (c) 2026
Patch, Copyright (c) 2026 Hugo Duprez). Its license text is in
[`LICENSE-pixelfx.txt`](LICENSE-pixelfx.txt).
