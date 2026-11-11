//! The errors a speech call reports. Each has a stable code; a message says what to do.

/// What went wrong, as a stable code; see [`SpeechError::code`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// No speech service on this device or platform, or speech cannot run here.
    NotAvailable,
    /// No TTS engine is installed (Android: install one first).
    EngineMissing,
    /// The voice is unknown or not installed on this device.
    VoiceNotFound,
    /// The platform rejected the language tag.
    LanguageNotSupported,
    /// The language's voice data needs downloading (Android `LANG_MISSING_DATA`).
    LanguageMissingData,
    /// Pause or resume applied to an idle or unpaused synthesizer.
    NotSpeaking,
    /// An argument is empty or out of range, such as blank text or a rate past 1.0.
    InvalidArgument,
    /// A second concurrent `synthesize` while one runs.
    Busy,
    /// The utterance was stopped before it finished.
    Cancelled,
    /// The platform failed to synthesize (the web's `onerror`, a Windows synthesis fault).
    SynthesisFailed,
    /// The platform's speech service failed some other way; `status` holds its own code.
    Backend,
}

/// A failed speech call.
///
/// `Display` names the code and the message. Neither holds the spoken text or any user data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .kind.message())]
pub struct SpeechError {
    kind: ErrorKind,
    status: Option<i64>,
}

impl SpeechError {
    /// An error of `kind`, with no platform status.
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, status: None }
    }

    /// The same error, carrying the platform's own status code.
    #[must_use]
    pub fn with_status(mut self, status: i64) -> Self {
        self.status = Some(status);
        self
    }

    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The platform's status code, when the platform gave one.
    pub fn status(&self) -> Option<i64> {
        self.status
    }

    /// The stable code, such as `ERR_TTS_CANCELLED`. Apps match on this.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// What to do about it, in one sentence.
    pub fn message(&self) -> &'static str {
        self.kind.message()
    }
}

impl ErrorKind {
    /// The stable code of this kind.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::NotAvailable => "ERR_TTS_NOT_AVAILABLE",
            ErrorKind::EngineMissing => "ERR_TTS_ENGINE_MISSING",
            ErrorKind::VoiceNotFound => "ERR_TTS_VOICE_NOT_FOUND",
            ErrorKind::LanguageNotSupported => "ERR_TTS_LANGUAGE_NOT_SUPPORTED",
            ErrorKind::LanguageMissingData => "ERR_TTS_LANGUAGE_MISSING_DATA",
            ErrorKind::NotSpeaking => "ERR_TTS_NOT_SPEAKING",
            ErrorKind::InvalidArgument => "ERR_TTS_INVALID_ARGUMENT",
            ErrorKind::Busy => "ERR_TTS_BUSY",
            ErrorKind::Cancelled => "ERR_TTS_CANCELLED",
            ErrorKind::SynthesisFailed => "ERR_TTS_SYNTHESIS_FAILED",
            ErrorKind::Backend => "ERR_TTS_BACKEND",
        }
    }

    /// What to do about it, in one sentence.
    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::NotAvailable => {
                "Speech is not available on this device or platform. Offer a visible fallback."
            }
            ErrorKind::EngineMissing => {
                "No text-to-speech engine is installed. Install one, then try again."
            }
            ErrorKind::VoiceNotFound => {
                "That voice is unknown or not installed. Pick one from voices()."
            }
            ErrorKind::LanguageNotSupported => {
                "The platform does not support that language tag. Pick one from languages()."
            }
            ErrorKind::LanguageMissingData => {
                "The language's voice data needs downloading in the system settings."
            }
            ErrorKind::NotSpeaking => "Nothing is speaking, or nothing is paused.",
            ErrorKind::InvalidArgument => {
                "An argument is empty or out of range, such as blank text or a rate past 1.0."
            }
            ErrorKind::Busy => "A synthesize is already running. Wait for its bytes.",
            ErrorKind::Cancelled => "The utterance was stopped before it finished.",
            ErrorKind::SynthesisFailed => "The platform failed to synthesize the text.",
            ErrorKind::Backend => "The platform's speech service failed.",
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_displays_as_its_code() {
        assert_eq!(ErrorKind::Cancelled.to_string(), "ERR_TTS_CANCELLED");
        assert_eq!(ErrorKind::Backend.to_string(), "ERR_TTS_BACKEND");
    }

    #[test]
    fn an_error_names_its_kind_message_and_code() {
        let error = SpeechError::new(ErrorKind::SynthesisFailed);
        assert_eq!(error.kind(), ErrorKind::SynthesisFailed);
        assert_eq!(error.code(), "ERR_TTS_SYNTHESIS_FAILED");
        assert_eq!(error.message(), ErrorKind::SynthesisFailed.message());
        assert_eq!(error.to_string(), ErrorKind::SynthesisFailed.message());
        assert_eq!(error.status(), None);
    }

    #[test]
    fn a_status_rides_along_without_changing_the_rest() {
        let plain = SpeechError::new(ErrorKind::Backend);
        let carrying = plain.clone().with_status(-2147483648);
        assert_eq!(carrying.status(), Some(-2147483648));
        assert_eq!(carrying.kind(), plain.kind());
        assert_eq!(carrying.code(), plain.code());
        assert_ne!(carrying, plain);
    }
}
