#!/bin/sh

storage_directory() {
    case "$1" in
        private) printf '%s\n' /data/recordings ;;
        addon_config) printf '%s\n' /config/recordings ;;
        media) printf '%s\n' /media/vistoda-ring ;;
        share) printf '%s\n' /share/vistoda-ring ;;
        *) return 1 ;;
    esac
}

migrate_recordings() {
    source_dir="$1"
    target_dir="$2"
    test "${source_dir}" != "${target_dir}" || return 0
    mkdir -p "${source_dir}" "${target_dir}"
    for source_file in "${source_dir}"/*; do
        test -e "${source_file}" || continue
        test -f "${source_file}" && ! test -L "${source_file}" || return 1
        file_name="${source_file##*/}"
        printf '%s' "${file_name}" | grep -Eq \
            '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.(json|mp4|webm)$' || return 1
        target_file="${target_dir}/${file_name}"
        if test -e "${target_file}"; then
            test -f "${target_file}" && ! test -L "${target_file}" || return 1
        else
            cp -p "${source_file}" "${target_file}"
        fi
        cmp -s "${source_file}" "${target_file}" || return 1
    done
    for source_file in "${source_dir}"/*; do
        test -e "${source_file}" || continue
        rm "${source_file}"
    done
}
