# Rizxel .rzxl

Rizxel's own project format keeps everything: the palette, layers with their groups,
clones, shapes, crops, opacity and materials, reference images (with the image files
embedded) and the render settings.

- *File ▸ Save* (`Ctrl+S`) and *Save As...* (`Ctrl+Shift+S`).
- The file is zstd-compressed JSON with a small binary header.
- A save writes a temporary file first, so a failed save never damages the project.

## Compatibility

The current format version is **5**. Rizxel loads every older version (1 to 5), and
refuses files from a newer Rizxel instead of loading them partly. From 0.10.0 on, every
later Rizxel keeps loading files saved by 0.10.0; this becomes a binding promise at 1.0.
The [format specification](https://github.com/MRizky099/Rizxel/blob/main/docs/rzxl_format.md)
lists every field and the compatibility rules.
