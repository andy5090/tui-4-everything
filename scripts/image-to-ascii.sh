#!/bin/sh
set -eu

usage() {
    echo "usage: $0 INPUT [WIDTH] [HEIGHT] [THRESHOLD] [CROP] [RAMP] [CONTRAST] [BRIGHTNESS]" >&2
    echo "example: $0 logo.png 74 22 80 '1500:620:(iw-1500)/2:(ih-620)/2' ' .:-=+*#%@' 1.5 -0.04" >&2
}

input=${1:-}
width=${2:-74}
height=${3:-18}
threshold=${4:-96}
crop=${5:-}
ramp=${6:-#}
contrast=${7:-1.0}
brightness=${8:-0}

if [ -z "$input" ] || [ ! -f "$input" ]; then
    usage
    exit 2
fi

case "$width:$height:$threshold" in
    *[!0-9:]* | :* | *::* | *:) usage; exit 2 ;;
esac

if [ "$width" -eq 0 ] || [ "$height" -eq 0 ] || [ "$threshold" -gt 255 ]; then
    usage
    exit 2
fi

case "$contrast:$brightness" in
    *[!0-9:.-]* | *..* | *--*) usage; exit 2 ;;
esac

if [ -z "$ramp" ]; then
    usage
    exit 2
fi

crop_filter=
if [ -n "$crop" ]; then
    crop_filter="crop=$crop,"
fi

tone_filter="eq=contrast=$contrast:brightness=$brightness"
if [ "$ramp" = "#" ]; then
    tone_filter="$tone_filter,lut=y='if(gt(val,$threshold),255,0)'"
fi

LC_ALL=C ffmpeg -v error -i "$input" \
    -vf "${crop_filter}scale=$width:$height:flags=lanczos,format=gray,$tone_filter" \
    -f rawvideo - |
    od -An -v -tu1 |
    awk -v width="$width" -v ramp="$ramp" '
        BEGIN {
            levels = length(ramp)
        }
        {
            for (field = 1; field <= NF; field++) {
                if (levels == 1) {
                    glyph = ($field > 0 ? ramp : " ")
                } else {
                    level = int($field * (levels - 1) / 255) + 1
                    glyph = substr(ramp, level, 1)
                }
                line = line glyph
                pixels++
                if (pixels % width == 0) {
                    sub(/[ ]+$/, "", line)
                    print line
                    line = ""
                }
            }
        }
    '
