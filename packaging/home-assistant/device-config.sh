#!/bin/sh
# Build explicit alias/device bindings without guessing physical destinations.

ring_device_config() {
    jq -e '
      def valid_alias: type == "string" and test("^[A-Za-z0-9_-]{1,64}$");
      def valid_id: type == "number" and . >= 1 and . <= 9007199254740991 and floor == .;
      (.intercoms // []) as $intercoms |
      if ($intercoms | type) != "array" or ($intercoms | length) > 32 then
        error("intercoms must be a list with at most 32 entrances")
      elif ($intercoms | length) == 0 then
        if (.alias | valid_alias) then {(.alias): {kind: "ring_intercom_audio"}}
        else error("enter a stable single-device alias") end
      elif all($intercoms[]; (.alias | valid_alias) and (.device_id | valid_id)) | not then
        error("each intercom requires a stable alias and positive numeric device_id")
      elif ($intercoms | map(.alias) | unique | length) != ($intercoms | length) or
           ($intercoms | map(.device_id) | unique | length) != ($intercoms | length) then
        error("intercom aliases and device_id bindings must be unique")
      else $intercoms | map({key: .alias, value: {kind: "ring_intercom_audio", device_id: .device_id}}) | from_entries
      end' "$1"
}

# Discovery is a bounded, allowlisted projection; never publish account metadata.
ring_discovery_devices() {
    inventory_config=''
    if inventory_body="$(printf 'header = "Authorization: Bearer %s"\n' "$(tr -d '\r\n' <"$1")" |
        curl --config - --silent --fail --connect-timeout 2 --max-time 10 \
            --max-filesize 131072 http://127.0.0.1:8775/v1/intercoms 2>/dev/null)"; then
        inventory_config="$(printf '%s' "${inventory_body}" | jq -ce '
          def valid_alias: type == "string" and test("^[A-Za-z0-9_-]{1,64}$");
          def valid_id: type == "string" and test("^[1-9][0-9]{0,19}$") and
            (length < 20 or . <= "18446744073709551615");
          .intercoms | select(type == "array" and length > 0 and length <= 32) |
          select(all(.[]; (.alias | valid_alias) and (.device_id | valid_id))) |
          select((map(.alias) | unique | length) == length) |
          select((map(.device_id) | unique | length) == length) |
          map({key: .alias, value: {device_id: .device_id}}) | from_entries
        ' 2>/dev/null)" || inventory_config=''
    fi
    if test -n "${inventory_config}"; then
        printf '%s\n' "${inventory_config}"
    else
        jq -ce 'with_entries(.value |= {device_id:
          (if .device_id == null then null else (.device_id | tostring) end)})' "$2"
    fi
}
