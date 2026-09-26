# Attribution

Tessera Publisher is licensed under the GNU General Public License v3.0 or
later. Third-party material included in the source is listed here.

## Adobe Spectrum 2 workflow icons

Most of the interface's icons are Adobe's Spectrum 2 workflow icons, from
[React Spectrum](https://github.com/adobe/react-spectrum)
(`packages/@react-spectrum/s2/s2wf-icons`, commit `f1cee83`). They are
vendored as SVG in `crates/tessera_ui/assets/icons/`, with their colour
variable replaced by a plain fill so the rasteriser reads them; the shapes are
unchanged. Each is compiled into the binary and rasterised at runtime.

They are licensed under the Apache License, Version 2.0, a copy of which is in
`crates/tessera_ui/assets/icons/LICENSE`.

> Copyright 2020 Adobe. All rights reserved.

## Lucide icons

The icons Spectrum has no picture of — line caps and joins, text wraps,
paragraph indents and spacing, the pointer's I-beam and crosshair, a book, a
moon, a glyph, the assistant — are drawn in `crates/tessera_ui/src/icons.rs` in
the manner of [Lucide](https://lucide.dev), and some are Lucide's own: the
open book, the moon, the ampersand, the sparkles, the dashed text frame, the
text cursor, the crosshair, the resize arrows and the error and warning
marks. They are stored as SVG path data and rendered at runtime; no image
files are distributed.

Lucide is ISC-licensed:

> Copyright (c) 2026 Lucide Contributors
>
> Permission to use, copy, modify, and/or distribute this software for any
> purpose with or without fee is hereby granted, provided that the above
> copyright notice and this permission notice appear in all copies.

Icons inherited from [Feather](https://feathericons.com) carry the MIT licence:

> Copyright (c) 2013-present Cole Bemis
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction.

Both licences are compatible with GPL-3.0-or-later.

The stroke samples, text wraps, indents, spacing, scale and angle fields,
columns, gutter and table of contents are Tessera-specific geometry on the same
24-unit grid.

## Noto Sans interface font

`assets/fonts/NotoSansVariable.ttf` is the unmodified Noto Sans variable font from
[Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosans).
It is embedded in the executable for interface labels and headings.

Copyright 2022 The Noto Project Authors
(https://github.com/notofonts/latin-greek-cyrillic).
Distributed under the SIL Open Font License 1.1; the complete license is
included in `assets/fonts/OFL-NotoSans.txt`. Light (300) and semibold (600) UI
instances use the font's weight axis at normal width (100) at runtime.
