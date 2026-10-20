# Themes and custom colors

Open **Appearance** in the toolbar, choose **Appearance / Custom theme** in the
command palette, or press **Ctrl+,**.

Four palettes are built in: **Hedit Ink**, **Hedit Paper**, **Nord**, and **Ember**.
Paper uses light native controls; the others use dark controls. Choose a preset
to replace the current palette, or edit its colors to make a custom theme.
Editing a built-in palette automatically names the result **My theme**; the
original preset remains available.

The Appearance window provides RGB color pickers and editable `#RRGGBB` fields
for the editor background, surfaces, toolbar/sidebar, controls, borders, text,
secondary text, accent/cursor, selection background/text, recovery success,
warning/error, and CUDA syntax colors. Scroll the color list for syntax roles.
The application and code sample preview valid changes immediately. Invalid hex
input is highlighted and keeps the previous valid color active. Colors are
opaque RGB; alpha/transparency is not part of the theme format.

Text and selection contrast ratios are shown below the colors. A ratio below
4.5:1 is flagged as low contrast. This is informational; custom colors are not
blocked. The **Dark controls** setting controls the native widget defaults; it
does not replace your color choices.

**Revert changes** returns to the palette that was active when the window opened.
**Done** keeps the current palette. The window's close button also keeps valid
changes. Appearance preferences are saved automatically every two seconds and
on normal exit in the session's `window.ron`, separately from buffer checkpoints.
Export a theme to retain another named palette or share it across sessions.

## Portable theme files

Use **Import JSON…** and **Export JSON…**. The `themes/` directory contains the
four complete preset files and a small custom example. The file format is a
versioned object with color values written as hex strings:

```json
{
  "version": 1,
  "name": "Violet Ink",
  "accent": "#B4A0F5",
  "selection": "#413657",
  "selection_text": "#F2ECFF",
  "cuda": "#B4A0F5"
}
```

Omitted roles inherit Hedit Ink. A complete export lists every role. Theme names
must have 1–80 characters; colors must have six hex digits (a leading `#` is
optional on input). Unknown fields, unsupported versions, and files above 64 KiB
are rejected with an error. A failed import keeps your current theme. Export
writes a complete file through a synced temporary file and atomic replacement.

Import/export uses native file dialogs and Rust JSON parsing, with no extension
host, script execution, or runtime package downloads.
