#!/usr/bin/env bash

package_linux_binary() {
    local repository_root="$1"
    local binary_path="$2"
    local package_directory="$3"
    local app_icon="$4"
    local icon_directory="${package_directory}/share/icons/hicolor/1024x1024/apps"
    local desktop_directory="${package_directory}/share/applications"

    mkdir -p -- "${icon_directory}" "${desktop_directory}"
    cp -- "${binary_path}" "${package_directory}/"
    cp -- "${app_icon}" "${icon_directory}/io.github.ivangor246.gen_music_ai.png"
    cp -- "${repository_root}/packaging/linux/io.github.ivangor246.gen_music_ai.desktop" \
        "${desktop_directory}/"
}

package_windows_binary() {
    local repository_root="$1"
    local binary_path="$2"
    local package_directory="$3"

    cp -- "${binary_path}" "${package_directory}/"
    cp -- "${repository_root}/packaging/windows/app-icon.ico" \
        "${package_directory}/gen_music_ai.ico"
}

package_macos_binary() {
    local repository_root="$1"
    local binary_path="$2"
    local package_directory="$3"
    local app_icon="$4"
    local version="$5"
    local temporary_directory="$6"
    local bundle_version="${version%%-*}"
    local app_directory="${package_directory}/Gen Music AI.app/Contents"
    local macos_directory="${app_directory}/MacOS"
    local resources_directory="${app_directory}/Resources"
    local iconset_directory="${temporary_directory}/AppIcon.iconset"

    mkdir -p -- "${macos_directory}" "${resources_directory}" "${iconset_directory}"
    cp -- "${binary_path}" "${macos_directory}/gen_music_ai"
    sed "s/@VERSION@/${bundle_version}/g" \
        "${repository_root}/packaging/macos/Info.plist" \
        >"${app_directory}/Info.plist"
    while read -r size filename; do
        sips --resampleHeightWidth "${size}" "${size}" "${app_icon}" \
            --out "${iconset_directory}/${filename}" >/dev/null
    done <<'EOF'
16 icon_16x16.png
32 icon_16x16@2x.png
32 icon_32x32.png
64 icon_32x32@2x.png
128 icon_128x128.png
256 icon_128x128@2x.png
256 icon_256x256.png
512 icon_256x256@2x.png
512 icon_512x512.png
1024 icon_512x512@2x.png
EOF
    iconutil --convert icns "${iconset_directory}" \
        --output "${resources_directory}/AppIcon.icns"
}

package_platform_binary() {
    local target="$1"
    local repository_root="$2"
    local binary_path="$3"
    local package_directory="$4"
    local app_icon="$5"
    local version="$6"
    local temporary_directory="$7"

    case "${target}" in
        x86_64-unknown-linux-gnu)
            package_linux_binary \
                "${repository_root}" "${binary_path}" "${package_directory}" "${app_icon}"
            ;;
        x86_64-pc-windows-msvc)
            package_windows_binary "${repository_root}" "${binary_path}" "${package_directory}"
            ;;
        x86_64-apple-darwin)
            package_macos_binary \
                "${repository_root}" \
                "${binary_path}" \
                "${package_directory}" \
                "${app_icon}" \
                "${version}" \
                "${temporary_directory}"
            ;;
        *)
            echo "Unsupported release target: ${target}" >&2
            return 1
            ;;
    esac
}
