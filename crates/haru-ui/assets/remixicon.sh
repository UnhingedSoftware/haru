#!/bin/sh
# Cuts remixicon.ttf down to the codepoints haru actually draws.
#
# The whole face is 599 KB for 3,229 icons; this is 880 bytes for five. Every
# codepoint here has a matching arm in ../src/icons.rs, and the test in that
# file fails if one is missing from the font.
#
# Needs fonttools:  python3 -m venv env && env/bin/pip install fonttools
set -eu

ICONS='U+EB99,U+ECAF,U+EF3E,U+EA64,U+EA6E'  # close external-link menu arrow-left-s arrow-right-s
SOURCE="${1:-remixicon-full.ttf}"           # from github.com/Remix-Design/RemixIcon/fonts

pyftsubset "$SOURCE" \
	--unicodes="$ICONS" \
	--no-hinting \
	--desubroutinize \
	--name-IDs='' \
	--notdef-outline \
	--output-file="$(dirname "$0")/remixicon.ttf"
