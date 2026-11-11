//! A backend with scripted answers. Tests use it, and so can an app that runs without an engine.
//!
//! The queue is the backend's own, as on a platform: `interrupt` cancels the running utterance,
//! otherwise the utterance waits. The test finishes each utterance, as the system would.

use std::{
    collections::VecDeque,
    future::ready,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use tokio::sync::Notify;

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice},
};

/// A backend whose answers the test sets, and whose utterances the test finishes.
///
/// Clones share one state.
#[derive(Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

/// One utterance the fake holds: started (pending) or waiting in its queue.
struct Pending {
    id: UtteranceId,
    sink: EventSink,
}

struct FakeState {
    init: Result<(), SpeechError>,
    available: Result<bool, SpeechError>,
    next_speak_error: Option<SpeechError>,
    pending: Option<Pending>,
    queue: VecDeque<Pending>,
    next_id: UtteranceId,
    synth: Result<Vec<u8>, SpeechError>,
    synth_hold: Option<Arc<Notify>>,
    voices: Result<Vec<Voice>, SpeechError>,
    engines: Result<Vec<Engine>, SpeechError>,
    languages: Vec<String>,
    max_len: Option<usize>,
    speaks: usize,
    stops: usize,
    pauses: usize,
    resumes: usize,
    synth_calls: usize,
    last_text: Option<String>,
    last_interrupt: Option<bool>,
    last_language: Option<String>,
    last_voice: Option<String>,
    last_rate: Option<f32>,
    last_pitch: Option<f32>,
    last_volume: Option<f32>,
    last_silence: Option<u32>,
}

impl Default for FakeBackend {
    /// A device with an engine, an installed en-US voice and a 4000-character limit.
    fn default() -> Self {
        Self::new()
    }
}

impl FakeBackend {
    /// A device with one engine, an installed en-US voice, a network voice that is not installed,
    /// and a 4000-character limit, as Android reports one.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                init: Ok(()),
                available: Ok(true),
                next_speak_error: None,
                pending: None,
                queue: VecDeque::new(),
                next_id: 1,
                synth: Ok(wav_header()),
                synth_hold: None,
                voices: Ok(vec![
                    Voice {
                        id: "fake-voice".to_owned(),
                        name: "Fake Voice".to_owned(),
                        language: "en-US".to_owned(),
                        network_required: false,
                        installed: true,
                    },
                    Voice {
                        id: "fake-net-voice".to_owned(),
                        name: "Fake Network Voice".to_owned(),
                        language: "en-GB".to_owned(),
                        network_required: true,
                        installed: false,
                    },
                ]),
                engines: Ok(vec![Engine {
                    name: "com.fake.tts".to_owned(),
                    label: "Fake TTS".to_owned(),
                    default: true,
                }]),
                languages: vec!["en-US".to_owned(), "en-GB".to_owned()],
                max_len: Some(4000),
                speaks: 0,
                stops: 0,
                pauses: 0,
                resumes: 0,
                synth_calls: 0,
                last_text: None,
                last_interrupt: None,
                last_language: None,
                last_voice: None,
                last_rate: None,
                last_pitch: None,
                last_volume: None,
                last_silence: None,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets what `init` answers.
    pub fn set_init(&self, answer: Result<(), SpeechError>) {
        self.lock().init = answer;
    }

    /// Sets what `is_available` answers.
    pub fn set_available(&self, answer: Result<bool, SpeechError>) {
        self.lock().available = answer;
    }

    /// Makes the next `speak` fail to start with `error`, as a platform does when it cannot speak.
    pub fn fail_next_speak(&self, error: SpeechError) {
        self.lock().next_speak_error = Some(error);
    }

    /// Sets what `voices` answers.
    pub fn set_voices(&self, answer: Result<Vec<Voice>, SpeechError>) {
        self.lock().voices = answer;
    }

    /// Sets what `engines` answers.
    pub fn set_engines(&self, answer: Result<Vec<Engine>, SpeechError>) {
        self.lock().engines = answer;
    }

