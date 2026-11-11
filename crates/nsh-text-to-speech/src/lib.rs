//! Text to speech for Native Shell apps: read text aloud, on every platform.
//!
//! [`Speech`] follows `flutter_tts` and `react-native-tts`: set a language, a voice and a rate,
//! then [`Speech::speak`] an utterance. Each call resolves exactly once, with its value or a
//! [`SpeechError`] with a stable code. While one utterance runs, `interrupt` cancels it and
//! speaks at once; otherwise the new utterance queues behind it, first in, first out.

mod backend;
mod error;
mod fake;
mod options;
mod platform;
mod speech;

use nsh_core::prelude::*;

pub use backend::{Backend, BackendFuture, EventSink};
pub use error::{ErrorKind, SpeechError};
pub use fake::FakeBackend;
pub use options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice};
pub use speech::Speech;

/// A voice, as TSX reads it from [`NativeSpeech::voices`].
#[derive(JsObject, Clone, Debug)]
pub struct NativeVoice {
    /// The id `set_voice` takes.
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

/// An engine, as TSX reads it from [`NativeSpeech::engines`].
#[derive(JsObject, Clone, Debug)]
pub struct NativeEngine {
    /// Its package name (`com.google.android.tts`).
    pub name: String,
    /// Its display name.
    pub label: String,
    /// Whether the system picks it by default.
    pub default: bool,
}

/// One utterance event, as TSX receives it in [`NativeSpeech::set_event_handler`].
#[derive(JsObject, Clone, Debug)]
pub struct NativeUtteranceEvent {
    /// The utterance it is about, the id `speak` returned.
    pub utterance_id: u64,
    /// `"start"`, `"progress"`, `"finish"`, `"cancel"` or `"error"`.
    pub kind: String,
    /// How far the utterance came, in characters; a progress event only.
    #[prop(skip_none)]
    pub location: Option<f64>,
    /// The utterance's length in characters; a progress event only.
    #[prop(skip_none)]
    pub length: Option<f64>,
    /// The error's stable code; an error event only.
    #[prop(skip_none)]
    pub error_code: Option<String>,
    /// The error's message; an error event only.
    #[prop(skip_none)]
    pub error_message: Option<String>,
}

/// Speech as TSX sees it: a class whose async members reject with an error carrying a `code`.
/// The queue, the completions and the platform stay on the Rust side.
#[class]
pub struct NativeSpeech {
    speech: Speech,
}

#[class]
impl NativeSpeech {
    /// The platform's speech. Throws where speech cannot run at all.
    pub fn new() -> Result<NativeSpeech, JsError> {
        Ok(NativeSpeech {
            speech: Speech::new().map_err(into_js)?,
        })
    }

    /// Prepares the platform's speech; its error says whether an engine is missing.
    pub async fn init(this: Instance<Self>) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.init().await.map_err(into_js)
    }

