#!/bin/sh

storage_directory() {
    case "$1" in
        private) printf '%s\n' /data/recordings ;;
        addon_config) printf '%s\n' /config/recordings ;;
        media) printf '%s\n' /media/vistoda-ring ;;
        share) printf '%s\n' /share/vistoda-ring ;;
        network)
            validate_network_mount "${2:-}" || return 1
            printf '%s/vistoda-ring\n' "$2"
            ;;
        *) return 1 ;;
    esac
}

validate_network_mount() {
    test "${#1}" -le 160 || return 1
    printf '%s\n' "$1" | grep -Eq \
        '^/(media|share)/[A-Za-z0-9][A-Za-z0-9._ -]{0,127}$'
}

require_live_network_mount() {
    mount_path="$1"
    validate_network_mount "${mount_path}" || {
        printf '%s\n' 'Network storage must be /media/<name> or /share/<name>.' >&2
        return 1
    }
    test -d "${mount_path}" || {
        printf 'HAOS network storage is absent: %s\n' "${mount_path}" >&2
        return 1
    }
    parent_path="${mount_path%/*}"
    test "$(stat -c %d "${mount_path}")" != "$(stat -c %d "${parent_path}")" || {
        printf 'Path is local, not a live HAOS network storage: %s\n' "${mount_path}" >&2
        return 1
    }
}

storage_directory_from_marker() {
    case "$1" in
        network\|*) storage_directory network "${1#network|}" ;;
        *) storage_directory "$1" ;;
    esac
}

storage_marker_value() {
    if test "$1" = network; then
        validate_network_mount "${2:-}" || return 1
        printf 'network|%s\n' "$2"
    else
        storage_directory "$1" >/dev/null
        printf '%s\n' "$1"
    fi
}

storage_api_kind() {
    if test "$1" != network; then
        printf '%s\n' "$1"
    elif printf '%s\n' "$2" | grep -q '^/media/'; then
        printf '%s\n' media
    else
        printf '%s\n' share
    fi
}

# Subshells keep recursive traversal state separate in both dash and BusyBox.
copy_archive_tree() (
    source_dir="$1"
    target_dir="$2"
    depth="$3"
    ! test -L "${source_dir}" && ! test -L "${target_dir}" || return 1
    test -d "${source_dir}" || return 1
    mkdir -p "${target_dir}" || return 1
    for source_file in "${source_dir}"/* "${source_dir}"/.[!.]* "${source_dir}"/..?*; do
        ! test -L "${source_file}" || return 1
        test -e "${source_file}" || continue
        file_name="${source_file##*/}"
        target_file="${target_dir}/${file_name}"
        if test -d "${source_file}"; then
            test "${depth}" -eq 0 || return 1
            printf '%s' "${file_name}" | grep -Eq '^device-[1-9][0-9]{0,19}$' || return 1
            copy_archive_tree "${source_file}" "${target_file}" 1 || return 1
            continue
        fi
        test -f "${source_file}" || return 1
        printf '%s' "${file_name}" | grep -Eq \
            '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.(json|mp4|webm)$' || return 1
        ! test -L "${target_file}" || return 1
        if test -e "${target_file}"; then
            test -f "${target_file}" || return 1
        else
            cp -p "${source_file}" "${target_file}" || return 1
        fi
        cmp -s "${source_file}" "${target_file}" || return 1
    done
)

retire_archive_tree() (
    source_dir="$1"
    target_dir="$2"
    depth="$3"
    ! test -L "${source_dir}" && ! test -L "${target_dir}" || return 1
    for source_file in "${source_dir}"/* "${source_dir}"/.[!.]* "${source_dir}"/..?*; do
        ! test -L "${source_file}" || return 1
        test -e "${source_file}" || continue
        file_name="${source_file##*/}"
        target_file="${target_dir}/${file_name}"
        if test -d "${source_file}"; then
            test "${depth}" -eq 0 || return 1
            printf '%s' "${file_name}" | grep -Eq '^device-[1-9][0-9]{0,19}$' || return 1
            retire_archive_tree "${source_file}" "${target_file}" 1 || return 1
            rmdir "${source_file}" || return 1
        else
            test -f "${source_file}" && ! test -L "${target_file}" || return 1
            printf '%s' "${file_name}" | grep -Eq \
                '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.(json|mp4|webm)$' || return 1
            # A writer racing the stopped provider cannot make us discard new bytes.
            cmp -s "${source_file}" "${target_file}" || return 1
            rm "${source_file}" || return 1
        fi
    done
)

migrate_recordings() (
    source_dir="$1"
    target_dir="$2"
    ! test -L "${source_dir}" && ! test -L "${target_dir}" || return 1
    mkdir -p "${source_dir}" "${target_dir}" || return 1
    test "$(stat -c '%d:%i' "${source_dir}")" != "$(stat -c '%d:%i' "${target_dir}")" || return 0
    # Complete every directory copy before retiring any source file. Legacy
    # flat files remain flat: never guess which physical entrance owns them.
    copy_archive_tree "${source_dir}" "${target_dir}" 0 || return 1
    retire_archive_tree "${source_dir}" "${target_dir}" 0
)
