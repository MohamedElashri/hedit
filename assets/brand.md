# hedit identity

**hedit** is always lowercase. **Keep your train of thought.** is the tagline.

The buffer mark joins a geometric lowercase h with two stacked lines. The h's
cut shoulder echoes a folded corner; the offset lines suggest logs and buffers
held in the background. The open space between the elements keeps it legible as
a small application icon. This design is independent of the upstream identity.

The wordmark is custom outline lettering. Distribute the SVG rather than
retyping the name in a similar font. All primary logo assets are paths, with no
font, bitmap, script, or network dependency.

| Asset | Use |
| --- | --- |
| `hedit.svg` | Application icon: sea glass mark on rounded ink tile |
| `hedit-mark.svg` | Transparent symbol for dark backgrounds |
| `hedit-mark-mono.svg` | Single-color symbol; inherits `currentColor` |
| `hedit-wordmark-dark.svg` | Wordmark on dark backgrounds |
| `hedit-wordmark-light.svg` | Wordmark on light backgrounds |
| `hedit-lockup-dark.svg` | Symbol and name on dark backgrounds |
| `hedit-lockup-light.svg` | Symbol and name on light backgrounds |
| `hedit-logo-mono.svg` | Single-color symbol and name for print/engraving |
| `brand-board.svg` | Identity overview, tagline, and color palette |

Leave at least one stem-width of clear space around the mark. Use the tile at
16 px and above, the transparent mark at 24 px and above, and the complete lockup
at 160 px and above. Keep the proportions; use the monochrome version when the
application calls for a single ink. Light/dark suffixes describe the intended
background, not the fill color. Monochrome SVGs default to black and can inherit
a color when embedded inline.

## Color

| Token | Value | Purpose |
| --- | --- | --- |
| Ink | `#10161F` | Primary identity background |
| Surface | `#161E29` | Raised surfaces |
| Sea glass | `#64D8CB` | Primary mark and active accents |
| Frost | `#9AEBDF` | Stacked buffer lines |
| Slate | `#8DA0B4` | Secondary identity text |
| Paper | `#FAF8F3` | Light interface background |
| Deep teal | `#147568` | Accent on light backgrounds |

The installed launcher keeps the fixed brand colors. Inside the editor, the mark
uses the active theme's accent and secondary text colors. This lets custom
palettes feel consistent without changing the product's identity assets.

## Interface direction

Give the document most of the space. Use a compact toolbar, quiet borders,
consistent rounded controls, and a restrained accent. Active tabs have an accent
underline; active buffer rows have a side marker. Recovery always has a written
status as well as a color. Keep editing and tool output monospace, with native
proportional type for controls. Menus hold secondary actions.

## Maintenance

Run `python3 scripts/generate-brand.py` to regenerate the SVG set. The native
window icon and in-app mark use matching geometry in `src/brand.rs`. The brand
board uses system text for its explanatory labels; the actual wordmarks are
font-independent paths.
