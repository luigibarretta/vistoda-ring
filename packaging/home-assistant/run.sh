#!/bin/sh
set -eu

readonly data_dir=/data
readonly options_file=/data/options.json
readonly token_file=/data/api-token
readonly devices_file=/data/devices.json
readonly storage_marker=/data/recording-storage
. /usr/local/lib/vistoda-recording-storage

umask 077
test -n "${SUPERVISOR_TOKEN:-}" || exit 1
app_info="$(curl -fsS --retry 5 --retry-all-errors \
    -H "Authorization: Bearer ${SUPERVISOR_TOKEN}" \
    http://supervisor/addons/self/info)"
app_hostname="$(printf '%s' "${app_info}" | jq -er '.data.hostname')"
app_slug="$(printf '%s' "${app_info}" | jq -er '.data.slug')"
storage_choice="$(jq -er '(.recording_storage // "private") | strings |
    select(. == "private" or . == "addon_config" or . == "media" or . == "share"
      or . == "network")' \
    "${options_file}")"
network_mount="$(jq -er '(.recording_network_mount // "") | strings' "${options_file}")"
if test "${storage_choice}" = network; then
    require_live_network_mount "${network_mount}"
fi
recording_dir="$(storage_directory "${storage_choice}" "${network_mount}")"
case "${storage_choice}" in
    private) recording_display_dir=/data/recordings ;;
    addon_config) recording_display_dir="/addon_configs/${app_slug}/recordings" ;;
    media) recording_display_dir=/media/vistoda-ring ;;
    share) recording_display_dir=/share/vistoda-ring ;;
    network) recording_display_dir="${recording_dir}" ;;
esac
recording_storage_kind="$(storage_api_kind "${storage_choice}" "${network_mount}")"
previous_storage="$(test -f "${storage_marker}" && cat "${storage_marker}" || printf private)"
case "${previous_storage}" in
    network\|*) require_live_network_mount "${previous_storage#network|}" ;;
esac
previous_dir="$(storage_directory_from_marker "${previous_storage}")"
migrate_recordings "${previous_dir}" "${recording_dir}"
mkdir -p "${recording_dir}"
chown bridge:bridge "${data_dir}"

alias_name="$(jq -er '.alias | strings | select(test("^[A-Za-z0-9_-]+$"))' "${options_file}")"
if ! test -f "${token_file}" || ! grep -Eq '^[0-9a-f]{64}$' "${token_file}"; then
    od -An -N32 -tx1 /dev/urandom | tr -d ' \n' >"${token_file}"
fi
jq -n --arg alias "${alias_name}" \
    '{($alias): {kind: "ring_intercom_audio"}}' >"${devices_file}"
chown bridge:bridge "${token_file}" "${devices_file}"
chown -R bridge:bridge "${recording_dir}"
chmod 0700 "${recording_dir}"
if test -e "${data_dir}/ring-session.json"; then
    chown bridge:bridge "${data_dir}/ring-session.json"
    chmod 0600 "${data_dir}/ring-session.json"
fi
if test -e "${data_dir}/ring-push.json"; then
    chown bridge:bridge "${data_dir}/ring-push.json"
    chmod 0600 "${data_dir}/ring-push.json"
fi
chmod 0600 "${token_file}" "${devices_file}"
storage_marker_value "${storage_choice}" "${network_mount}" >"${storage_marker}.new"
chmod 0600 "${storage_marker}.new"
mv "${storage_marker}.new" "${storage_marker}"

export RING_INTERCOM_API_TOKEN_FILE="${token_file}"
export RING_INTERCOM_DEVICES_FILE="${devices_file}"
export RING_INTERCOM_SESSION_FILE="${data_dir}/ring-session.json"
export RING_INTERCOM_PUSH_FILE="${data_dir}/ring-push.json"
export RING_INTERCOM_RECORDING_DIR="${recording_dir}"
export RING_INTERCOM_RECORDING_DISPLAY_DIR="${recording_display_dir}"
export RING_INTERCOM_RECORDING_STORAGE_KIND="${recording_storage_kind}"

gosu bridge:bridge ring-intercom-bridge serve &
child_pid=$!

stop_child() {
    kill -TERM "${child_pid}" 2>/dev/null || true
    wait "${child_pid}" 2>/dev/null || true
}
trap stop_child INT TERM

attempt=0
until curl -fsS --max-time 2 http://127.0.0.1:8775/healthz >/dev/null 2>&1; do
    if ! kill -0 "${child_pid}" 2>/dev/null; then
        wait "${child_pid}"
    fi
    attempt=$((attempt + 1))
    test "${attempt}" -lt 30 || exit 1
    sleep 1
done

private_url="http://${app_hostname}:8775"
jq -n \
    --arg service media_bridge \
    --arg provider ring \
    --arg url "${private_url}" \
    --arg alias "${alias_name}" \
    --rawfile api_token "${token_file}" \
    '{service: $service, config: {provider: $provider, url: $url,
      alias: $alias, api_token: ($api_token | gsub("\\s"; "")), managed_app: true}}' |
    curl -fsS --retry 5 --retry-all-errors \
        -H "Authorization: Bearer ${SUPERVISOR_TOKEN}" \
        -H 'Content-Type: application/json' \
        --data-binary @- http://supervisor/discovery >/dev/null

wait "${child_pid}"