    /// Speaks `text` aloud, interrupting the running utterance when `interrupt` is true.
    pub async fn speak(
        this: Instance<Self>,
        text: String,
        interrupt: bool,
    ) -> Result<u64, JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.speak(&text, interrupt).await.map_err(into_js)
    }

    /// Whether `speak` waits for its utterance to end. Off by default.
    pub fn await_speak_completion(this: Instance<Self>, wait: bool) {
        let speech = this.with(|native| native.speech.clone());
        speech.await_speak_completion(wait);
    }

    /// Cancels the running utterance; an awaited `speak` rejects with `ERR_TTS_CANCELLED`.
    pub async fn stop(this: Instance<Self>) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.stop().await.map_err(into_js)
    }

    /// Pauses the running utterance. Rejects with `ERR_TTS_NOT_SPEAKING` when nothing speaks.
    pub async fn pause(this: Instance<Self>) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.pause().await.map_err(into_js)
    }

    /// Resumes a paused utterance. Rejects with `ERR_TTS_NOT_SPEAKING` when none is paused.
    pub async fn resume(this: Instance<Self>) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.resume().await.map_err(into_js)
    }

    /// Whether an utterance is being spoken.
    pub async fn is_speaking(this: Instance<Self>) -> Result<bool, JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.is_speaking().await.map_err(into_js)
    }

    /// Synthesizes `text` to WAV bytes, without speaking it.
    pub async fn synthesize(this: Instance<Self>, text: String) -> Result<Vec<u8>, JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.synthesize(&text).await.map_err(into_js)
    }

    /// Sets the language of later utterances.
    pub async fn set_language(this: Instance<Self>, language: String) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_language(&language).await.map_err(into_js)
    }

    /// Sets the voice of later utterances. Unknown and uninstalled voices are rejected.
    pub async fn set_voice(this: Instance<Self>, voice: String) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_voice(&voice).await.map_err(into_js)
    }

    /// Sets the rate of later utterances (0.0 to 1.0).
    pub async fn set_speech_rate(this: Instance<Self>, rate: f32) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_speech_rate(rate).await.map_err(into_js)
    }

    /// Sets the pitch of later utterances (0.5 to 2.0).
    pub async fn set_pitch(this: Instance<Self>, pitch: f32) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_pitch(pitch).await.map_err(into_js)
    }

    /// Sets the volume of later utterances (0.0 to 1.0).
    pub async fn set_volume(this: Instance<Self>, volume: f32) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_volume(volume).await.map_err(into_js)
    }

    /// Sets the silence before each utterance, in milliseconds; 0 starts immediately.
    pub async fn set_silence(this: Instance<Self>, silence_ms: u32) -> Result<(), JsError> {
        let speech = this.with(|native| native.speech.clone());
        speech.set_silence(silence_ms).await.map_err(into_js)
    }

    /// The voices the device offers; an empty list when the platform gives none.
    pub async fn voices(this: Instance<Self>) -> Result<Vec<NativeVoice>, JsError> {
        let speech = this.with(|native| native.speech.clone());
        let voices = speech.voices().await;
        Ok(voices.into_iter().map(to_native_voice).collect())
    }

    /// The installed engines; desktops and Apple report one.
    pub async fn engines(this: Instance<Self>) -> Result<Vec<NativeEngine>, JsError> {
        let speech = this.with(|native| native.speech.clone());
        let engines = speech.engines().await;
        Ok(engines.into_iter().map(to_native_engine).collect())
    }

    /// The languages a voice exists for, as BCP 47 tags.
    pub async fn languages(this: Instance<Self>) -> Result<Vec<String>, JsError> {
        let speech = this.with(|native| native.speech.clone());
        Ok(speech.languages().await)
    }

    /// Whether a voice exists for `language`.
    pub async fn is_language_available(
        this: Instance<Self>,
        language: String,
    ) -> Result<bool, JsError> {
        let speech = this.with(|native| native.speech.clone());
        Ok(speech.is_language_available(&language).await)
    }

    /// The longest text the platform accepts; `null` when it reports no limit.
    pub async fn max_speech_input_length(this: Instance<Self>) -> Result<Option<usize>, JsError> {
        let speech = this.with(|native| native.speech.clone());
        Ok(speech.max_speech_input_length().await)
    }

    /// Sets the handler every utterance event reaches, replacing any earlier one. Events arrive
    /// while the call's owner lives; the handler goes when it goes.
    #[overload(fn(handler: Callback<NativeUtteranceEvent>))]
    pub fn set_event_handler(
        this: Instance<Self>,
        #[args(handler)] handler: Callback<NativeUtteranceEvent>,
        cx: CallCx<'_>,
    ) {
        let speech = this.with(|native| native.speech.clone());
        // The sink is Send and may fire from a platform thread; the callback runs on the engine.
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        speech.set_event_handler(move |event| {
            let _ = sender.send(to_native_event(event));
        });
        cx.start(async move {
            while let Some(event) = receiver.recv().await {
                handler.post(event);
            }
        });
    }
}

/// The error a rejection carries: its message, and the `code` apps match on.
fn into_js(error: SpeechError) -> JsError {
    JsError::Error(error.message().to_owned()).with_code(error.code())
}

fn to_native_voice(voice: Voice) -> NativeVoice {
    NativeVoice {
        id: voice.id,
        name: voice.name,
        language: voice.language,
        network_required: voice.network_required,
        installed: voice.installed,
    }
}

