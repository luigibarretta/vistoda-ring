//! Camera SDP is forwarded unchanged: encrypted media travels Ring to browser.
use crate::{BridgeError, ring_audio::AudioSessionRequest};

const SDP_LIMIT: usize = 64 * 1024;

pub fn validate_request(request: &AudioSessionRequest) -> Result<(), BridgeError> {
    if request.ice_gathering_ms.is_some_and(|value| value > 60_000) {
        return Err(invalid());
    }
    crate::ring_expected_device::parse_id(
        request.expected_device_id.as_deref().ok_or_else(invalid)?,
    )?;
    validate_sdp(&request.offer_sdp, false).map_err(|()| invalid())
}

pub fn validate_answer(sdp: &str) -> Result<(), BridgeError> {
    validate_sdp(sdp, true).map_err(|()| {
        BridgeError::Protocol("Ring camera did not negotiate H264 video and PCMU audio".into())
    })
}

fn validate_sdp(sdp: &str, answer: bool) -> Result<(), ()> {
    if sdp.is_empty() || sdp.len() > SDP_LIMIT || sdp.contains('\0') {
        return Err(());
    }
    let mut lines = sdp.lines().map(str::trim);
    if lines.next() != Some("v=0") {
        return Err(());
    }
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for line in lines {
        if line.starts_with("m=") {
            sections.push(vec![line]);
        } else if let Some(section) = sections.last_mut() {
            section.push(line);
        }
    }
    if sections.len() != 2 {
        return Err(());
    }
    let audio = section(&sections, "m=audio ")?;
    let video = section(&sections, "m=video ")?;
    let audio_payloads = media_payloads(audio[0])?;
    let video_payloads = media_payloads(video[0])?;
    if !audio_payloads.contains(&"0") || !direction(audio, &["a=sendrecv"]) {
        return Err(());
    }
    let video_direction = if answer { "a=sendonly" } else { "a=recvonly" };
    if !direction(video, &[video_direction]) {
        return Err(());
    }
    if !video.iter().any(|line| {
        line.strip_prefix("a=rtpmap:")
            .and_then(|value| value.split_once(' '))
            .is_some_and(|(payload, codec)| {
                video_payloads.contains(&payload) && codec.eq_ignore_ascii_case("H264/90000")
            })
    }) {
        return Err(());
    }
    Ok(())
}

fn section<'a>(sections: &'a [Vec<&'a str>], prefix: &str) -> Result<&'a [&'a str], ()> {
    let mut matching = sections
        .iter()
        .filter(|section| section[0].starts_with(prefix));
    let first = matching.next().ok_or(())?;
    if matching.next().is_some() {
        return Err(());
    }
    Ok(first)
}

fn media_payloads(line: &str) -> Result<Vec<&str>, ()> {
    let parts: Vec<_> = line.split_ascii_whitespace().collect();
    if parts.len() < 4
        || parts[1].parse::<u16>().ok().is_none_or(|port| port == 0)
        || parts[2] != "UDP/TLS/RTP/SAVPF"
    {
        return Err(());
    }
    Ok(parts[3..].to_vec())
}

fn direction(section: &[&str], allowed: &[&str]) -> bool {
    let values: Vec<_> = section
        .iter()
        .filter(|line| {
            matches!(
                **line,
                "a=sendrecv" | "a=recvonly" | "a=sendonly" | "a=inactive"
            )
        })
        .collect();
    values.len() == 1 && allowed.contains(values[0])
}

fn invalid() -> BridgeError {
    BridgeError::InvalidRequest(
        "camera offer requires H264 recvonly video, PCMU sendrecv audio and an exact device ID"
            .into(),
    )
}
