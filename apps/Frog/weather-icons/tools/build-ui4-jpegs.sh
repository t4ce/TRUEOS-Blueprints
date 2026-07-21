#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
icon_root=$(cd -- "$script_dir/.." && pwd)
output_root="$icon_root/ui4-jpeg"
output_64="$output_root/64"
output_128="$output_root/128"
frame_count=${UI4_WEATHER_FRAME_COUNT:-10}
background=${UI4_WEATHER_BACKGROUND:-142238}

case "$frame_count" in
    ''|*[!0-9]*)
        echo "UI4_WEATHER_FRAME_COUNT must be an integer from 1 through 10" >&2
        exit 2
        ;;
esac
if ((frame_count < 1 || frame_count > 10)); then
    echo "UI4_WEATHER_FRAME_COUNT must be from 1 through 10" >&2
    exit 2
fi
if [[ ! "$background" =~ ^[[:xdigit:]]{6}$ ]]; then
    echo "UI4_WEATHER_BACKGROUND must be six hexadecimal RGB digits" >&2
    exit 2
fi

chrome=${CHROME:-}
if [[ -z "$chrome" ]]; then
    for candidate in google-chrome chromium chromium-browser; do
        if command -v "$candidate" >/dev/null 2>&1; then
            chrome=$(command -v "$candidate")
            break
        fi
    done
fi
if [[ -z "$chrome" || ! -x "$chrome" ]]; then
    echo "Google Chrome or Chromium is required to sample SVG animations" >&2
    exit 1
fi
if ! command -v magick >/dev/null 2>&1; then
    echo "ImageMagick's magick command is required to encode JPEG frames" >&2
    exit 1
fi

mkdir -p -- "$output_64" "$output_128"
find "$output_64" "$output_128" -maxdepth 1 -type f \
    \( -name '*-frame-??.jpg' -o -name '*-frame-???.jpg' \
       -o -name '*-frame-00.rgba' -o -name '*-frame-000.rgba' \) -delete

work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT
profile_dir="$work_dir/chrome-profile"
mkdir -p -- "$profile_dir"

manifest_tmp="$work_dir/manifest.tsv"
printf 'icon\tframe_count\tperiod_seconds\tfirst_64_jpeg\tfirst_128_jpeg\tfirst_64_rgba8\tfirst_128_rgba8\n' >"$manifest_tmp"

icon_count=0
while IFS= read -r source; do
    icon=$(basename -- "$source" .svg)
    static_source="$icon_root/static/$icon.svg"
    if [[ ! -f "$static_source" ]]; then
        echo "missing static first-frame source for $icon" >&2
        exit 1
    fi

    period=$(
        grep -Eo 'animation-duration:[[:space:]]*[0-9.]+s|dur="[0-9.]+s"' "$source" \
            | sed -E 's/.*:[[:space:]]*([0-9.]+)s/\1/; s/.*="([0-9.]+)s"/\1/' \
            | sort -nr \
            | head -n 1
    )
    period=${period:-1}
    strip_width=$((frame_count * 128))
    strip="$work_dir/$icon.png"
    capture_url="file://$script_dir/ui4-capture.html?icon=$icon&frames=$frame_count&period=$period&background=$background"

    "$chrome" \
        --headless=new \
        --no-sandbox \
        --allow-file-access-from-files \
        --disable-background-networking \
        --disable-component-update \
        --disable-default-apps \
        --disable-dev-shm-usage \
        --disable-extensions \
        --force-device-scale-factor=1 \
        --hide-scrollbars \
        --no-first-run \
        --run-all-compositor-stages-before-draw \
        --user-data-dir="$profile_dir" \
        --virtual-time-budget=3000 \
        --window-size="$strip_width,128" \
        --screenshot="$strip" \
        "$capture_url" >/dev/null 2>&1

    dimensions=$(magick identify -format '%wx%h' "$strip")
    if [[ "$dimensions" != "${strip_width}x128" ]]; then
        echo "$icon capture has unexpected dimensions $dimensions" >&2
        exit 1
    fi

    for ((frame_index = 0; frame_index < frame_count; frame_index += 1)); do
        frame=$(printf '%03d' "$frame_index")
        x=$((frame_index * 128))
        output_name="$icon-frame-$frame.jpg"

        magick "$strip" \
            -crop "128x128+$x+0" +repage \
            -alpha off \
            -colorspace sRGB \
            -sampling-factor 4:4:4 \
            -quality 90 \
            -strip \
            "$output_128/$output_name"
        magick "$strip" \
            -crop "128x128+$x+0" +repage \
            -filter Lanczos \
            -resize 64x64 \
            -alpha off \
            -colorspace sRGB \
            -sampling-factor 4:4:4 \
            -quality 90 \
            -strip \
            "$output_64/$output_name"

        if ((frame_index == 0)); then
            magick "$strip" \
                -crop "128x128+$x+0" +repage \
                -alpha set \
                -channel A -evaluate set 100% +channel \
                -depth 8 \
                "rgba:$output_128/$icon-frame-000.rgba"
            magick "$strip" \
                -crop "128x128+$x+0" +repage \
                -filter Lanczos \
                -resize 64x64 \
                -alpha set \
                -channel A -evaluate set 100% +channel \
                -depth 8 \
                "rgba:$output_64/$icon-frame-000.rgba"
        fi
    done

    printf '%s\t%s\t%s\t64/%s-frame-000.jpg\t128/%s-frame-000.jpg\t64/%s-frame-000.rgba\t128/%s-frame-000.rgba\n' \
        "$icon" "$frame_count" "$period" "$icon" "$icon" "$icon" "$icon" >>"$manifest_tmp"
    icon_count=$((icon_count + 1))
    printf 'generated %s (%s/%s)\n' "$icon" "$icon_count" "$(find "$icon_root/animated" -maxdepth 1 -type f -name '*.svg' | wc -l)"
done < <(find "$icon_root/animated" -maxdepth 1 -type f -name '*.svg' | sort)

mv -- "$manifest_tmp" "$output_root/manifest.tsv"
printf 'generated %s icons x %s frames x 2 sizes\n' "$icon_count" "$frame_count"
