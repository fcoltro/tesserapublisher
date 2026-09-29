Ghostscript
===========

This folder holds a copy of Ghostscript, by Artifex Software, which Tessera
Publisher runs as a separate program to place EPS artwork: each EPS file is
converted once to a PDF and placed as that.

Ghostscript is free software, licensed under the GNU Affero General Public
License, version 3 or later: LICENSE beside this file says which parts
are covered and how, and COPYING is the licence itself.
Tessera Publisher is licensed under the GNU General Public License, version 3
or later, and the two licences permit being distributed together.

Its complete source code, for the version shipped here, is available from

    https://github.com/ArtifexSoftware/ghostpdl

(tagged by version, for example ghostpdl-10.08.0), and from
https://ghostscript.com/releases/. Nothing in it has been changed. On Mac
and Linux this copy is built from that source by
packaging/ghostscript/build.sh in Tessera Publisher's own source, which
says exactly how.

Removing this folder is harmless: Tessera Publisher then looks for a
Ghostscript installed on the machine, and without one shows each EPS file's
own preview instead.
