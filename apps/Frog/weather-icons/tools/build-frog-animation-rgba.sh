#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
icon_root=$(cd -- "$script_dir/.." && pwd)
source_dir="$icon_root/ui4-jpeg/64"
output_dir="$icon_root/ui4-rgba/64"
background=${UI4_WEATHER_BACKGROUND:-142238}

if [[ ! "$background" =~ ^[[:xdigit:]]{6}$ ]]; then
    echo "UI4_WEATHER_BACKGROUND must be six hexadecimal RGB digits" >&2
    exit 2
fi
if ! command -v magick >/dev/null 2>&1; then
    echo "ImageMagick's magick command is required" >&2
    exit 1
fi

icons=(
    clear-day
    clear-night
    cloudy-2-day
    cloudy-2-night
    cloudy
    rainy-1-day
    rainy-2
    thunderstorms
    snowy-1
    fog-day
)

mkdir -p -- "$output_dir"
find "$output_dir" -maxdepth 1 -type f -name '*-frame-???.rgba' -delete

for icon in "${icons[@]}"; do
    for frame_index in {0..9}; do
        frame=$(printf '%03d' "$frame_index")
        source="$source_dir/$icon-frame-$frame.jpg"
        destination="$output_dir/$icon-frame-$frame.rgba"
        if [[ ! -f "$source" ]]; then
            echo "missing animation frame: $source" >&2
            exit 1
        fi
        magick "$source" \
            -alpha on \
            -fuzz 8% \
            -transparent "#$background" \
            -depth 8 \
            "rgba:$destination"
        if [[ $(stat -c '%s' "$destination") != 16384 ]]; then
            echo "unexpected RGBA size: $destination" >&2
            exit 1
        fi
    done
done
