# Native Ring camera live video

The camera path discovers native Ring doorbells and cameras from `doorbots`,
`stickup_cams` and `authorized_doorbots`. It does not depend on a Home Assistant
camera entity. Intercom Audio/Video and ONVIF devices remain separate.

`GET /v1/cameras` returns at most 32 minimal identities, with decimal string
device IDs, model, name, optional location name and media capabilities. Duplicate
IDs, camera/Intercom ID collisions and oversized inventories fail closed.
Discovery never obtains a stream ticket or starts a live view.

`POST /v1/cameras/{device_id}/video/sessions` accepts an exact matching
`expected_device_id`, a browser SDP offer, `mode` (`listen` or `talk`) and optional
`ice_gathering_ms`. SDP requires one H264 recvonly video section and one PCMU
sendrecv audio section. The response includes `session_id`, unchanged answer SDP,
ICE candidates with their original media-line indices, mode and 120-second TTL.
The consumer must gather its own ICE into the offer before posting it.

The provider forwards signaling only. Encrypted H264/PCMU RTP flows directly
between the browser and Ring; no video bytes enter the audio-only PCM WebSocket
relay. Codec payload numbering, timestamps, sequence numbers, marker bits and
RTCP feedback are negotiated by the peers, without provider rewriting. NAT or
firewall compatibility of this direct path still needs device acceptance.

The consumer owns microphone permission and supplies silence in listen mode.
Only an explicit live action starts media. The consumer must call
`DELETE /v1/cameras/{device_id}/video/sessions/{session_id}` with matching
`expected_device_id` on close, disconnect or expiry. One active session per camera
and a ten-second cooldown use the existing bounded session manager. Another
camera's route cannot stop a session. Teardown works without Ring discovery.

For camera-only standalone accounts, configure an empty devices map (`{}`).
Existing Intercom aliases and their unlock, history, recording and audio paths
remain supported. The Home Assistant add-on retains its legacy bootstrap alias;
consumers must use native inventory rather than treating that alias as a camera.
Camera recordings, snapshots, camera settings and Ring Edge are not implemented.

## Evidence and limits

Camera capabilities remain `protocol_research`. The available owned device is
Intercom Audio; no Ring camera hardware, provider call or unlock was used during
development. Tests cover mixed discovery, exact device grants, bounded SDP,
session ownership/teardown and a local WebSocket exchange carrying unchanged
H264 SDP and video ICE. This verifies the signaling implementation, not camera
firmware compatibility or decoded hardware video.

Protocol observations were checked on 2026-09-13 against pinned public upstream
source, with no proprietary APK analysis or account traffic acquisition:

- [Inventory categories](https://github.com/dgreif/ring/blob/638d5285aea5f34d44d9bacbb41917f736764d49e/packages/ring-client-api/api.ts)
- [Cloud WebRTC signaling](https://github.com/dgreif/ring/blob/638d5285aea5f34d44d9bacbb41917f736764d49e/packages/ring-client-api/streaming/webrtc-connection.ts)

The implementation extends the provider's existing native signaling contract;
these public sources document compatibility behavior and are not vendor approval.
