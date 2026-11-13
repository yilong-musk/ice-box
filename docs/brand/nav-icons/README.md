# Navigation icons

Pixel-art glyphs for the app's menu buttons: Home, Nodes, Apps, Rules,
Subscriptions, Connections, MCP and Settings. Drawn in code with
[pixelfx](https://github.com/patch-gg/pixelfx). One ink: the app fills each
mark with the button's text color, so these previews are a single navy only
so the shape can be seen.

<img src="assets/home@4x.png" alt="" width="64">
<img src="assets/nodes@4x.png" alt="" width="64">
<img src="assets/apps@4x.png" alt="" width="64">
<img src="assets/rules@4x.png" alt="" width="64">
<img src="assets/subs@4x.png" alt="" width="64">
<img src="assets/connections@4x.png" alt="" width="64">
<img src="assets/mcp@4x.png" alt="" width="64">
<img src="assets/settings@4x.png" alt="" width="64">

The sidebar and the phone tab bar draw them at 16 px, in place of the stroke
icons. The button around each one is unchanged.

## Regenerating

The effect is `fx/nav-icons.js`. It runs inside a pixelfx checkout. These
assets were rendered with pixelfx at commit `5294518`. Copying and rendering
are both done by the export script.

```bash
git clone https://github.com/patch-gg/pixelfx /path/to/pixelfx
node docs/brand/nav-icons/tools/export.mjs /path/to/pixelfx
```

To look at the set before exporting, from the checkout:

```bash
node tools/render.mjs ui/nav-icons --zoom 8 --bg light
```

## License

`fx/nav-icons.js` and `tools/export.mjs` are GPL-3.0-or-later, like the rest of
ice-box. The glyphs are original. Rendering uses pixelfx, which is MIT
licensed (Copyright (c) 2026 Patch). The license text is in
[`../switch-logo/LICENSE-pixelfx.txt`](../switch-logo/LICENSE-pixelfx.txt).