    /// Sets the languages the device offers. `set_language` rejects a tag outside it.
    pub fn set_languages(&self, languages: Vec<String>) {
        self.lock().languages = languages;
    }

    /// Sets the limit `max_speech_input_length` answers; `None` is no limit.
    pub fn set_max_speech_input_length(&self, max: Option<usize>) {
        self.lock().max_len = max;
    }

    /// Sets what `synthesize` answers.
    pub fn set_synthesize(&self, answer: Result<Vec<u8>, SpeechError>) {
        self.lock().synth = answer;
    }

    /// Holds the next `synthesize` until `release_synthesize`, so a second one can overlap it.
    pub fn hold_synthesize(&self) {
        self.lock().synth_hold = Some(Arc::new(Notify::new()));
    }

    /// Lets a held `synthesize` answer.
    pub fn release_synthesize(&self) {
        let hold = self.lock().synth_hold.take();
        if let Some(hold) = hold {
            hold.notify_one();
        }
    }

    /// How many utterances were given to `speak`.
    pub fn speaks(&self) -> usize {
        self.lock().speaks
    }

    /// How many times `stop` was called.
    pub fn stops(&self) -> usize {
        self.lock().stops
    }

    /// How many times `pause` was called.
    pub fn pauses(&self) -> usize {
        self.lock().pauses
    }

    /// How many times `resume` was called.
    pub fn resumes(&self) -> usize {
        self.lock().resumes
    }

    /// How many times `synthesize` was called.
    pub fn synth_calls(&self) -> usize {
        self.lock().synth_calls
    }

    /// The id of the running utterance.
    pub fn pending_id(&self) -> Option<UtteranceId> {
        self.lock().pending.as_ref().map(|p| p.id)
    }

    /// How many utterances wait in the queue.
    pub fn queued(&self) -> usize {
        self.lock().queue.len()
    }

    /// The text of the last `speak`.
    pub fn last_text(&self) -> Option<String> {
        self.lock().last_text.clone()
    }

    /// Whether the last `speak` asked to interrupt.
    pub fn last_interrupt(&self) -> Option<bool> {
        self.lock().last_interrupt
    }

    /// The tag of the last `set_language`.
    pub fn last_language(&self) -> Option<String> {
        self.lock().last_language.clone()
    }

    /// The id of the last `set_voice`.
    pub fn last_voice(&self) -> Option<String> {
        self.lock().last_voice.clone()
    }

    /// The value of the last `set_rate`.
    pub fn last_rate(&self) -> Option<f32> {
        self.lock().last_rate
    }

    /// The value of the last `set_pitch`.
    pub fn last_pitch(&self) -> Option<f32> {
        self.lock().last_pitch
    }

    /// The value of the last `set_volume`.
    pub fn last_volume(&self) -> Option<f32> {
        self.lock().last_volume
    }

    /// The milliseconds of the last `set_silence`.
    pub fn last_silence(&self) -> Option<u32> {
        self.lock().last_silence
    }

    /// Reports `progress` for the running utterance. Returns `false` when none runs.
    pub fn progress_utterance(&self, location: u64, length: u64) -> bool {
        // Out of the lock while the handler runs, as the other firings are.
        let pending = self.lock().pending.take();
        let Some(pending) = pending else {
            return false;
        };
        let id = pending.id;
        fire(
            &pending.sink,
            id,
            UtteranceEventKind::Progress { location, length },
        );
        self.lock().pending = Some(pending);
        true
    }

    /// Finishes the running utterance and starts the next queued one, as the system would.
    /// Returns `false` when none runs.
    pub fn finish_utterance(&self) -> bool {
        let pending = self.lock().pending.take();
        let Some(pending) = pending else {
            return false;
        };
        fire(&pending.sink, pending.id, UtteranceEventKind::Finish);
        self.advance();
        true
    }

    /// Fails the running utterance and starts the next queued one. Returns `false` when none runs.
    pub fn error_utterance(&self, error: SpeechError) -> bool {
        let pending = self.lock().pending.take();
        let Some(pending) = pending else {
            return false;
        };
        fire(&pending.sink, pending.id, UtteranceEventKind::Error(error));
        self.advance();
        true
    }

