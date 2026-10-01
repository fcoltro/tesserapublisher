Spelling dictionaries shipped with Tessera
==========================================

Hunspell dictionaries, read by Tessera's spell checker and dynamic
spelling. The installers put this folder beside the application (in a Mac
bundle, under Contents/Resources); Tessera reads it after the person's own
dictionaries folder, so a dictionary of their own for the same language
wins.

  en_US.dic, en_US.aff   American English
  en_GB.dic, en_GB.aff   British English, "-ise" spelling

Both are SCOWL's Hunspell builds, version 2020.12.07, unchanged, from
http://wordlist.aspell.net/ (SourceForge project "wordlist"):

  hunspell-en_US-2020.12.07.zip
    en_US.dic  sha256 identical to LibreOffice's en_US.dic
  hunspell-en_GB-ise-2020.12.07.zip
    en_GB-ise.dic, en_GB-ise.aff  renamed en_GB.dic, en_GB.aff

Text set in English ("en") is checked against one of them, chosen in
Preferences > Spelling: American (the default) or British.

Licence: the SCOWL licence, permissive, which asks that its copyright and
permission notice travel with the files. It is in README_en_US.txt and
README_en_GB-ise.txt, under "COPYRIGHT, SOURCES, and CREDITS", as SCOWL
ships them.

Other languages: put a Hunspell .dic and .aff pair, named by language
("de.dic" and "de.aff", or "de_DE.dic"), in the dictionaries folder beside
Tessera's preferences. LibreOffice's dictionaries are that format:
https://github.com/LibreOffice/dictionaries

These files are marked -text in .gitattributes so git never rewrites
their line endings.
