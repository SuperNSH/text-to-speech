//! The voices, engines and events speech reports.

use crate::error::SpeechError;

/// One utterance's id, as `speak` returned it and as its events carry it.
pub type UtteranceId = u64;

/// A voice the device can speak with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// The id `set_voice` takes, stable for this device.
    pub id: String,
    /// Its name, as the system settings show it.
    pub name: String,
    /// Its language, a BCP 47 tag (`en-US`).
    pub language: String,
    /// Whether it needs the network; such a voice fails offline.
    pub network_required: bool,
    /// Whether it is installed. An uninstalled voice cannot be set.
    pub installed: bool,
}

/// A text-to-speech engine (Android's notion; other platforms report one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    /// Its package name (`com.google.android.tts`).
    pub name: String,
    /// Its display name.
    pub label: String,
    /// Whether the system picks it by default.
    pub default: bool,
}

/// What happened to one utterance, in this order: [`Self::Start`], any number of
/// [`Self::Progress`]es, then exactly one of [`Self::Finish`], [`Self::Cancel`] or [`Self::Error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtteranceEventKind {
    /// The utterance began.
    Start,
    /// The utterance passed `location` of `length` characters.
    Progress { location: u64, length: u64 },
    /// The utterance ended normally.
    Finish,
    /// The utterance was stopped or interrupted.
    Cancel,
    /// The utterance failed.
    Error(SpeechError),
}

/// One report about one utterance, as the event handler receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UtteranceEvent {
    /// The utterance it is about, the id `speak` returned.
    pub utterance_id: UtteranceId,
    /// What happened.
    pub kind: UtteranceEventKind,
}
