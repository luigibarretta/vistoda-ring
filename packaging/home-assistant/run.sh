#!/bin/sh
set -eu

readonly data_dir=/data
readonly options_file=/data/options.json
readonly token_file=/data/api-token
readonly devices_file=/data/devices.json
readonly storage_marker=/data/recording-storage
. /usr/local/lib/vistoda-app-bootstrap
. /usr/local/lib/vistoda-recording-storage

umask 077
vistoda_require_supervisor_token
app_info="$(vistoda_supervisor_app_info)"
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
vistoda_prepare_data_dir bridge:bridge "${data_dir}"

alias_name="$(jq -er '.alias | strings | select(test("^[A-Za-z0-9_-]+$"))' "${options_file}")"
vistoda_ensure_hex_token "${token_file}" bridge:bridge ''
jq -n --arg alias "${alias_name}" \
    '{($alias): {kind: "ring_intercom_audio"}}' >"${devices_file}"
chown bridge:bridge "${devices_file}"
chown -R bridge:bridge "${recording_dir}"
chmod 0700 "${recording_dir}"
vistoda_secure_file bridge:bridge "${data_dir}/ring-session.json"
vistoda_secure_file bridge:bridge "${data_dir}/ring-push.json"
chmod 0600 "${devices_file}"
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

vistoda_start_child gosu bridge:bridge ring-intercom-bridge serve
vistoda_wait_for_health http://127.0.0.1:8775/healthz 30 1

private_url="http://${app_hostname}:8775"
jq -n \
    --arg service media_bridge \
    --arg provider ring \
    --arg url "${private_url}" \
    --arg alias "${alias_name}" \
    --rawfile api_token "${token_file}" \
    '{service: $service, config: {provider: $provider, url: $url,
      alias: $alias, api_token: ($api_token | gsub("\\s"; "")), managed_app: true}}' |
    vistoda_publish_discovery

vistoda_wait_child
