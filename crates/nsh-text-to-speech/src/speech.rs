//! `Speech`: the calls an app makes — one completion per call, a FIFO queue behind a running
//! utterance, and settings checked before the platform is asked.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::oneshot;

use crate::{
    backend::{Backend, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice},
};

type EventHandler = Arc<dyn Fn(UtteranceEvent) + Send + Sync>;

struct SpeechState {
    await_completion: bool,
    paused: bool,
    synthesizing: bool,
    next_local_id: UtteranceId,
}

struct Shared {
    backend: Arc<dyn Backend>,
    state: Mutex<SpeechState>,
    handler: Mutex<Option<EventHandler>>,
}

/// Reading text aloud, as `flutter_tts` and `react-native-tts` name it.
///
/// Clones share one queue and one completion state. While an utterance runs, `speak` with
/// `interrupt` cancels it (a `cancel` event, then the new utterance starts); without, the new
/// utterance queues behind it, first in, first out. Every call resolves exactly once: with its
/// value, a coded [`SpeechError`], or — an awaited `speak` that was stopped — a cancel.
#[derive(Clone)]
pub struct Speech {
    shared: Arc<Shared>,
}

impl Speech {
    /// The platform's speech. `Err` where speech cannot run at all; the platform's own faults
    /// surface later, from [`Speech::init`].
    pub fn new() -> Result<Self, SpeechError> {
        let backend = crate::platform::platform_backend()?;
        Ok(Self::from_arc(Arc::from(backend)))
    }

