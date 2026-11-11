//! The seam between [`crate::Speech`] and a platform: the settings and queries, and utterances
//! that each settle exactly once through their events.

use std::{future::Future, pin::Pin};

use crate::{
    error::SpeechError,
    options::{Engine, UtteranceEvent, Voice},
};

/// A future a backend returns. Each call's future borrows the backend.
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Receives one utterance's events: `start`, any number of `progress`, then exactly one terminal
/// (`finish`, `cancel` or `error`). Sent into `speak`; called from the platform's own thread.
pub type EventSink = Box<dyn Fn(UtteranceEvent) + Send>;

/// What a platform provides. [`crate::Speech`] adds the rules: argument checks, one completion per
/// awaited `speak`, and pause only while speaking.
///
/// A backend owns the queue: `speak` starts its utterance at once, or queues it when one runs and
/// `interrupt` is false (cancelling the running one first when `interrupt` is true). The backend
/// assigns each utterance an id, returns it from `speak`, and stamps it into every event it fires
/// for that utterance; `None` is for a platform that gives no id, and `crate::Speech` numbers the
/// utterance itself. Every call's future resolves exactly once: with its value, a coded
/// [`SpeechError`], or the utterance's terminal event. A backend never panics and never hangs.
pub trait Backend: Send + Sync {
    /// Prepares the platform's speech. `EngineMissing` when the device has no engine,
    /// `NotAvailable` when speech exists but cannot run here.
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Whether the platform has a speech service to run.
    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>>;

    /// Starts (or queues) one utterance, returning its id. Events reach `on_event`.
    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>>;

    /// Cancels the running utterance; its sink gets a `cancel` event. Queued utterances are kept.
    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Pauses the running utterance at its next opportunity.
    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Resumes a paused utterance.
    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Whether an utterance is being spoken, now or from the queue.
    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>>;

    /// Synthesizes `text` to WAV bytes, without speaking it.
    fn synthesize(&self, text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>>;

    /// The voices the device offers.
    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>>;

    /// The installed engines. Desktops and Apple report one.
    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>>;

    /// The languages a voice exists for, as BCP 47 tags.
    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>>;

    /// The longest text the platform accepts, when it reports a limit.
    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>>;

    /// Sets the language of later utterances.
    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Sets the voice of later utterances.
    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Sets the rate of later utterances (0.0 to 1.0).
    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Sets the pitch of later utterances (0.5 to 2.0).
    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Sets the volume of later utterances (0.0 to 1.0).
    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>>;

    /// Sets the silence before each utterance, in milliseconds; 0 starts immediately.
    fn set_silence(&self, ms: u32) -> BackendFuture<'_, Result<(), SpeechError>>;
}
