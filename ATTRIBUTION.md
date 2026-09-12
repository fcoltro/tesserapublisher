# Attribution

Tessera Publisher is licensed under the GNU General Public License v3.0 or
later. Third-party material included in the source is listed here.

## Lucide icons

The tool icons in `crates/tessera_ui/src/icons.rs` are drawn from
[Lucide](https://lucide.dev). They are stored as SVG path data and painted at
runtime; no image files are distributed.

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

The Direct Select, Picture Frame, Polygon, Properties, Pages, Preflight and
Swatches paths are Tessera-specific geometry using the same 24-unit grid.

## Noto Sans interface font

`assets/fonts/NotoSansVariable.ttf` is the unmodified Noto Sans variable font from
[Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosans).
It is embedded in the executable for interface labels and headings.

Copyright 2022 The Noto Project Authors
(https://github.com/notofonts/latin-greek-cyrillic).
Distributed under the SIL Open Font License 1.1; the complete license is
included in `assets/fonts/OFL-NotoSans.txt`. Light (300) and semibold (600) UI
instances use the font's weight axis at normal width (100) at runtime.
