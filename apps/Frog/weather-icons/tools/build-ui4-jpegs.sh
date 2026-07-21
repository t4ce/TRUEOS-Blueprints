#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
icon_root=$(cd -- "$script_dir/.." && pwd)
output_root="$icon_root/ui4-jpeg"
output_64="$output_root/64"
output_128="$output_root/128"
frame_count=${UI4_WEATHER_FRAME_COUNT:-10}
background=${UI4_WEATHER_BACKGROUND:-142238}
capture_size=256

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
    strip_width=$((frame_count * capture_size))
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
        --window-size="$strip_width,$capture_size" \
        --screenshot="$strip" \
        "$capture_url" >/dev/null 2>&1

    dimensions=$(magick identify -format '%wx%h' "$strip")
    if [[ "$dimensions" != "${strip_width}x${capture_size}" ]]; then
        echo "$icon capture has unexpected dimensions $dimensions" >&2
        exit 1
    fi

    # Derive one content rectangle for the complete sequence. Every frame is
    # normalized with this same union, preserving motion while removing the
    # very large and inconsistent internal padding in the source SVGs.
    union_left=$capture_size
    union_top=$capture_size
    union_right=0
    union_bottom=0
    for ((frame_index = 0; frame_index < frame_count; frame_index += 1)); do
        frame=$(printf '%03d' "$frame_index")
        x=$((frame_index * capture_size))
        cell="$work_dir/$icon-cell-$frame.png"
        magick "$strip" \
            -crop "${capture_size}x${capture_size}+$x+0" +repage \
            "$cell"
        bounds=$(magick "$cell" -fuzz 4% -trim -format '%wx%h%O' info: 2>/dev/null || true)
        if [[ "$bounds" =~ ^([0-9]+)x([0-9]+)\+([0-9]+)\+([0-9]+)$ ]]; then
            content_width=${BASH_REMATCH[1]}
            content_height=${BASH_REMATCH[2]}
            content_x=${BASH_REMATCH[3]}
            content_y=${BASH_REMATCH[4]}
            if ((content_width > 1 && content_height > 1)); then
                ((content_x < union_left)) && union_left=$content_x
                ((content_y < union_top)) && union_top=$content_y
                content_right=$((content_x + content_width))
                content_bottom=$((content_y + content_height))
                ((content_right > union_right)) && union_right=$content_right
                ((content_bottom > union_bottom)) && union_bottom=$content_bottom
            fi
        fi
    done
    if ((union_right <= union_left || union_bottom <= union_top)); then
        union_left=0
        union_top=0
        union_right=$capture_size
        union_bottom=$capture_size
    else
        padding=8
        union_left=$((union_left > padding ? union_left - padding : 0))
        union_top=$((union_top > padding ? union_top - padding : 0))
        union_right=$((union_right + padding < capture_size ? union_right + padding : capture_size))
        union_bottom=$((union_bottom + padding < capture_size ? union_bottom + padding : capture_size))
    fi
    for ((frame_index = 0; frame_index < frame_count; frame_index += 1)); do
        frame=$(printf '%03d' "$frame_index")
        output_name="$icon-frame-$frame.jpg"
        cell="$work_dir/$icon-cell-$frame.png"
        normalized="$work_dir/$icon-normalized-$frame.png"
        crop_left=$union_left
        crop_top=$union_top
        crop_right=$union_right
        crop_bottom=$union_bottom
        if ((frame_index == 0)); then
            # Frame zero is the deliberately static bring-up image. Fit it
            # tightly on its own; animated frames continue to share the union
            # bounds above so their relative motion remains stable.
            bounds=$(magick "$cell" -fuzz 4% -trim -format '%wx%h%O' info: 2>/dev/null || true)
            if [[ "$bounds" =~ ^([0-9]+)x([0-9]+)\+([0-9]+)\+([0-9]+)$ ]]; then
                content_width=${BASH_REMATCH[1]}
                content_height=${BASH_REMATCH[2]}
                content_x=${BASH_REMATCH[3]}
                content_y=${BASH_REMATCH[4]}
                if ((content_width > 1 && content_height > 1)); then
                    padding=8
                    crop_left=$((content_x > padding ? content_x - padding : 0))
                    crop_top=$((content_y > padding ? content_y - padding : 0))
                    content_right=$((content_x + content_width))
                    content_bottom=$((content_y + content_height))
                    crop_right=$((content_right + padding < capture_size ? content_right + padding : capture_size))
                    crop_bottom=$((content_bottom + padding < capture_size ? content_bottom + padding : capture_size))
                fi
            fi
        fi
        crop_width=$((crop_right - crop_left))
        crop_height=$((crop_bottom - crop_top))

        magick "$cell" \
            -crop "${crop_width}x${crop_height}+$crop_left+$crop_top" +repage \
            -filter Lanczos \
            -resize 112x96 \
            -gravity center \
            -background "#$background" \
            -extent 128x128 \
            "$normalized"
        magick "$normalized" \
            -alpha off \
            -colorspace sRGB \
            -sampling-factor 4:4:4 \
            -quality 90 \
            -strip \
            "$output_128/$output_name"
        magick "$normalized" \
            -filter Lanczos \
            -resize 64x64 \
            -alpha off \
            -colorspace sRGB \
            -sampling-factor 4:4:4 \
            -quality 90 \
            -strip \
            "$output_64/$output_name"

        if ((frame_index == 0)); then
            magick "$normalized" \
                -alpha set \
                -channel A -evaluate set 100% +channel \
                -depth 8 \
                "rgba:$output_128/$icon-frame-000.rgba"
            magick "$normalized" \
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
