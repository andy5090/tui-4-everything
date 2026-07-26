#!/bin/sh
set -eu

usage() {
    echo "usage: $0 INPUT [WIDTH] [HEIGHT] [THRESHOLD] [CROP]" >&2
    echo "example: $0 logo.png 74 18 80 '1400:500:(iw-1400)/2:(ih-500)/2'" >&2
}

input=${1:-}
width=${2:-74}
height=${3:-18}
threshold=${4:-96}
crop=${5:-}

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

crop_filter=
if [ -n "$crop" ]; then
    crop_filter="crop=$crop,"
fi

LC_ALL=C ffmpeg -v error -i "$input" \
    -vf "${crop_filter}scale=$width:$height:flags=lanczos,format=gray,lut=y='if(gt(val,$threshold),255,0)'" \
    -f rawvideo - |
    od -An -v -tu1 |
    awk -v width="$width" '
        {
            for (field = 1; field <= NF; field++) {
                line = line ($field > 0 ? "#" : " ")
                pixels++
                if (pixels % width == 0) {
                    sub(/[ ]+$/, "", line)
                    print line
                    line = ""
                }
            }
        }
    '
