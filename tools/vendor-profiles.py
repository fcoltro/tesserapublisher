#!/usr/bin/env python3
"""Fetch the bundled ICC profiles named in assets/profiles/manifest.tsv.

Run once after cloning, or whenever the manifest changes:

    python tools/vendor-profiles.py

Nothing here is clever. What it is, is suspicious: a URL that has rotted into an
HTML error page, a redirect to a login form, or a truncated download all produce
a file, and a colour-managed application that bundles one of those will proof
against nonsense and be believed. So every download is checked to be an actual
ICC profile of the space the manifest expects before it is allowed to land, and
anything that fails is reported and skipped rather than saved.

A row whose licence tag has no entry in LICENCES.md is refused outright. A
profile is somebody else's work; it does not arrive without its terms.

Options:
    --url FILE=URL   fetch one file from somewhere else (a moved download)
    --from DIR       take files from a local directory instead of the network
    --list           say what is expected and what is present, and change nothing
"""

import argparse
import hashlib
import os
import re
import ssl
import struct
import sys
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
PROFILES = os.path.join(ROOT, "assets", "profiles")
MANIFEST = os.path.join(PROFILES, "manifest.tsv")
LICENCES = os.path.join(PROFILES, "LICENCES.md")

# An ICC profile's header is 128 bytes and the tag table follows it. Anything
# shorter is not one, whatever it is called.
HEADER = 128

# The four bytes at offset 36 of every ICC profile.
SIGNATURE = b"acsp"

# Colour space signatures, at offset 16.
SPACES = {
    b"CMYK": "CMYK",
    b"RGB ": "RGB",
    b"GRAY": "Grey",
    b"Lab ": "Lab",
}


def rows():
    """Every profile the manifest asks for."""
    out = []
    with open(MANIFEST, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            line = line.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != 5:
                sys.exit(
                    f"{MANIFEST}:{number}: expected 5 tab-separated fields, "
                    f"got {len(parts)}: {line!r}"
                )
            file, space, licence, name, source = (p.strip() for p in parts)
            out.append(
                {
                    "file": file,
                    "space": space,
                    "licence": licence,
                    "name": name,
                    "source": source,
                    "line": number,
                }
            )
    return out


def licence_tags():
    """The tags LICENCES.md actually has a section for."""
    with open(LICENCES, encoding="utf-8") as handle:
        text = handle.read()
    return set(re.findall(r"^## `([^`]+)`", text, re.MULTILINE))


def looks_like_a_profile(data):
    """(space, None) for a real ICC profile, or (None, why not)."""
    if len(data) < HEADER:
        return None, f"only {len(data)} bytes; an ICC header alone is {HEADER}"

    # The profile says its own length in its first four bytes. A truncated
    # download is the failure this catches, and it is the one most likely to
    # otherwise be shipped.
    (declared,) = struct.unpack(">I", data[0:4])
    if declared != len(data):
        return None, f"says it is {declared} bytes but is {len(data)}"

    if data[36:40] != SIGNATURE:
        got = data[36:40]
        return None, f"no 'acsp' signature (found {got!r}); not an ICC profile"

    space = SPACES.get(data[16:20])
    if space is None:
        return None, f"unknown colour space {data[16:20]!r}"
    return space, None


def copyright_of(data):
    """The profile's own copyright text, so its terms can be eyeballed.

    Read out of the `cprt` tag rather than trusted from the manifest: the point
    of printing it is to notice when a URL has quietly started serving something
    else.
    """
    try:
        (count,) = struct.unpack(">I", data[HEADER : HEADER + 4])
        for index in range(count):
            at = HEADER + 4 + index * 12
            tag, offset, size = struct.unpack(">4sII", data[at : at + 12])
            if tag != b"cprt":
                continue
            body = data[offset : offset + size]
            # Both the ASCII and the Unicode tag types turn up in the wild.
            text = "".join(chr(b) for b in body if 32 <= b < 127)
            return " ".join(text.split())[:300]
    except (struct.error, IndexError):
        pass
    return ""


def fetch(url):
    """Download, or return None and say why."""
    try:
        # A default context, so certificate verification is on. A profile
        # fetched over an unverified connection is a profile from nobody.
        context = ssl.create_default_context()
        request = urllib.request.Request(
            url, headers={"User-Agent": "tessera-vendor-profiles"}
        )
        with urllib.request.urlopen(request, timeout=60, context=context) as response:
            return response.read(), None
    except (urllib.error.URLError, urllib.error.HTTPError, ssl.SSLError, OSError) as why:
        return None, str(why)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", action="append", default=[], metavar="FILE=URL")
    parser.add_argument("--from", dest="source_dir", metavar="DIR")
    parser.add_argument("--list", action="store_true")
    options = parser.parse_args()

    moved = {}
    for pair in options.url:
        if "=" not in pair:
            sys.exit(f"--url wants FILE=URL, got {pair!r}")
        file, url = pair.split("=", 1)
        moved[file] = url

    wanted = rows()
    tags = licence_tags()
    os.makedirs(PROFILES, exist_ok=True)

    if options.list:
        for row in wanted:
            at = os.path.join(PROFILES, row["file"])
            state = "present" if os.path.exists(at) else "MISSING"
            print(f"{state:8}  {row['space']:5}  {row['name']}  ({row['file']})")
        return 0

    failed = []
    for row in wanted:
        file = row["file"]
        target = os.path.join(PROFILES, file)

        if row["licence"] not in tags:
            # A profile is somebody else's work. It does not arrive without its
            # terms arriving with it.
            print(
                f"REFUSED  {file}: licence tag '{row['licence']}' has no section "
                f"in LICENCES.md"
            )
            failed.append(file)
            continue

        if os.path.exists(target):
            print(f"have     {file}")
            continue

        if options.source_dir:
            local = os.path.join(options.source_dir, file)
            if not os.path.exists(local):
                print(f"missing  {file}: not in {options.source_dir}")
                failed.append(file)
                continue
            with open(local, "rb") as handle:
                data = handle.read()
            why = None
        else:
            url = moved.get(file, row["source"])
            print(f"fetch    {file}  <- {url}")
            data, why = fetch(url)

        if data is None:
            print(f"FAILED   {file}: {why}")
            failed.append(file)
            continue

        space, complaint = looks_like_a_profile(data)
        if space is None:
            print(f"REJECTED {file}: {complaint}")
            failed.append(file)
            continue
        if space != row["space"]:
            print(f"REJECTED {file}: manifest says {row['space']}, file is {space}")
            failed.append(file)
            continue

        with open(target, "wb") as handle:
            handle.write(data)

        digest = hashlib.sha256(data).hexdigest()
        print(f"saved    {file}  {space}  sha256:{digest}")
        notice = copyright_of(data)
        if notice:
            # Printed so a person can compare it with LICENCES.md. An automatic
            # check would need the grant in machine-readable form, and licences
            # are not.
            print(f"         copyright: {notice}")

    print()
    if failed:
        print(f"{len(failed)} of {len(wanted)} could not be vendored: {', '.join(failed)}")
        print("Tessera still runs; the list of profiles is simply shorter.")
        print("See assets/profiles/CANDIDATES.md for what is worth adding and why.")
        return 1

    print(f"all {len(wanted)} profiles vendored")
    print("Check each printed copyright line against assets/profiles/LICENCES.md.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
