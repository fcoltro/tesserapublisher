#!/usr/bin/env bash
# Build Ghostscript from source, for the Mac and Linux packages to carry.
#
#   packaging/ghostscript/build.sh <folder>
#
# Leaves <folder>/bin/gs: one program with its PostScript resources and
# fonts compiled in (the build's default), and FreeType, Little CMS, libjpeg,
# libpng, zlib, OpenJPEG and jbig2dec built from the copies in Ghostscript's
# own source tree — so it needs nothing of the machine but the C library,
# which Homebrew's and a distribution's builds do not promise. Its output
# devices are Ghostscript's default set: narrowed to the PDF writer alone,
# 10.08 does not link, since its PDF reader needs the RC4 filter, which
# only comes in with the other devices.
#
# Run as a program beside Tessera, never linked into it: an EPS is a program,
# and one that crashes or never ends costs a conversion, not the document
# being edited. See packaging/ghostscript/README.txt for the licence.
#
# A folder that already holds this version is left as it is, so a cached
# build is not rebuilt.
set -euo pipefail

version=10.08.0
tag=gs10080
sha512=8006e2a32d03759a905b9548bdd83d4563173041006750e17e428b9eea24ad519aa4452ceecd7c073d3c420a68bf1b07ccc9e0533b1a05935f6b39ea8d9ce875

out=${1:?"say where: packaging/ghostscript/build.sh <folder>"}
mkdir -p "$out"
out=$(cd "$out" && pwd)

if [ -x "$out/bin/gs" ] && [ "$(cat "$out/VERSION" 2>/dev/null)" = "$version" ]; then
  echo "Ghostscript $version already built in $out"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive="$work/ghostscript-$version.tar.xz"
curl -fsSL -o "$archive" \
  "https://github.com/ArtifexSoftware/ghostpdl-downloads/releases/download/$tag/ghostscript-$version.tar.xz"

# **Checked before anything in it runs.** The digest is pinned here, from
# Artifex's SHA512SUMS for this release, not fetched beside the archive: a
# checksum from the same place as the file proves only that it downloaded.
if command -v sha512sum >/dev/null 2>&1; then
  actual=$(sha512sum "$archive" | cut -d' ' -f1)
else
  actual=$(shasum -a 512 "$archive" | cut -d' ' -f1)
fi
if [ "$actual" != "$sha512" ]; then
  echo "ghostscript-$version.tar.xz does not match its pinned SHA-512" >&2
  exit 1
fi

tar -xJf "$archive" -C "$work"
cd "$work/ghostscript-$version"

# Everything the machine might offer is refused, so the source tree's own
# copies are what gets built in, and nothing is left to look for at run time.
./configure \
  --prefix="$work/installed" \
  --without-x \
  --disable-cups \
  --disable-gtk \
  --disable-dbus \
  --disable-fontconfig \
  --without-libidn \
  --without-libpaper \
  --without-tesseract \
  --without-ijs \
  --without-urf \
  --without-pdftoraster \
  --without-libtiff \
  --with-local-zlib \
  --with-local-brotli \
  --disable-contrib

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
make -j"$jobs"

mkdir -p "$out/bin"
cp bin/gs "$out/bin/gs"
strip "$out/bin/gs" 2>/dev/null || true

# **It must stand alone.** Every library it names has to be the operating
# system's own; anything else would be missing on the machine it lands on.
case "$(uname -s)" in
  Darwin)
    foreign=$(otool -L "$out/bin/gs" | tail -n +2 | awk '{print $1}' \
      | grep -vE '^(/usr/lib/|/System/)' || true)
    ;;
  *)
    foreign=$(ldd "$out/bin/gs" | awk '{print $1}' \
      | grep -vE '^(linux-vdso|/lib.*/ld-linux|lib(c|m|dl|pthread|rt)\.so)' || true)
    ;;
esac
if [ -n "$foreign" ]; then
  echo "The built gs depends on libraries a user's machine may not have:" >&2
  echo "$foreign" >&2
  exit 1
fi

# **And it must convert an EPS**, from a folder far from its build, with no
# GS_LIB to find resources by: only what is compiled in.
probe="$work/probe"
mkdir -p "$probe"
printf '%s\n' '%!PS-Adobe-3.0 EPSF-3.0' '%%BoundingBox: 0 0 100 50' \
  '/Helvetica findfont 20 scalefont setfont 10 20 moveto (EPS) show' \
  '0 0 100 50 rectstroke' '%%EOF' > "$probe/probe.eps"
(cd "$probe" && env -u GS_LIB "$out/bin/gs" -q -dSAFER -dBATCH -dNOPAUSE \
  -dEPSCrop -sDEVICE=pdfwrite -o probe.pdf probe.eps)
if ! head -c 5 "$probe/probe.pdf" | grep -q '%PDF-'; then
  echo "The built gs did not write a PDF from an EPS" >&2
  exit 1
fi

echo "$version" > "$out/VERSION"
echo "Built Ghostscript $version into $out"
