# Terms for every bundled profile

A profile in this directory is somebody else's work being shipped inside
Tessera. Every one needs an entry here quoting the grant that permits it, and
`tools/vendor-profiles.py` refuses to fetch a row whose licence tag has no entry
— so a profile cannot arrive without its terms arriving with it.

Nothing here is committed to the repository. The vendoring script fetches these
files onto the machine that runs it, which keeps the question of what Tessera
*ships* separate from what Tessera *can use*.

## `icc`

Profiles published by the International Color Consortium through
[registry.color.org](https://registry.color.org/).

For `sRGB2014.icc`:

> Copyright International Color Consortium, 2015. This profile is made available
> by the International Color Consortium, and may be copied, distributed,
> embedded, made, used, and sold without restriction. Altered versions of this
> profile shall have the original identification and copyright information
> removed and shall not be misrepresented as the original profile.

For `sRGB_v4_ICC_preference.icc`:

> To anyone who acknowledges that the file "sRGB_v4_ICC_preference.icc" is
> provided "AS IS" WITH NO EXPRESS OR IMPLIED WARRANTY, permission to use, copy
> and distribute this file for any purpose is hereby granted without fee,
> provided that the file is not changed including the ICC copyright notice tag,
> and that the name of ICC shall not be used in advertising or publicity
> pertaining to distribution of the software without specific, written prior
> permission.

**May be sold: yes.** No restriction on redistribution or sale, provided the
files are unaltered and the copyright tags are intact — which is exactly how
Tessera uses them.

Files under this tag:

- `sRGB2014.icc`
- `sRGB_v4_ICC_preference.icc`

## `idealliance-crpc`

The CGATS.21-2 / ISO 15339 characterised reference printing conditions, seven
CMYK presses from cold-set news to extra-large gamut, published through the ICC
registry.

> This profile is made available by IDEAlliance®, with permission of X-Rite,
> Inc., and may be used, embedded, exchanged, and shared without restriction. It
> may not be altered, or sold without written permission of IDEAlliance.

**May be sold: NO.** Read that clause carefully, because it decides who may ship
these.

- **A personal build, and Tessera's own free distribution: fine.** The profiles
  are separate data files sitting beside the program, not part of it — GPL v3
  calls that aggregation, and the grant to "share without restriction" covers it.
- **Selling Tessera, or packaging it for Debian: not fine.** Debian's DFSG
  requires the freedom to sell, and a no-sale clause fails it. Those builds
  should run the vendoring script with `--skip idealliance-crpc` and rely on the
  profiles discovered on the machine, which is what Scribus does.

This is the reason the script has a `--skip`, and the reason nothing fetched is
committed here: the repository itself carries only URLs and terms, so Tessera's
source can be sold by anyone without a thought about profile licensing.

Files under this tag:

- `CGATS21_CRPC1.icc` through `CGATS21_CRPC7.icc`

## Adding a tag

1. Read the licence that ships with the download, not a summary of it.
2. Add a section here, quoting the grant rather than describing it. A paraphrase
   is not a licence.
3. Say plainly whether it **may be sold**, because that is the clause that
   decides who can ship the result.
4. List the files it covers.
5. Add the row to `manifest.tsv`.

If the terms are unclear, the answer is not to bundle. Tessera reads profiles
already installed on the machine, so a press that cannot be shipped is still
reachable — nothing is lost by leaving it out except one entry in a list.
