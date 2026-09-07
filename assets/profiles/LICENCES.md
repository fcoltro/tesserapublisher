# Terms for every bundled profile

A profile in this directory is somebody else's work being shipped inside
Tessera. Every one needs an entry here quoting the grant that permits it, and
`tools/vendor-profiles.py` refuses to fetch a row whose licence tag has no entry
— so a profile cannot arrive without its terms arriving with it.

## `icc`

Profiles published by the International Color Consortium at color.org.

The ICC's grant, as it appears with these profiles:

> To anyone who acknowledges that the file "sRGB2014.icc" is provided "AS IS"
> with no express or implied warranty, permission to use, copy and distribute
> this file for any purpose is hereby granted without fee.

Files under this tag:

- `sRGB2014.icc`
- `sRGB_v4_ICC_preference.icc`

**Confirm on fetch.** The script prints the profile's own copyright tag when it
saves a file. If what it prints does not match the grant above, the URL has
moved onto something else — stop and correct the row rather than shipping it.

## Adding a tag

1. Read the licence that ships with the download, not a summary of it.
2. Add a section here, quoting the grant rather than describing it. A paraphrase
   is not a licence.
3. List the files it covers.
4. Add the row to `manifest.tsv`.

If the terms are unclear, the answer is not to bundle. Tessera reads profiles
already installed on the machine, so a press that cannot be shipped is still
reachable — nothing is lost by leaving it out except one entry in a list.
