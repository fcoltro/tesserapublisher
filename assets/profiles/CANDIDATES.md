# Freely licensed presses worth bundling

The familiar CMYK names split three ways, and the split is not the one you would
guess from the names. `Coated FOGRA39` and `U.S. Web Coated (SWOP) v2` are
**Adobe's builds** of public characterisation data — the data is public, Adobe's
file is not. So the question is never "is FOGRA39 free?" but "whose build of
FOGRA39 is this, and what did they say about copying it?"

Three separate things travel under one name:

1. **The reference printing condition** — FOGRA39, FOGRA51, CGATS TR 003. A
   standard. Not copyrightable as such.
2. **The characterisation data** — the measured patch set, published by FOGRA,
   ECI, or IDEAlliance. Usually free to download; terms vary.
3. **The ICC profile** — somebody's build from that data, with their own
   separation, black generation and gamut mapping. This is a work with an owner.

Two different profiles of the same printing condition are both "FOGRA39" and are
not interchangeable byte for byte, which is also why Tessera prefers to *find*
the profile a machine already has: matching the next application matters more
than matching a name.

## Worth adding, once the terms are confirmed

Each of these is reported to be freely redistributable. **None is in the manifest
yet, because "reported to be" is not a licence I have read.** Read the terms that
ship with the download, add an entry to `LICENCES.md` quoting them, then add the
row to `manifest.tsv`.

### basICColor / OpenICC set — the strongest candidate

`ISOcoated_v2_bas.ICC`, `ISOcoated_v2_300_bas.ICC`, `ISOuncoated.ICC`,
`ISOwebcoated.ICC`, `ISOnewspaper26v4.icc`.

These are basICColor's builds, distributed through the OpenICC project under a
permissive (zlib/libpng-style) licence. The reason to trust this one further than
the others: Debian ships them in **`icc-profiles-free`**, and Debian's ftpmasters
review licences before a package may be called DFSG-free. That is a second party
having read the terms.

`ISOcoated_v2_bas.ICC` is FOGRA39L — the same printing condition as Adobe's
`Coated FOGRA39`. It is the single most useful file on this page.

Look in: the `icc-profiles-free` source package, or the OpenICC data release.

### ECI

`ISOcoated_v2_eci.icc`, `PSOcoated_v3.icc` (FOGRA51),
`PSOuncoated_v3_FOGRA52.icc`, `eciRGB_v2.icc`.

The European Color Initiative publishes these free of charge from eci.org, as a
zip. Free to *use* is clear; free to *redistribute inside an application* is the
part to check.

`PSOcoated_v3` is FOGRA51, which is the current European coated standard and is
what FOGRA39 is being superseded by. Worth having for that reason alone.

### ICC, at color.org

The sRGB profiles carry an explicit and unusually generous grant — copy,
distribute, embed, use and sell without restriction. Those two are already in the
manifest, and they are why the `icc` licence tag exists.

ICC also publishes **CGATS21 reference printing condition** profiles (CRPC1
through CRPC7, newsprint through premium coated). If their terms match the sRGB
grant, this is the ideal set: seven presses spanning the whole range, from the
body that defines the format.

### Not free — do not bundle

- Adobe's builds: `CoatedFOGRA39.icc`, `USWebCoatedSWOP.icc`,
  `JapanColor2001Coated.icc`, `AdobeRGB1998.icc`, and the rest of
  `Common Files/Adobe/Color/Profiles`. Tessera reads these where they are
  already installed and never copies them.
- Anything from a printer's own supplied set without asking that printer.

## ACES and OpenColorIO — a different question

Blender's colour management is **OpenColorIO**, not ICC, and the two solve
different problems. OCIO is built for rendering and film: scene-linear working
spaces, view transforms like AgX and Filmic, no notion of ink. ACES is freely
licensed and would be entirely legitimate to adopt.

It would not help here. **ACES has no CMYK output**, so it cannot answer "what
will this look like on that press", which is the whole of what an output intent
is for. Adding OCIO to get FOGRA39 would be adding a system that does not contain
it.

Where it *would* earn its place is a different job: handling HDR and wide-gamut
imagery in a scene-linear space, and a view transform for tone mapping. That is
worth considering when placed artwork grows beyond 8-bit RGB — and it belongs
alongside ICC rather than instead of it.