fn to_native_engine(engine: Engine) -> NativeEngine {
    NativeEngine {
        name: engine.name,
        label: engine.label,
        default: engine.default,
    }
}

fn to_native_event(event: UtteranceEvent) -> NativeUtteranceEvent {
    let (kind, location, length, error_code, error_message) = match event.kind {
        UtteranceEventKind::Start => ("start", None, None, None, None),
        UtteranceEventKind::Progress { location, length } => (
            "progress",
            Some(location as f64),
            Some(length as f64),
            None,
            None,
        ),
        UtteranceEventKind::Finish => ("finish", None, None, None, None),
        UtteranceEventKind::Cancel => ("cancel", None, None, None, None),
        UtteranceEventKind::Error(error) => (
            "error",
            None,
            None,
            Some(error.code().to_owned()),
            Some(error.message().to_owned()),
        ),
    };
    NativeUtteranceEvent {
        utterance_id: event.utterance_id,
        kind: kind.to_owned(),
        location,
        length,
        error_code,
        error_message,
    }
}

/// The crate's UI set: what TSX imports from `nsh-text-to-speech/native`.
#[native_module("nsh-text-to-speech/native")]
fn native(registry: &mut Registry) {
    registry.class::<NativeSpeech>();
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use super::*;

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                // Backend futures are not Send, so calls that overlap spawn on the local set.
                let local = tokio::task::LocalSet::new();
                local.run_until(future).await
            })
    }

    /// Yields until `ready` holds, so a spawned call has reached its utterance.
    async fn settle(ready: impl Fn() -> bool) {
        for _ in 0..1000 {
            if ready() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the utterance never started");
    }

    /// The class over the fake: every answer is scripted, and none of it is audio.
    fn native_with_fake() -> (Instance<NativeSpeech>, FakeBackend) {
        let fake = FakeBackend::new();
        let native = Instance::new(NativeSpeech {
            speech: Speech::with_backend(Box::new(fake.clone())),
        });
        (native, fake)
    }

    #[test]
    fn the_js_error_carries_the_code_and_the_message() {
        let error = into_js(SpeechError::new(ErrorKind::Busy).with_status(7));
        assert_eq!(error.code(), Some("ERR_TTS_BUSY"));
        assert_eq!(error.message(), ErrorKind::Busy.message());
    }

    #[test]
    fn a_voice_maps_field_for_field() {
        let voice = to_native_voice(Voice {
            id: "v".to_owned(),
            name: "V".to_owned(),
            language: "en-US".to_owned(),
            network_required: true,
            installed: false,
        });
        assert_eq!(voice.id, "v");
        assert_eq!(voice.name, "V");
        assert_eq!(voice.language, "en-US");
        assert!(voice.network_required);
        assert!(!voice.installed);
    }

    #[test]
    fn an_engine_maps_field_for_field() {
        let engine = to_native_engine(Engine {
            name: "com.example.tts".to_owned(),
            label: "Example TTS".to_owned(),
            default: false,
        });
        assert_eq!(engine.name, "com.example.tts");
        assert_eq!(engine.label, "Example TTS");
        assert!(!engine.default);
    }

    #[test]
    fn every_event_maps_to_the_shape_tsx_reads() {
        let event = |kind| {
            to_native_event(UtteranceEvent {
                utterance_id: 3,
                kind,
            })
        };
        let start = event(UtteranceEventKind::Start);
        assert_eq!((start.kind.as_str(), start.utterance_id), ("start", 3));
        assert_eq!((start.location, start.length), (None, None));
        assert_eq!((start.error_code, start.error_message), (None, None));

        let progress = event(UtteranceEventKind::Progress {
            location: 5,
            length: 11,
        });
        assert_eq!(progress.kind, "progress");
        assert_eq!(
            (progress.location, progress.length),
            (Some(5.0), Some(11.0))
        );

        assert_eq!(event(UtteranceEventKind::Finish).kind, "finish");
        assert_eq!(event(UtteranceEventKind::Cancel).kind, "cancel");

        let error = event(UtteranceEventKind::Error(SpeechError::new(
            ErrorKind::SynthesisFailed,
        )));
        assert_eq!(error.kind, "error");
        assert_eq!(
            error.error_code.as_deref(),
            Some("ERR_TTS_SYNTHESIS_FAILED")
        );
        assert_eq!(
            error.error_message.as_deref(),
            Some(ErrorKind::SynthesisFailed.message())
        );
    }

    #[test]
    fn invalid_arguments_reject_with_the_code_before_the_backend() {
        let (native, fake) = native_with_fake();
        run(async {
            let speak = NativeSpeech::speak(native.clone(), " ".to_owned(), true)
                .await
                .unwrap_err();
            assert_eq!(speak.code(), Some("ERR_TTS_INVALID_ARGUMENT"));
            let language = NativeSpeech::set_language(native.clone(), "\t".to_owned())
                .await
                .unwrap_err();
            assert_eq!(language.code(), Some("ERR_TTS_INVALID_ARGUMENT"));
            let rate = NativeSpeech::set_speech_rate(native.clone(), 1.5)
                .await
                .unwrap_err();
            assert_eq!(rate.code(), Some("ERR_TTS_INVALID_ARGUMENT"));
            let pitch = NativeSpeech::set_pitch(native.clone(), 2.5)
                .await
                .unwrap_err();
            assert_eq!(pitch.code(), Some("ERR_TTS_INVALID_ARGUMENT"));
            let volume = NativeSpeech::set_volume(native.clone(), -0.1)
                .await
                .unwrap_err();
            assert_eq!(volume.code(), Some("ERR_TTS_INVALID_ARGUMENT"));
        });
        // Each rejection settled before the platform was asked anything.
        assert_eq!(fake.speaks(), 0);
        assert_eq!(fake.last_language(), None);
        assert_eq!(fake.last_rate(), None);
    }

    #[test]
    fn settings_reach_the_speech_behind_the_class() {
        let (native, fake) = native_with_fake();
        run(async {
            NativeSpeech::set_speech_rate(native.clone(), 0.5)
                .await
                .unwrap();
            NativeSpeech::set_pitch(native.clone(), 1.5).await.unwrap();
            NativeSpeech::set_volume(native.clone(), 0.25)
                .await
                .unwrap();
            NativeSpeech::set_silence(native.clone(), 250)
                .await
                .unwrap();
            NativeSpeech::set_language(native.clone(), "en-GB".to_owned())
                .await
                .unwrap();
            NativeSpeech::set_voice(native.clone(), "fake-voice".to_owned())
                .await
                .unwrap();
        });
        assert_eq!(fake.last_rate(), Some(0.5));
        assert_eq!(fake.last_pitch(), Some(1.5));
        assert_eq!(fake.last_volume(), Some(0.25));
        assert_eq!(fake.last_silence(), Some(250));
        assert_eq!(fake.last_language().as_deref(), Some("en-GB"));
        assert_eq!(fake.last_voice().as_deref(), Some("fake-voice"));
    }

    #[test]
    fn an_unknown_voice_keeps_its_rejection_code() {
        let (native, fake) = native_with_fake();
        run(async {
            let error = NativeSpeech::set_voice(native.clone(), "no-such-voice".to_owned())
                .await
                .unwrap_err();
            assert_eq!(error.code(), Some("ERR_TTS_VOICE_NOT_FOUND"));
        });
        assert_eq!(fake.last_voice(), None, "the platform is not asked");
    }

    #[test]
    fn the_class_speaks_and_stops_through_the_fake() {
        let (native, fake) = native_with_fake();
        run(async {
            NativeSpeech::await_speak_completion(native.clone(), true);
            let call = {
                let native = native.clone();
                tokio::task::spawn_local(async move {
                    NativeSpeech::speak(native, "Hello".to_owned(), true).await
                })
            };
            settle(|| fake.pending_id().is_some()).await;
            assert!(NativeSpeech::is_speaking(native.clone()).await.unwrap());

            NativeSpeech::stop(native.clone()).await.unwrap();
            let cancelled = call.await.unwrap().unwrap_err();
            assert_eq!(cancelled.code(), Some("ERR_TTS_CANCELLED"));
            assert!(!NativeSpeech::is_speaking(native.clone()).await.unwrap());
        });
    }

    #[test]
    fn the_class_pauses_and_resumes_or_reports_not_speaking() {
        let (native, _fake) = native_with_fake();
        run(async {
            let idle = NativeSpeech::pause(native.clone()).await.unwrap_err();
            assert_eq!(idle.code(), Some("ERR_TTS_NOT_SPEAKING"));
            let unpaused = NativeSpeech::resume(native.clone()).await.unwrap_err();
            assert_eq!(unpaused.code(), Some("ERR_TTS_NOT_SPEAKING"));

            NativeSpeech::speak(native.clone(), "Hello".to_owned(), true)
                .await
                .unwrap();
            NativeSpeech::pause(native.clone()).await.unwrap();
            NativeSpeech::resume(native).await.unwrap();
        });
    }

    #[test]
    fn synthesize_returns_the_wav_without_speaking() {
        let (native, _fake) = native_with_fake();
        run(async {
            let bytes = NativeSpeech::synthesize(native, "hello".to_owned())
                .await
                .unwrap();
            assert!(bytes.starts_with(b"RIFF"));
            assert!(bytes.len() >= 44);
        });
    }

    #[test]
    fn init_passes_the_engine_answer_through() {
        let (native, fake) = native_with_fake();
        run(async {
            NativeSpeech::init(native.clone()).await.unwrap();
            fake.set_init(Err(SpeechError::new(ErrorKind::EngineMissing)));
            let error = NativeSpeech::init(native).await.unwrap_err();
            assert_eq!(error.code(), Some("ERR_TTS_ENGINE_MISSING"));
        });
    }

    #[test]
    fn the_lists_map_into_the_shapes_tsx_reads() {
        let (native, _fake) = native_with_fake();
        run(async {
            let voices = NativeSpeech::voices(native.clone()).await.unwrap();
            assert_eq!(voices.len(), 2);
            assert_eq!(voices[0].id, "fake-voice");
            assert_eq!(voices[0].language, "en-US");
            assert!(voices[0].installed);
            assert!(!voices[0].network_required);

            let engines = NativeSpeech::engines(native.clone()).await.unwrap();
            assert_eq!(engines.len(), 1);
            assert_eq!(engines[0].label, "Fake TTS");

            let languages = NativeSpeech::languages(native.clone()).await.unwrap();
            assert_eq!(languages, ["en-US", "en-GB"]);
            assert!(
                NativeSpeech::is_language_available(native.clone(), "en-US".to_owned())
                    .await
                    .unwrap()
            );
            let limit = NativeSpeech::max_speech_input_length(native).await.unwrap();
            assert_eq!(limit, Some(4000));
        });
    }

    #[test]
    #[cfg(target_vendor = "apple")]
    fn the_platform_backend_answers_queries_without_audio() {
        let native = Instance::new(NativeSpeech::new().expect("this Apple host has speech"));
        // Enumerate and query only: no utterance is spoken anywhere here. Every call is
        // bounded, so a wedged platform fails the test instead of hanging it.
        let bound = std::time::Duration::from_secs(30);
        run(async {
            let init = tokio::time::timeout(bound, NativeSpeech::init(native.clone())).await;
            assert!(init.is_ok(), "init settles");
            let speaking = tokio::time::timeout(bound, NativeSpeech::is_speaking(native.clone()))
                .await
                .expect("is_speaking settles")
                .unwrap();
            assert!(!speaking);

            let voices = tokio::time::timeout(bound, NativeSpeech::voices(native.clone()))
                .await
                .expect("voices settles")
                .unwrap();
            assert!(!voices.is_empty(), "this host lists its voices");

            let engines = tokio::time::timeout(bound, NativeSpeech::engines(native.clone()))
                .await
                .expect("engines settles")
                .unwrap();
            assert_eq!(
                engines.first().map(|engine| engine.label.as_str()),
                Some("AVSpeechSynthesizer")
            );

            let languages = tokio::time::timeout(bound, NativeSpeech::languages(native.clone()))
                .await
                .expect("languages settles")
                .unwrap();
            assert!(languages.iter().any(|language| language == "en-US"));

            let limit =
                tokio::time::timeout(bound, NativeSpeech::max_speech_input_length(native.clone()))
                    .await
                    .expect("the limit settles")
                    .unwrap();
            assert_eq!(limit, None, "AVFoundation documents no limit");
        });
    }
}
