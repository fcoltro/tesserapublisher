# Attribution

Tessera Publisher is licensed under the GNU General Public License v3.0 or
later. Third-party material included in the source is listed here.

## Ghostscript

Every package ships Ghostscript, unmodified, in a `ghostscript` folder beside
the application, which runs it as a separate program to convert EPS artwork
to PDF: on Windows `gswin64c.exe` and `gsdll64.dll` from Artifex Software's
release, on Mac and Linux a `gs` built from Artifex's pinned, checksummed
source by `packaging/ghostscript/build.sh`. Its code is not in this
repository; `packaging/build.sh` stages it when it builds a package.

Ghostscript is licensed under the GNU Affero General Public License v3.0 or
later: its notice (`packaging/ghostscript/LICENSE`) and the licence
(`COPYING`) ship beside it with `README.txt`, which says where its source is:
[ArtifexSoftware/ghostpdl](https://github.com/ArtifexSoftware/ghostpdl).

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

## Standard PDF fonts, through hayro

Placed PDF artwork is read and drawn with [hayro](https://github.com/LaurenzV/hayro)
(Apache-2.0 OR MIT), which builds into the executable the Foxit fonts that
stand in for the fourteen standard PDF fonts when a placed file names one
without embedding it. They come from PDFium and carry its licence:

> Copyright 2014 PDFium Authors. All rights reserved.
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are
> met:
>
> - Redistributions of source code must retain the above copyright notice,
>   this list of conditions and the following disclaimer.
> - Redistributions in binary form must reproduce the above copyright
>   notice, this list of conditions and the following disclaimer in the
>   documentation and/or other materials provided with the distribution.
> - Neither the name of Google Inc. nor the names of its contributors may
>   be used to endorse or promote products derived from this software
>   without specific prior written permission.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
> IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
> THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
> PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR
> CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
> EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
> PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
> PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
> LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
> NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
> SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

The BSD licence is compatible with GPL-3.0-or-later.

## JPEG encoding

Pictures are written as JPEG with [jpeg-encoder](https://github.com/vstroebel/jpeg-encoder)
(MIT OR Apache-2.0), whose licence also carries the Independent JPEG Group's,
since parts of it derive from their work:

> This software is based in part on the work of the Independent JPEG Group.

The IJG licence is compatible with GPL-3.0-or-later.
