# Visual review

The headless UI tests can export the actual egui layout into SVG for review:

```sh
HEDIT_PREVIEW_DIR=/tmp/hedit-previews cargo test --locked --bin hedit export_theme_previews -- --ignored
rsvg-convert /tmp/hedit-previews/hedit-paper-appearance.svg -o /tmp/hedit-paper.png
```

These exports preserve layout and colors but substitute system fonts and omit
some GPU meshes/effects. They complement desktop testing rather than replace it.