    /// Speech through `backend`, with its own queue. Tests and fakes use this.
    pub fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self::from_arc(Arc::from(backend))
    }

    fn from_arc(backend: Arc<dyn Backend>) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                state: Mutex::new(SpeechState {
                    await_completion: false,
                    paused: false,
                    synthesizing: false,
                    next_local_id: 1,
                }),
                handler: Mutex::new(None),
            }),
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, SpeechState> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the handler every utterance event reaches, replacing any earlier one. The handler
    /// runs on the platform's own thread; it must be callable while another call runs.
    pub fn set_event_handler(&self, handler: impl Fn(UtteranceEvent) + Send + Sync + 'static) {
        *self
            .shared
            .handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(handler));
    }

    /// The wire an utterance's events run through: the handler hears them, and the first
    /// terminal one settles the awaited `speak` and ends any pause. The completion carries no
    /// id: `speak` fills in the id it returned, whatever the platform stamped on the event.
    fn sink(&self, sender: oneshot::Sender<Result<(), SpeechError>>) -> EventSink {
        let shared = Arc::clone(&self.shared);
        let sender = Mutex::new(Some(sender));
        Box::new(move |event| {
            if let Some(result) = terminal(&event.kind) {
                let sender = sender.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(sender) = sender {
                    let _ = sender.send(result);
                }
                shared
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .paused = false;
            }
            // The handler runs after the call's own state is settled, outside every lock.
            let handler = shared
                .handler
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(handler) = handler {
                handler(event);
            }
        })
    }

    /// Prepares the platform's speech. `EngineMissing` when the device has no engine;
    /// `NotAvailable` when speech exists but cannot run here.
    pub async fn init(&self) -> Result<(), SpeechError> {
        self.shared.backend.init().await
    }

    /// Speaks `text` aloud and returns the utterance's id. Blank text, and text past the
    /// platform's length limit, are rejected without asking the platform.
    ///
    /// While an utterance runs, `interrupt` cancels it; otherwise the new utterance queues.
    /// With [`Speech::await_speak_completion`] on, the call resolves when its utterance ends —
    /// `Ok` with the id when it finished, `Err` when it was cancelled or failed — exactly once.
    pub async fn speak(&self, text: &str, interrupt: bool) -> Result<UtteranceId, SpeechError> {
        if text.trim().is_empty() {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        if let Some(max) = self
            .shared
            .backend
            .max_speech_input_length()
            .await
            .ok()
            .flatten()
            && text.chars().count() > max
        {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        let (sender, receiver) = oneshot::channel();
        let sink = self.sink(sender);
        let started = self.shared.backend.speak(text, interrupt, sink).await?;
        let id = started.unwrap_or_else(|| {
            let mut state = self.lock_state();
            let id = state.next_local_id;
            state.next_local_id += 1;
            id
        });
        if !self.lock_state().await_completion {
            return Ok(id);
        }
        receiver
            .await
            .unwrap_or_else(|_| Err(SpeechError::new(ErrorKind::Backend)))
            .map(|()| id)
    }

    /// Whether `speak` waits for its utterance to end (off by default, as `flutter_tts` has it).
    pub fn await_speak_completion(&self, wait: bool) {
        self.lock_state().await_completion = wait;
    }

    /// Cancels the running utterance; its events end with `cancel`, and its awaited `speak`
    /// resolves `ERR_TTS_CANCELLED` exactly once. A queued utterance still runs next.
    pub async fn stop(&self) -> Result<(), SpeechError> {
        self.lock_state().paused = false;
        self.shared.backend.stop().await
    }

    /// Pauses the running utterance. `ERR_TTS_NOT_SPEAKING` when nothing speaks.
    pub async fn pause(&self) -> Result<(), SpeechError> {
        if !self.shared.backend.is_speaking().await? {
            return Err(SpeechError::new(ErrorKind::NotSpeaking));
        }
        self.shared.backend.pause().await?;
        self.lock_state().paused = true;
        Ok(())
    }

    /// Resumes a paused utterance. `ERR_TTS_NOT_SPEAKING` when none is paused.
    pub async fn resume(&self) -> Result<(), SpeechError> {
        if !self.lock_state().paused {
            return Err(SpeechError::new(ErrorKind::NotSpeaking));
        }
        self.shared.backend.resume().await?;
        self.lock_state().paused = false;
        Ok(())
    }

    /// Whether an utterance is being spoken, now or from the queue.
    pub async fn is_speaking(&self) -> Result<bool, SpeechError> {
        self.shared.backend.is_speaking().await
    }

    /// Synthesizes `text` to WAV bytes, without speaking it. A second call while one runs
    /// gets `ERR_TTS_BUSY`. Dropping the call's future mid-run frees the gate for the next one.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>, SpeechError> {
        if text.trim().is_empty() {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        {
            let mut state = self.lock_state();
            if state.synthesizing {
                return Err(SpeechError::new(ErrorKind::Busy));
            }
            state.synthesizing = true;
        }
        // Dropped with the future it lives in, so a cancelled call cannot leave the gate held.
        let _gate = SynthGate {
            shared: Arc::clone(&self.shared),
        };
        self.shared.backend.synthesize(text).await
    }

    /// Sets the language of later utterances. An empty tag is rejected without asking.
    pub async fn set_language(&self, tag: &str) -> Result<(), SpeechError> {
        if tag.trim().is_empty() {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        self.shared.backend.set_language(tag).await
    }

    /// Sets the voice of later utterances. An unknown id, or a voice that is not installed,
    /// gets `ERR_TTS_VOICE_NOT_FOUND`; the platform is not asked.
    pub async fn set_voice(&self, id: &str) -> Result<(), SpeechError> {
        let installed = self
            .shared
            .backend
            .voices()
            .await
            .unwrap_or_default()
            .into_iter()
            .any(|voice| voice.installed && voice.id == id);
        if !installed {
            return Err(SpeechError::new(ErrorKind::VoiceNotFound));
        }
        self.shared.backend.set_voice(id).await
    }

    /// Sets the rate of later utterances, 0.0 to 1.0. Out of range is rejected without asking.
    pub async fn set_speech_rate(&self, rate: f32) -> Result<(), SpeechError> {
        if !(0.0..=1.0).contains(&rate) {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        self.shared.backend.set_rate(rate).await
    }

    /// Sets the pitch of later utterances, 0.5 to 2.0. Out of range is rejected without asking.
    pub async fn set_pitch(&self, pitch: f32) -> Result<(), SpeechError> {
        if !(0.5..=2.0).contains(&pitch) {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        self.shared.backend.set_pitch(pitch).await
    }

    /// Sets the volume of later utterances, 0.0 to 1.0. Out of range is rejected without asking.
    pub async fn set_volume(&self, volume: f32) -> Result<(), SpeechError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(SpeechError::new(ErrorKind::InvalidArgument));
        }
        self.shared.backend.set_volume(volume).await
    }

    /// Sets the silence before each utterance, in milliseconds; 0 starts immediately.
    pub async fn set_silence(&self, ms: u32) -> Result<(), SpeechError> {
        self.shared.backend.set_silence(ms).await
    }

    /// The voices the device offers; an empty list when the platform gives none.
    pub async fn voices(&self) -> Vec<Voice> {
        self.shared.backend.voices().await.unwrap_or_default()
    }

    /// The installed engines; desktops and Apple report one.
    pub async fn engines(&self) -> Vec<Engine> {
        self.shared.backend.engines().await.unwrap_or_default()
    }

    /// The languages a voice exists for, as BCP 47 tags.
    pub async fn languages(&self) -> Vec<String> {
        self.shared.backend.languages().await.unwrap_or_default()
    }

    /// Whether a voice exists for `tag`; the case does not matter, as BCP 47 has it.
    pub async fn is_language_available(&self, tag: &str) -> bool {
        self.shared
            .backend
            .languages()
            .await
            .ok()
            .is_some_and(|languages| {
                languages
                    .iter()
                    .any(|language| language.eq_ignore_ascii_case(tag))
            })
    }

    /// The longest text the platform accepts; `None` when it reports no limit.
    pub async fn max_speech_input_length(&self) -> Option<usize> {
        self.shared
            .backend
            .max_speech_input_length()
            .await
            .ok()
            .flatten()
    }
}

/// Frees the synthesize gate when the call ends, however it ends: answered, failed, or its
/// future dropped at the backend's await.
struct SynthGate {
    shared: Arc<Shared>,
}

impl Drop for SynthGate {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .synthesizing = false;
    }
}

/// The resolution a terminal event carries, or `None` when the utterance goes on. The id is
/// `speak`'s to fill in; the event's own stamp may differ where a platform gives none.
fn terminal(kind: &UtteranceEventKind) -> Option<Result<(), SpeechError>> {
    match kind {
        UtteranceEventKind::Finish => Some(Ok(())),
        UtteranceEventKind::Cancel => Some(Err(SpeechError::new(ErrorKind::Cancelled))),
        UtteranceEventKind::Error(error) => Some(Err(error.clone())),
        UtteranceEventKind::Start | UtteranceEventKind::Progress { .. } => None,
    }
}