    /// Starts the next queued utterance, if one waits.
    fn advance(&self) {
        let next = self.lock().queue.pop_front();
        if let Some(next) = next {
            fire(&next.sink, next.id, UtteranceEventKind::Start);
            self.lock().pending = Some(next);
        }
    }
}

/// A 44-byte WAV header for no samples: what a `synthesize` answer looks like.
fn wav_header() -> Vec<u8> {
    let mut wav = Vec::with_capacity(44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&36u32.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&22050u32.to_le_bytes());
    wav.extend_from_slice(&44100u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&0u32.to_le_bytes());
    wav
}

/// Events fire outside the fake's lock: a handler may ask the fake back.
fn fire(sink: &EventSink, id: UtteranceId, kind: UtteranceEventKind) {
    sink(UtteranceEvent {
        utterance_id: id,
        kind,
    });
}

impl Backend for FakeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(ready(self.lock().init.clone()))
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(ready(self.lock().available.clone()))
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        let mut cancelled = None;
        let mut started = None;
        let mut queued = None;
        {
            let mut state = self.lock();
            state.last_text = Some(text.to_owned());
            state.last_interrupt = Some(interrupt);
            state.speaks += 1;
            if let Some(error) = state.next_speak_error.take() {
                return Box::pin(ready(Err(error)));
            }
            if interrupt {
                cancelled = state.pending.take();
            }
            let id = state.next_id;
            state.next_id += 1;
            let pending = Pending { id, sink: on_event };
            if state.pending.is_some() {
                state.queue.push_back(pending);
                queued = Some(id);
            } else {
                started = Some(pending);
            }
        }
        if let Some(old) = cancelled {
            fire(&old.sink, old.id, UtteranceEventKind::Cancel);
        }
        let started_id = started.as_ref().map(|p| p.id);
        if let Some(new) = started {
            fire(&new.sink, new.id, UtteranceEventKind::Start);
            self.lock().pending = Some(new);
        }
        Box::pin(ready(Ok(started_id.or(queued))))
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let pending = {
            let mut state = self.lock();
            state.stops += 1;
            state.pending.take()
        };
        if let Some(pending) = pending {
            fire(&pending.sink, pending.id, UtteranceEventKind::Cancel);
            self.advance();
        }
        Box::pin(ready(Ok(())))
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().pauses += 1;
        Box::pin(ready(Ok(())))
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().resumes += 1;
        Box::pin(ready(Ok(())))
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        let speaking = {
            let state = self.lock();
            state.pending.is_some() || !state.queue.is_empty()
        };
        Box::pin(ready(Ok(speaking)))
    }

    fn synthesize(&self, _text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        let (hold, answer) = {
            let mut state = self.lock();
            state.synth_calls += 1;
            (state.synth_hold.clone(), state.synth.clone())
        };
        Box::pin(async move {
            if let Some(hold) = hold {
                hold.notified().await;
            }
            answer
        })
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        Box::pin(ready(self.lock().voices.clone()))
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        Box::pin(ready(self.lock().engines.clone()))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        Box::pin(ready(Ok(self.lock().languages.clone())))
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        Box::pin(ready(Ok(self.lock().max_len)))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let known = self
            .lock()
            .languages
            .iter()
            .any(|language| language.eq_ignore_ascii_case(tag));
        if !known {
            return Box::pin(ready(Err(SpeechError::new(
                ErrorKind::LanguageNotSupported,
            ))));
        }
        self.lock().last_language = Some(tag.to_owned());
        Box::pin(ready(Ok(())))
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().last_voice = Some(id.to_owned());
        Box::pin(ready(Ok(())))
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().last_rate = Some(rate);
        Box::pin(ready(Ok(())))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().last_pitch = Some(pitch);
        Box::pin(ready(Ok(())))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().last_volume = Some(volume);
        Box::pin(ready(Ok(())))
    }

    fn set_silence(&self, ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        self.lock().last_silence = Some(ms);
        Box::pin(ready(Ok(())))
    }
}
