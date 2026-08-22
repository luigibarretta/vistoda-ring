use crate::error::BridgeError;

pub const MAX_MEDIA_BYTES: usize = 8 * 1024 * 1024;
pub const MIN_MEDIA_BYTES: usize = 128;

#[derive(Clone, Copy)]
pub enum RecordingMedia {
    Mp4,
    Webm,
}

impl RecordingMedia {
    pub fn parse(value: &str) -> Result<Self, BridgeError> {
        match value
            .split(';')
            .next()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("audio/mp4") => Ok(Self::Mp4),
            Some("audio/webm") => Ok(Self::Webm),
            _ => Err(BridgeError::InvalidRequest(
                "recording content type must be audio/mp4 or audio/webm".into(),
            )),
        }
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Mp4 => "audio/mp4",
            Self::Webm => "audio/webm",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    pub fn validate(self, media: &[u8]) -> Result<(), BridgeError> {
        if !(MIN_MEDIA_BYTES..=MAX_MEDIA_BYTES).contains(&media.len()) {
            return Err(BridgeError::InvalidRequest(
                "recording size is outside the bounded archive limit".into(),
            ));
        }
        let signature_valid = match self {
            Self::Mp4 => media.get(4..8) == Some(b"ftyp".as_slice()),
            Self::Webm => media.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        };
        if !signature_valid {
            return Err(BridgeError::InvalidRequest(
                "recording container signature is invalid".into(),
            ));
        }
        Ok(())
    }
}
