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
    --skip TAG       leave out everything under one licence tag
    --only TAG       take nothing but that tag
    --list           say what is expected and what is present, and change nothing

Some profiles may be shared but not sold. `--skip idealliance-crpc` leaves those
out, which is what a build that will be sold, or packaged for a distribution
requiring the freedom to sell, has to do. LICENCES.md says which tags those are
and why, and the script prints a reminder when it fetches one.
"""

import argparse
import hashlib
import os
import re
import struct
import sys
import subprocess

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


def is_icc_profile(data):
    """Whether these bytes are an ICC profile, said by the bytes themselves.

    **Not by the status code and not by the file extension.** A reorganised
    web site answers a request for `sRGB2014.icc` with an HTML page and a
    perfectly cheerful 200, and a downloader that trusted either would write
    that page into `assets/profiles/` under an `.icc` name. It would then fail
    somewhere inside Little CMS, months later, with a message about a malformed
    tag table.

    Every ICC profile carries `acsp` at offset 36. That is the whole check, it
    is the one the specification defines, and it costs four bytes.
    """
    return len(data) > 132 and data[36:40] == b"acsp"


def fetch(url):
    """Download, or return None and say why.

    Through `curl` rather than `urllib`, and that is not a preference.

    Two things bite on a locked-down machine, and they look alike from a
    distance. Python's sockets may be refused outright by a sandbox policy
    (`WinError 10013` on Windows), which no amount of TLS configuration fixes.
    And `curl` built against Windows schannel tries to check certificate
    revocation, which fails closed with `CRYPT_E_REVOCATION_OFFLINE` when the
    revocation responder cannot be reached — a network that works perfectly for
    everything else.

    `--ssl-no-revoke` turns off *revocation* checking only. Certificate
    verification stays on: a profile fetched over an unverified connection is a
    profile from nobody, and that is a different thing from one whose issuer's
    revocation list happens to be unreachable.
    """
    try:
        finished = subprocess.run(
            [
                "curl",
                "--silent",
                "--show-error",
                "--location",
                "--ssl-no-revoke",
                "--max-time",
                "120",
                "--user-agent",
                "tessera-vendor-profiles",
                "--output",
                "-",
                url,
            ],
            capture_output=True,
            timeout=180,
        )
    except (OSError, subprocess.TimeoutExpired) as why:
        return None, str(why)

    if finished.returncode != 0:
        return None, (finished.stderr.decode("utf-8", "replace").strip() or
                      f"curl exited {finished.returncode}")

    data = finished.stdout
    if not is_icc_profile(data):
        # Said as what it is, because "downloaded 6923 bytes" reads as success.
        head = data[:60].decode("utf-8", "replace").strip().replace("\n", " ")
        return None, (
            f"the server sent {len(data)} bytes that are not an ICC profile "
            f"(no 'acsp' at offset 36) — starts: {head!r}"
        )
    return data, None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", action="append", default=[], metavar="FILE=URL")
    parser.add_argument("--from", dest="source_dir", metavar="DIR")
    parser.add_argument("--skip", action="append", default=[], metavar="TAG")
    parser.add_argument("--only", action="append", default=[], metavar="TAG")
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

    # A tag named on the command line that no row uses is almost certainly a
    # typo, and a typo in `--skip` would silently ship what was meant to be left
    # out. That is the one mistake here with a consequence, so it is an error.
    present = {row["licence"] for row in wanted}
    for tag in options.skip + options.only:
        if tag not in present:
            sys.exit(
                f"no row uses the licence tag {tag!r}; the manifest has: "
                + ", ".join(sorted(present))
            )

    if options.only:
        wanted = [row for row in wanted if row["licence"] in options.only]
    wanted = [row for row in wanted if row["licence"] not in options.skip]

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
        if row["licence"] == "idealliance-crpc":
            # Printed at the moment of fetching, because a clause read once in a
            # file is a clause forgotten by the time somebody packages this.
            print("         may be shared but NOT SOLD - see LICENCES.md")
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
