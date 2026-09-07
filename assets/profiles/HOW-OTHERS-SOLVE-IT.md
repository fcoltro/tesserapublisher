# How the other open-source tools get their profiles

Photoshop's CMYK menu is long because Adobe ships those files. No free
application can copy that list, and yet Krita, Scribus, GIMP, darktable and
Inkscape all manage colour. They do it by splitting the problem the same way,
and the split is worth copying because it is forced by what the two halves *are*
rather than by anybody's preference.

## The pattern, across all of them

**RGB working spaces are shipped or synthesised.** They are defined by published
numbers — three primaries, a white point, a transfer curve — so a small permissive
file, or a few lines of code, reproduces them exactly. Nobody has to ask
permission for arithmetic.

**CMYK presses are not shipped.** They are measured data with an owner, so every
one of these applications gets them from *outside itself*: the distribution's
profile packages, or the user's own file. Not one of them bundles Adobe's builds,
and the CMYK menu in a fresh install of any of them is short or empty.

That second half is the answer to "how did they manage it". Mostly, they didn't —
they arranged not to have to.

## Each of them

### Krita

Ships **Elle Stone's profiles**, which are released into the public domain. They
cover the RGB working spaces — sRGB, an Adobe RGB-compatible build, ProPhoto,
Rec. 2020, ACES — in both normal and linear-gamma versions, which matters for
compositing. This is the cleanest precedent available: a complete, genuinely
unencumbered RGB set that anyone may ship.

For CMYK, Krita takes what the system has.

### darktable and RawTherapee

The same Elle Stone set, for the same reason. Both are photographic tools with no
print output, so CMYK does not arise.

### GIMP

Generates sRGB internally rather than shipping it, and reads everything else from
the system. Its CMYK support has always been limited, and requiring the user to
supply a profile is part of why.

### Scribus — the one that matters here

The closest analogue to Tessera: open-source DTP, littleCMS, real CMYK, real soft
proofing. Scribus **discovers** rather than bundles. It searches the system colour
directories, its own `share/color/icc`, and `~/.color/icc`, and its documentation
sends users to ECI and OpenICC for the presses themselves. On Linux it leans on
the distribution's `icc-profiles` packages.

Tessera does the same thing, and Scribus having done it for twenty years is the
strongest evidence it is the right shape rather than a shortcut.

### Inkscape

Basic ICC support, ships almost nothing, reads the system directories.

## What this changed here

Two things, both from looking at where these applications actually put files.

**The scan now recurses.** It did not, and that was a real bug rather than a
missing nicety: Debian's `icc-profiles-free` installs into
`/usr/share/color/icc/basICColor/` and `/usr/share/color/icc/OpenICC/`, so
reading one level found nothing on exactly the platform where the freely licensed
presses live. It now walks three levels, does not follow symlinks, and stops after
a few hundred files.

**The search paths include the other tools.** Krita, Scribus, GIMP, Inkscape and
darktable each keep profiles inside their own installation. Looking there costs a
`read_dir` that usually fails, and on a machine with any of them installed it
gains their whole answer.

## Where Tessera differs

It **builds** the RGB spaces from published numbers rather than shipping Elle
Stone's files. Both are legitimate; building them means there is no file to lose,
no licence to track, and the list is never empty on a bare machine. Elle Stone's
set remains worth adding for the linear-gamma variants, which cannot be got from
the same primaries without deciding to offer them.

For CMYK it does what Scribus does, and for the same reason.
