//! Android: `android.speech.tts.TextToSpeech` over JNI, the technique of tts-rs's Android backend.
//!
//! The VM and the activity come from `ndk-context`; an app where nothing published them (an NSH
//! app has nothing doing that yet) settles every call with `ERR_TTS_NOT_AVAILABLE`. `init` waits
//! for the engine's `onInit`, whose failure is `ERR_TTS_ENGINE_MISSING`; `LANG_MISSING_DATA` and
//! `LANG_NOT_SUPPORTED` map to their codes. Android's TTS has no pause or resume, so those two
//! settle `ERR_TTS_BACKEND` rather than pretend; its `stop` also flushes the engine's own queue
//! without reporting the flushed utterances, so this backend settles every sink itself.
//!
//! Callbacks arrive through `dev.supernsh.tts.Bridge`, a class extending
//! `UtteranceProgressListener` that implements `OnInitListener` with one public native method per
//! `BRIDGE_METHODS` entry; ART accepts no interface implemented from Rust alone. Its dex is the
//! embedded `BRIDGE_DEX` (format 038), built with `javac -bootclasspath $ANDROID_JAR` and `d8
//! --min-api 26 --lib $ANDROID_JAR`, and loaded through `InMemoryDexClassLoader` (API 26+).
//!
//! The tests at the bottom are pure-function tests: they compile under the aarch64-linux-android
//! cross-check and would run on an Android runner, which this workflow does not drive (AGENTS.md
//! forbids local emulators).

use std::{
    collections::HashMap,
    ffi::c_void,
    future::ready,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use jni::{
    Env, JavaVM, NativeMethod, jni_sig, jni_str, native_method,
    objects::{Global, JClass, JObject, JString},
    sys::{jboolean, jint, jlong},
};

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice},
};

/// `dev.supernsh.tts.Bridge`, dex format 038, min-api 26. See the module header for rebuilding.
#[rustfmt::skip]
const BRIDGE_DEX: &[u8] = &[
    0x64, 0x65, 0x78, 0x0a, 0x30, 0x33, 0x38, 0x00, 0x90, 0xad, 0x43, 0x7a, 0x28, 0x01, 0x0d, 0x40,
    0x26, 0xb9, 0xa6, 0x80, 0xc0, 0x99, 0x29, 0x5e, 0x39, 0x92, 0x12, 0xab, 0x47, 0x4f, 0x2e, 0x49,
    0x14, 0x04, 0x00, 0x00, 0x70, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x80, 0x03, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x70, 0x00, 0x00, 0x00,
    0x07, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xdc, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x18, 0x01, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x58, 0x01, 0x00, 0x00, 0x9c, 0x02, 0x00, 0x00, 0x78, 0x01, 0x00, 0x00,
    0xc0, 0x01, 0x00, 0x00, 0xc8, 0x01, 0x00, 0x00, 0xd5, 0x01, 0x00, 0x00, 0xd8, 0x01, 0x00, 0x00,
    0x0a, 0x02, 0x00, 0x00, 0x3a, 0x02, 0x00, 0x00, 0x55, 0x02, 0x00, 0x00, 0x69, 0x02, 0x00, 0x00,
    0x6c, 0x02, 0x00, 0x00, 0x70, 0x02, 0x00, 0x00, 0x74, 0x02, 0x00, 0x00, 0x7b, 0x02, 0x00, 0x00,
    0x80, 0x02, 0x00, 0x00, 0x83, 0x02, 0x00, 0x00, 0x8b, 0x02, 0x00, 0x00, 0x94, 0x02, 0x00, 0x00,
    0x9c, 0x02, 0x00, 0x00, 0xaa, 0x02, 0x00, 0x00, 0xb3, 0x02, 0x00, 0x00, 0xbb, 0x02, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x06, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x9c, 0x01, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xa4, 0x01, 0x00, 0x00,
    0x0a, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xac, 0x01, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0xb8, 0x01, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x0d, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x02, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x0f, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x03, 0x00, 0x10, 0x00, 0x00, 0x00, 0x03, 0x00, 0x02, 0x00, 0x11, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x04, 0x00, 0x12, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x94, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x5d, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x90, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x70, 0x10, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x00,
    0x0d, 0x00, 0x0e, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x06, 0x00,
    0x06, 0x3c, 0x69, 0x6e, 0x69, 0x74, 0x3e, 0x00, 0x0b, 0x42, 0x72, 0x69, 0x64, 0x67, 0x65, 0x2e,
    0x6a, 0x61, 0x76, 0x61, 0x00, 0x01, 0x49, 0x00, 0x30, 0x4c, 0x61, 0x6e, 0x64, 0x72, 0x6f, 0x69,
    0x64, 0x2f, 0x73, 0x70, 0x65, 0x65, 0x63, 0x68, 0x2f, 0x74, 0x74, 0x73, 0x2f, 0x54, 0x65, 0x78,
    0x74, 0x54, 0x6f, 0x53, 0x70, 0x65, 0x65, 0x63, 0x68, 0x24, 0x4f, 0x6e, 0x49, 0x6e, 0x69, 0x74,
    0x4c, 0x69, 0x73, 0x74, 0x65, 0x6e, 0x65, 0x72, 0x3b, 0x00, 0x2e, 0x4c, 0x61, 0x6e, 0x64, 0x72,
    0x6f, 0x69, 0x64, 0x2f, 0x73, 0x70, 0x65, 0x65, 0x63, 0x68, 0x2f, 0x74, 0x74, 0x73, 0x2f, 0x55,
    0x74, 0x74, 0x65, 0x72, 0x61, 0x6e, 0x63, 0x65, 0x50, 0x72, 0x6f, 0x67, 0x72, 0x65, 0x73, 0x73,
    0x4c, 0x69, 0x73, 0x74, 0x65, 0x6e, 0x65, 0x72, 0x3b, 0x00, 0x19, 0x4c, 0x64, 0x65, 0x76, 0x2f,
    0x73, 0x75, 0x70, 0x65, 0x72, 0x6e, 0x73, 0x68, 0x2f, 0x74, 0x74, 0x73, 0x2f, 0x42, 0x72, 0x69,
    0x64, 0x67, 0x65, 0x3b, 0x00, 0x12, 0x4c, 0x6a, 0x61, 0x76, 0x61, 0x2f, 0x6c, 0x61, 0x6e, 0x67,
    0x2f, 0x53, 0x74, 0x72, 0x69, 0x6e, 0x67, 0x3b, 0x00, 0x01, 0x56, 0x00, 0x02, 0x56, 0x49, 0x00,
    0x02, 0x56, 0x4c, 0x00, 0x05, 0x56, 0x4c, 0x49, 0x49, 0x49, 0x00, 0x03, 0x56, 0x4c, 0x5a, 0x00,
    0x01, 0x5a, 0x00, 0x06, 0x6f, 0x6e, 0x44, 0x6f, 0x6e, 0x65, 0x00, 0x07, 0x6f, 0x6e, 0x45, 0x72,
    0x72, 0x6f, 0x72, 0x00, 0x06, 0x6f, 0x6e, 0x49, 0x6e, 0x69, 0x74, 0x00, 0x0c, 0x6f, 0x6e, 0x52,
    0x61, 0x6e, 0x67, 0x65, 0x53, 0x74, 0x61, 0x72, 0x74, 0x00, 0x07, 0x6f, 0x6e, 0x53, 0x74, 0x61,
    0x72, 0x74, 0x00, 0x06, 0x6f, 0x6e, 0x53, 0x74, 0x6f, 0x70, 0x00, 0x9f, 0x01, 0x7e, 0x7e, 0x44,
    0x38, 0x7b, 0x22, 0x62, 0x61, 0x63, 0x6b, 0x65, 0x6e, 0x64, 0x22, 0x3a, 0x22, 0x64, 0x65, 0x78,
    0x22, 0x2c, 0x22, 0x63, 0x6f, 0x6d, 0x70, 0x69, 0x6c, 0x61, 0x74, 0x69, 0x6f, 0x6e, 0x2d, 0x6d,
    0x6f, 0x64, 0x65, 0x22, 0x3a, 0x22, 0x72, 0x65, 0x6c, 0x65, 0x61, 0x73, 0x65, 0x22, 0x2c, 0x22,
    0x68, 0x61, 0x73, 0x2d, 0x63, 0x68, 0x65, 0x63, 0x6b, 0x73, 0x75, 0x6d, 0x73, 0x22, 0x3a, 0x66,
    0x61, 0x6c, 0x73, 0x65, 0x2c, 0x22, 0x6d, 0x69, 0x6e, 0x2d, 0x61, 0x70, 0x69, 0x22, 0x3a, 0x32,
    0x36, 0x2c, 0x22, 0x73, 0x68, 0x61, 0x2d, 0x31, 0x22, 0x3a, 0x22, 0x61, 0x37, 0x61, 0x64, 0x31,
    0x38, 0x61, 0x37, 0x30, 0x34, 0x36, 0x30, 0x62, 0x37, 0x39, 0x39, 0x64, 0x30, 0x34, 0x38, 0x32,
    0x65, 0x34, 0x39, 0x37, 0x63, 0x31, 0x30, 0x39, 0x61, 0x37, 0x35, 0x62, 0x66, 0x37, 0x66, 0x39,
    0x31, 0x64, 0x65, 0x22, 0x2c, 0x22, 0x76, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x22, 0x3a, 0x22,
    0x38, 0x2e, 0x31, 0x30, 0x2e, 0x39, 0x2d, 0x64, 0x65, 0x76, 0x22, 0x7d, 0x00, 0x00, 0x00, 0x01,
    0x06, 0x01, 0x81, 0x80, 0x04, 0xf8, 0x02, 0x02, 0x81, 0x02, 0x00, 0x01, 0x81, 0x02, 0x00, 0x01,
    0x81, 0x02, 0x00, 0x01, 0x81, 0x02, 0x00, 0x01, 0x81, 0x02, 0x00, 0x01, 0x81, 0x02, 0x00, 0x00,
    0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x70, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x07, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0xdc, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x18, 0x01, 0x00, 0x00,
    0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x58, 0x01, 0x00, 0x00, 0x01, 0x20, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x78, 0x01, 0x00, 0x00, 0x03, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x90, 0x01, 0x00, 0x00, 0x01, 0x10, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x94, 0x01, 0x00, 0x00,
    0x02, 0x20, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0xc0, 0x01, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x5d, 0x03, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x80, 0x03, 0x00, 0x00,
];

/// `TextToSpeech.QUEUE_ADD` (0) queues behind a running utterance; `QUEUE_FLUSH` (1) cancels it.
const QUEUE_ADD: jint = 0;
const QUEUE_FLUSH: jint = 1;
/// `TextToSpeech.LANG_MISSING_DATA` and `LANG_NOT_SUPPORTED`; 0 to 2 mean the language is usable.
const LANG_MISSING_DATA: jint = -1;
const LANG_NOT_SUPPORTED: jint = -2;
/// `TextToSpeech.Engine.KEY_PARAM_VOLUME`, the per-utterance volume key in the speak bundle.
const KEY_PARAM_VOLUME: &str = "volume";
/// `TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED`, marking a voice that still needs downloading.
const KEY_FEATURE_NOT_INSTALLED: &str = "notInstalled";
/// Bounded waits: `onInit` arrives on the app's main thread, and a caller on that same thread must
/// still settle, as must a synthesis whose engine never calls back.
const ENGINE_TIMEOUT: Duration = Duration::from_secs(5);
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(120);

/// `QUEUE_FLUSH` when interrupting, `QUEUE_ADD` when queueing behind the running utterance.
fn queue_mode(interrupt: bool) -> jint {
    if interrupt { QUEUE_FLUSH } else { QUEUE_ADD }
}

/// Our rate (0.0 to 1.0, 0.5 normal) to the engine's multiplier (1.0 normal), as flutter_tts has it.
fn speech_rate(rate: f32) -> f32 {
    rate * 2.0
}

/// The engine's `setLanguage`/`setVoice` answer: 0 to 2 usable, -1 data missing, -2 unsupported.
fn language_result(code: jint) -> Result<(), SpeechError> {
    match code {
        LANG_MISSING_DATA => Err(SpeechError::new(ErrorKind::LanguageMissingData)),
        LANG_NOT_SUPPORTED => Err(SpeechError::new(ErrorKind::LanguageNotSupported)),
        code if code >= 0 => Ok(()),
        code => Err(SpeechError::new(ErrorKind::Backend).with_status(i64::from(code))),
    }
}

/// `onInit`'s status: 0 means the engine came up, anything else that none did.
fn init_result(status: jint) -> Result<(), SpeechError> {
    if status == 0 {
        Ok(())
    } else {
        Err(SpeechError::new(ErrorKind::EngineMissing))
    }
}

/// The engine's int return: 0 is success, anything else a fault carrying the code as its status.
fn call_result(rv: jint) -> Result<(), SpeechError> {
    if rv == 0 {
        Ok(())
    } else {
        Err(SpeechError::new(ErrorKind::Backend).with_status(i64::from(rv)))
    }
}

/// Which call an Android utterance id belongs to, as built by the `*_id` functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Utterance {
    Speak(UtteranceId),
    Synthesis(UtteranceId),
    Silence,
}

/// `onRangeStart`'s [start, end) character range: where the utterance got to, and how many
/// characters that stretch spans. Negative inputs clamp to zero.
fn progress(start: jint, end: jint) -> UtteranceEventKind {
    UtteranceEventKind::Progress {
        location: u64::try_from(start).unwrap_or(0),
        length: u64::try_from(end.saturating_sub(start)).unwrap_or(0),
    }
}

/// Whether `speak` may start at once: `onInit` must have succeeded and the engine object be
/// stored. Anything less queues, because `TextToSpeech.speak` only errors while the service
/// is still connecting.
fn can_start_now(initialized: Option<&Result<(), SpeechError>>, has_tts: bool) -> bool {
    initialized.is_some_and(|result| result.is_ok()) && has_tts
}

/// The engine's `getMaxSpeechInputLength` answer; a negative one is no usable limit, so `None`.
fn max_length(rv: jint) -> Option<usize> {
    usize::try_from(rv).ok()
}

fn speak_id(id: UtteranceId) -> String {
    id.to_string()
}

fn synth_id(id: UtteranceId) -> String {
    format!("synth:{id}")
}

fn silence_id(id: UtteranceId) -> String {
    format!("silence:{id}")
}

/// Splits an Android utterance id into its call; internal ids carry a prefix the engine echoes back.
fn parse_utterance(id: &str) -> Option<Utterance> {
    if let Some(id) = id.strip_prefix("synth:") {
        return id.parse().ok().map(Utterance::Synthesis);
    }
    if id.starts_with("silence:") {
        return Some(Utterance::Silence);
    }
    id.parse().ok().map(Utterance::Speak)
}

/// Every JNI fault is a coded `Backend` error; none of them panics past this boundary.
impl From<jni::errors::Error> for SpeechError {
    fn from(_: jni::errors::Error) -> Self {
        SpeechError::new(ErrorKind::Backend)
    }
}

/// One utterance held until the engine reports `onInit`, replayed FIFO unless interrupted away.
struct Queued {
    id: UtteranceId,
    text: String,
    interrupt: bool,
}

#[derive(Default)]
struct EngineState {
    /// `None` until `onInit` answers; `Err` is why the engine cannot run.
    initialized: Option<Result<(), SpeechError>>,
    /// The application context and the `TextToSpeech` object, once construction stored them.
    context: Option<Arc<Global<JObject<'static>>>>,
    tts: Option<Arc<Global<JObject<'static>>>>,
    /// The sinks of utterances the engine has taken, by id; each is taken out to fire exactly once.
    sinks: HashMap<UtteranceId, EventSink>,
    /// `speak` calls made before the engine object was stored; a replay speaks them.
    queued: Vec<Queued>,
    /// Blocked `synthesize` calls by id; their `onDone`/`onStop`/`onError` completes them.
    syntheses: HashMap<UtteranceId, mpsc::Sender<Result<(), SpeechError>>>,
    /// What the app last asked for, so a replay restores it onto the engine.
    language: Option<String>,
    voice: Option<String>,
    rate: Option<f32>,
    pitch: Option<f32>,
    /// Per-utterance settings the engine takes in its speak bundle rather than up front.
    volume: Option<f32>,
    silence_ms: u32,
}

/// The one engine every backend shares — Android has one system TTS. Constructed once, never
/// dropped: the engine service and its global references live for the process.
struct AndroidEngine {
    vm: JavaVM,
    state: Mutex<EngineState>,
    ready: Condvar,
}

static BRIDGE: OnceLock<Global<JClass<'static>>> = OnceLock::new();
static NEXT_UTTERANCE_ID: AtomicU64 = AtomicU64::new(1);
/// Set before construction starts (a synchronous `onInit` must find the engine) and read by the
/// JNI callbacks; never locked while JNI runs.
static ENGINE: Mutex<Option<Arc<AndroidEngine>>> = Mutex::new(None);
/// Serializes construction only; callbacks never take it, so the constructor's re-entrant
/// `onInit` cannot deadlock against it.
static CONSTRUCTING: Mutex<()> = Mutex::new(());
/// Serializes the ndk-context probe's panic-hook swap; see [`vm_and_context`].
static PROBING: Mutex<()> = Mutex::new(());

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The engine the callbacks report for, if one is registered.
fn active() -> Option<Arc<AndroidEngine>> {
    locked(&ENGINE).clone()
}

/// ndk-context's getter panics while nothing is installed and it has no try-get, so the probe
/// silences the hook and catches, as biometrics does; [`PROBING`] keeps the swap atomic against
/// concurrent probes. Absence is `NotAvailable`.
fn vm_and_context() -> Result<(JavaVM, *mut c_void), SpeechError> {
    let _probe = locked(&PROBING);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let probe = catch_unwind(AssertUnwindSafe(ndk_context::android_context));
    std::panic::set_hook(previous);
    let context = probe.map_err(|_| SpeechError::new(ErrorKind::NotAvailable))?;
    let (vm, context) = (context.vm(), context.context());
    if vm.is_null() || context.is_null() {
        return Err(SpeechError::new(ErrorKind::NotAvailable));
    }
    // SAFETY: the runtime that published ndk-context owns the VM for the process's lifetime.
    Ok((unsafe { JavaVM::from_raw(vm.cast()) }, context))
}

/// The `Bridge` class, defined from `BRIDGE_DEX` on first use — deferred, because its parent
/// class loader only exists once the app runs.
fn bridge(
    env: &mut Env<'_>,
    context: &JObject<'_>,
) -> Result<&'static Global<JClass<'static>>, SpeechError> {
    if let Some(class) = BRIDGE.get() {
        return Ok(class);
    }
    let parent = env
        .call_method(
            context,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    // The class loader keeps referencing the buffer, so leak a mutable copy of the dex data.
    let dex: &'static mut [u8] = Box::leak(Box::from(BRIDGE_DEX));
    let buffer = unsafe { env.new_direct_byte_buffer(dex.as_mut_ptr(), dex.len())? };
    let loader = env.new_object(
        jni_str!("dalvik/system/InMemoryDexClassLoader"),
        jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
        &[(&buffer).into(), (&parent).into()],
    )?;
    let name = env.new_string("dev.supernsh.tts.Bridge")?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[(&name).into()],
        )?
        .l()?;
    let class = env.cast_local::<JClass>(class)?;
    unsafe { env.register_native_methods(&class, BRIDGE_METHODS)? };
    let class = env.new_global_ref(&class)?;
    // A racing thread may have won, in which case its class wins and ours is dropped.
    Ok(BRIDGE.get_or_init(move || class))
}

/// Resolves the application context (so an Activity is never pinned), defines the bridge, and
/// builds the `TextToSpeech` with the default engine. A synchronous `onInit` from inside the
/// constructor finds the engine already registered.
fn construct(
    env: &mut Env<'_>,
    engine: &Arc<AndroidEngine>,
    context_raw: *mut c_void,
) -> Result<(), SpeechError> {
    let context = unsafe { JObject::from_raw(env, context_raw.cast()) };
    let app = env
        .call_method(
            &context,
            jni_str!("getApplicationContext"),
            jni_sig!("()Landroid/content/Context;"),
            &[],
        )?
        .l()?;
    let context = if app.is_null() {
        env.new_global_ref(&context)?
    } else {
        env.new_global_ref(&app)?
    };
    let class = bridge(env, context.as_obj())?;
    let listener = env.new_object(class, jni_sig!("()V"), &[])?;
    let tts = env.new_object(
        jni_str!("android/speech/tts/TextToSpeech"),
        jni_sig!("(Landroid/content/Context;Landroid/speech/tts/TextToSpeech$OnInitListener;)V"),
        &[context.as_obj().into(), (&listener).into()],
    )?;
    let rv = env
        .call_method(
            &tts,
            jni_str!("setOnUtteranceProgressListener"),
            jni_sig!("(Landroid/speech/tts/UtteranceProgressListener;)I"),
            &[(&listener).into()],
        )?
        .i()?;
    call_result(rv)?;
    let tts = env.new_global_ref(&tts)?;
    let mut state = locked(&engine.state);
    state.context = Some(Arc::new(context));
    state.tts = Some(Arc::new(tts));
    Ok(())
}

/// Settles everything waiting on the engine with `error`, exactly once each.
fn fail_all(engine: &Arc<AndroidEngine>, error: SpeechError) {
    let (sinks, syntheses) = {
        let mut state = locked(&engine.state);
        state.queued.clear();
        (
            std::mem::take(&mut state.sinks),
            std::mem::take(&mut state.syntheses),
        )
    };
    for (id, sink) in sinks {
        sink(UtteranceEvent {
            utterance_id: id,
            kind: UtteranceEventKind::Error(error.clone()),
        });
    }
    for (_, done) in syntheses {
        let _ = done.send(Err(error.clone()));
    }
}

/// The engine of this process, constructing it (once) if need be. The registration order is the
/// deadlock contract: the slot is filled before JNI runs, and the callbacks never take
/// `CONSTRUCTING`.
fn ensure() -> Result<Arc<AndroidEngine>, SpeechError> {
    if let Some(engine) = active() {
        return Ok(engine);
    }
    let _construction = locked(&CONSTRUCTING);
    if let Some(engine) = active() {
        return Ok(engine);
    }
    let (vm, context_raw) = vm_and_context()?;
    let engine = Arc::new(AndroidEngine {
        vm,
        state: Mutex::new(EngineState::default()),
        ready: Condvar::new(),
    });
    *locked(&ENGINE) = Some(Arc::clone(&engine));
    let built = engine
        .vm
        .attach_current_thread(|env| construct(env, &engine, context_raw));
    let built = built.and_then(|()| replay(&engine));
    if let Err(error) = built {
        // Mark the failure and retire the slot before settling anyone: a sink that reenters a
        // backend call then either reads the failure, or builds a fresh engine through the empty
        // slot — never this thread's CONSTRUCTING, and never the dying engine's queue.
        {
            let mut state = locked(&engine.state);
            state.initialized = Some(Err(error.clone()));
            engine.ready.notify_all();
        }
        {
            let mut slot = locked(&ENGINE);
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &engine))
            {
                *slot = None;
            }
        }
        drop(_construction);
        fail_all(&engine, error.clone());
        return Err(error);
    }
    Ok(engine)
}

/// Blocks until `onInit` answers or the bound passes; a caller on the thread that would deliver
/// `onInit` gets `Backend` after the bound instead of a hang.
fn wait_ready(engine: &Arc<AndroidEngine>) -> Result<(), SpeechError> {
    let mut state = locked(&engine.state);
    if state.initialized.is_none() {
        let (guard, _) = engine
            .ready
            .wait_timeout_while(state, ENGINE_TIMEOUT, |state| state.initialized.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        state = guard;
    }
    state
        .initialized
        .clone()
        .unwrap_or_else(|| Err(SpeechError::new(ErrorKind::Backend)))
}

/// Takes the utterance's sink and runs it outside every lock: a handler may call back in.
fn fire(engine: &Arc<AndroidEngine>, id: UtteranceId, kind: UtteranceEventKind) {
    let sink = locked(&engine.state).sinks.remove(&id);
    if let Some(sink) = sink {
        sink(UtteranceEvent {
            utterance_id: id,
            kind,
        });
    }
}

/// Hands a blocked `synthesize` its answer.
fn complete(engine: &Arc<AndroidEngine>, id: UtteranceId, result: Result<(), SpeechError>) {
    let done = locked(&engine.state).syntheses.remove(&id);
    if let Some(done) = done {
        let _ = done.send(result);
    }
}

/// Restores the stored settings onto a ready engine and speaks the queue. Called from `onInit`
/// (the app's main thread) and from `ensure` (the constructor's thread, for the `onInit` that
/// ran synchronously inside the constructor). Utterances stay queued while the engine is not
/// ready: speaking before `onInit` only fails them.
fn replay(engine: &Arc<AndroidEngine>) -> Result<(), SpeechError> {
    let (initialized, tts) = {
        let state = locked(&engine.state);
        (state.initialized.clone(), state.tts.clone())
    };
    let (Some(tts), Some(Ok(()))) = (tts, initialized) else {
        return Ok(());
    };
    engine
        .vm
        .attach_current_thread(|env| -> Result<(), SpeechError> {
            let (language, voice, rate, pitch, queued) = {
                let mut state = locked(&engine.state);
                (
                    state.language.clone(),
                    state.voice.clone(),
                    state.rate,
                    state.pitch,
                    std::mem::take(&mut state.queued),
                )
            };
            // Restoring settings can fail on an engine that lacks them; the utterances still go out.
            if let Some(language) = language.as_deref() {
                let _ = apply_language(env, tts.as_obj(), language);
            }
            if let Some(voice) = voice.as_deref() {
                let _ = apply_voice(env, tts.as_obj(), voice);
            }
            if let Some(rate) = rate {
                let _ = apply_rate(env, tts.as_obj(), rate);
            }
            if let Some(pitch) = pitch {
                let _ = apply_pitch(env, tts.as_obj(), pitch);
            }
            for utterance in queued {
                if let Err(error) = speak_utterance(
                    env,
                    engine,
                    tts.as_obj(),
                    utterance.id,
                    &utterance.text,
                    utterance.interrupt,
                ) {
                    fire(engine, utterance.id, UtteranceEventKind::Error(error));
                }
            }
            Ok(())
        })
}

/// The per-utterance volume bundle, or `None` to pass the engine a null bundle.
fn volume_bundle<'local>(
    env: &mut Env<'local>,
    volume: Option<f32>,
) -> Result<Option<JObject<'local>>, SpeechError> {
    let Some(volume) = volume else {
        return Ok(None);
    };
    let bundle = env.new_object(jni_str!("android/os/Bundle"), jni_sig!("()V"), &[])?;
    let key = env.new_string(KEY_PARAM_VOLUME)?;
    env.call_method(
        &bundle,
        jni_str!("putFloat"),
        jni_sig!("(Ljava/lang/String;F)V"),
        &[(&key).into(), volume.into()],
    )?;
    Ok(Some(bundle))
}

/// Speaks one utterance now: the stored silence first (its own id, so its callbacks are ignored),
/// then the text, queueing or flushing as asked.
fn speak_utterance(
    env: &mut Env<'_>,
    engine: &Arc<AndroidEngine>,
    tts: &JObject<'_>,
    id: UtteranceId,
    text: &str,
    interrupt: bool,
) -> Result<(), SpeechError> {
    let mode = queue_mode(interrupt);
    let (silence_ms, volume) = {
        let state = locked(&engine.state);
        (state.silence_ms, state.volume)
    };
    if silence_ms > 0 {
        let silence = env.new_string(silence_id(id))?;
        let rv = env
            .call_method(
                tts,
                jni_str!("playSilentUtterance"),
                jni_sig!("(JILjava/lang/String;)I"),
                &[
                    jlong::from(silence_ms).into(),
                    mode.into(),
                    (&silence).into(),
                ],
            )?
            .i()?;
        call_result(rv)?;
    }
    let text = env.new_string(text)?;
    let utterance = env.new_string(speak_id(id))?;
    let bundle = volume_bundle(env, volume)?;
    let null = JObject::null();
    let bundle = match &bundle {
        Some(bundle) => bundle.into(),
        None => (&null).into(),
    };
    let rv = env
        .call_method(
            tts,
            jni_str!("speak"),
            jni_sig!("(Ljava/lang/CharSequence;ILandroid/os/Bundle;Ljava/lang/String;)I"),
            &[(&text).into(), mode.into(), bundle, (&utterance).into()],
        )?
        .i()?;
    call_result(rv)
}

/// One `getVoices` entry as our `Voice`; the Android name is the id `set_voice` takes.
fn voice_of(env: &mut Env<'_>, voice: &JObject<'_>) -> Result<Voice, SpeechError> {
    let name = env
        .call_method(
            voice,
            jni_str!("getName"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let name = env.cast_local::<JString>(name)?.try_to_string(env)?;
    let locale = env
        .call_method(
            voice,
            jni_str!("getLocale"),
            jni_sig!("()Ljava/util/Locale;"),
            &[],
        )?
        .l()?;
    let tag = env
        .call_method(
            &locale,
            jni_str!("toLanguageTag"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let tag = env.cast_local::<JString>(tag)?.try_to_string(env)?;
    let network_required = env
        .call_method(
            voice,
            jni_str!("isNetworkConnectionRequired"),
            jni_sig!("()Z"),
            &[],
        )?
        .z()?;
    let features = env
        .call_method(
            voice,
            jni_str!("getFeatures"),
            jni_sig!("()Ljava/util/Set;"),
            &[],
        )?
        .l()?;
    // A null feature set, or one without the marker, counts as installed, as flutter_tts reads it.
    let installed = if features.is_null() {
        true
    } else {
        let marker = env.new_string(KEY_FEATURE_NOT_INSTALLED)?;
        !env.call_method(
            &features,
            jni_str!("contains"),
            jni_sig!("(Ljava/lang/Object;)Z"),
            &[(&marker).into()],
        )?
        .z()?
    };
    Ok(Voice {
        id: name.clone(),
        name,
        language: tag,
        network_required,
        installed,
    })
}

/// Walks a Java `Set`/`List`; a null collection reads as empty.
fn for_each_element(
    env: &mut Env<'_>,
    collection: &JObject<'_>,
    mut visit: impl FnMut(&mut Env<'_>, JObject<'_>) -> Result<(), SpeechError>,
) -> Result<(), SpeechError> {
    if collection.is_null() {
        return Ok(());
    }
    let iterator = env
        .call_method(
            collection,
            jni_str!("iterator"),
            jni_sig!("()Ljava/util/Iterator;"),
            &[],
        )?
        .l()?;
    while env
        .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
        .z()?
    {
        let element = env
            .call_method(
                &iterator,
                jni_str!("next"),
                jni_sig!("()Ljava/lang/Object;"),
                &[],
            )?
            .l()?;
        if !element.is_null() {
            visit(env, element)?;
        }
    }
    Ok(())
}

/// The engine's voices, in the order it reports them.
fn voices_of(env: &mut Env<'_>, tts: &JObject<'_>) -> Result<Vec<Voice>, SpeechError> {
    let set = env
        .call_method(
            tts,
            jni_str!("getVoices"),
            jni_sig!("()Ljava/util/Set;"),
            &[],
        )?
        .l()?;
    let mut voices = Vec::new();
    for_each_element(env, &set, |env, voice| {
        voices.push(voice_of(env, &voice)?);
        Ok(())
    })?;
    Ok(voices)
}

/// `setLanguage` for a BCP 47 tag, mapping the engine's answer.
fn apply_language(env: &mut Env<'_>, tts: &JObject<'_>, tag: &str) -> Result<(), SpeechError> {
    let tag = env.new_string(tag)?;
    let locale = env
        .call_static_method(
            jni_str!("java/util/Locale"),
            jni_str!("forLanguageTag"),
            jni_sig!("(Ljava/lang/String;)Ljava/util/Locale;"),
            &[(&tag).into()],
        )?
        .l()?;
    if locale.is_null() {
        return Err(SpeechError::new(ErrorKind::LanguageNotSupported));
    }
    let rv = env
        .call_method(
            tts,
            jni_str!("setLanguage"),
            jni_sig!("(Ljava/util/Locale;)I"),
            &[(&locale).into()],
        )?
        .i()?;
    language_result(rv)
}

/// `setVoice` for a voice the engine reports; an unknown name is `VoiceNotFound`.
fn apply_voice(env: &mut Env<'_>, tts: &JObject<'_>, id: &str) -> Result<(), SpeechError> {
    let set = env
        .call_method(
            tts,
            jni_str!("getVoices"),
            jni_sig!("()Ljava/util/Set;"),
            &[],
        )?
        .l()?;
    // The voice object cannot leave the walk (its frame owns it), so setVoice happens inside.
    let mut answer: Option<Result<(), SpeechError>> = None;
    for_each_element(env, &set, |env, voice| {
        if answer.is_some() {
            return Ok(());
        }
        let name = env
            .call_method(
                &voice,
                jni_str!("getName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let name = env.cast_local::<JString>(name)?.try_to_string(env)?;
        if name == id {
            let rv = env
                .call_method(
                    tts,
                    jni_str!("setVoice"),
                    jni_sig!("(Landroid/speech/tts/Voice;)I"),
                    &[(&voice).into()],
                )?
                .i()?;
            answer = Some(language_result(rv));
        }
        Ok(())
    })?;
    answer.unwrap_or_else(|| Err(SpeechError::new(ErrorKind::VoiceNotFound)))
}

fn apply_rate(env: &mut Env<'_>, tts: &JObject<'_>, rate: f32) -> Result<(), SpeechError> {
    let rv = env
        .call_method(
            tts,
            jni_str!("setSpeechRate"),
            jni_sig!("(F)I"),
            &[speech_rate(rate).into()],
        )?
        .i()?;
    call_result(rv)
}

fn apply_pitch(env: &mut Env<'_>, tts: &JObject<'_>, pitch: f32) -> Result<(), SpeechError> {
    let rv = env
        .call_method(tts, jni_str!("setPitch"), jni_sig!("(F)I"), &[pitch.into()])?
        .i()?;
    call_result(rv)
}

// The callbacks receive nothing but their arguments, so they work through the engine slot. Names
// and signatures must stay in step with the `Bridge` dex; see `BRIDGE_METHODS`.
fn on_init(_env: &mut Env, _this: JObject, status: jint) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    let result = init_result(status);
    {
        let mut state = locked(&engine.state);
        state.initialized = Some(result.clone());
        engine.ready.notify_all();
    }
    match result {
        // The queue stays in place; replay() drains it once the engine object is stored.
        Ok(()) => {
            if let Err(error) = replay(&engine) {
                fail_all(&engine, error);
            }
            Ok(())
        }
        // Nothing will speak: every sink and blocked synthesis settles with the failure.
        Err(error) => {
            fail_all(&engine, error);
            Ok(())
        }
    }
}

fn on_start(env: &mut Env, _this: JObject, utterance_id: JString) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    if let Some(Utterance::Speak(id)) = parse_utterance(&utterance_id.try_to_string(env)?) {
        fire(&engine, id, UtteranceEventKind::Start);
    }
    Ok(())
}

fn on_range_start(
    env: &mut Env,
    _this: JObject,
    utterance_id: JString,
    start: jint,
    end: jint,
    _frame: jint,
) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    if let Some(Utterance::Speak(id)) = parse_utterance(&utterance_id.try_to_string(env)?) {
        fire(&engine, id, progress(start, end));
    }
    Ok(())
}

fn on_stop(
    env: &mut Env,
    _this: JObject,
    utterance_id: JString,
    _interrupted: jboolean,
) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    match parse_utterance(&utterance_id.try_to_string(env)?) {
        Some(Utterance::Speak(id)) => fire(&engine, id, UtteranceEventKind::Cancel),
        Some(Utterance::Synthesis(id)) => {
            complete(&engine, id, Err(SpeechError::new(ErrorKind::Cancelled)));
        }
        Some(Utterance::Silence) | None => {}
    }
    Ok(())
}

fn on_done(env: &mut Env, _this: JObject, utterance_id: JString) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    match parse_utterance(&utterance_id.try_to_string(env)?) {
        Some(Utterance::Speak(id)) => fire(&engine, id, UtteranceEventKind::Finish),
        Some(Utterance::Synthesis(id)) => complete(&engine, id, Ok(())),
        Some(Utterance::Silence) | None => {}
    }
    Ok(())
}

fn on_error(env: &mut Env, _this: JObject, utterance_id: JString) -> jni::errors::Result<()> {
    let Some(engine) = active() else {
        return Ok(());
    };
    let error = SpeechError::new(ErrorKind::SynthesisFailed);
    match parse_utterance(&utterance_id.try_to_string(env)?) {
        Some(Utterance::Speak(id)) => fire(&engine, id, UtteranceEventKind::Error(error)),
        Some(Utterance::Synthesis(id)) => complete(&engine, id, Err(error)),
        Some(Utterance::Silence) | None => {}
    }
    Ok(())
}

/// Bound with `RegisterNatives` against the `Bridge` dex; one entry per native method there.
const BRIDGE_METHODS: &[NativeMethod] = &[
    native_method! { fn on_init(status: jint), },
    native_method! { fn on_start(utterance_id: JString), },
    native_method! { fn on_range_start(utterance_id: JString, start: jint, end: jint, frame: jint), },
    native_method! { fn on_stop(utterance_id: JString, interrupted: jboolean), },
    native_method! { fn on_done(utterance_id: JString), },
    native_method! { fn on_error(utterance_id: JString), },
];

pub(crate) struct NativeBackend;

/// Everything below blocks on JNI for the length of the call; the futures it returns are already
/// settled, which keeps the `Backend` contract (resolve exactly once, never hang).
impl Backend for NativeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = ensure().and_then(|engine| wait_ready(&engine));
        Box::pin(ready(result))
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(ready(Ok(vm_and_context().is_ok())))
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let id = NEXT_UTTERANCE_ID.fetch_add(1, Ordering::Relaxed);
            let tts = {
                let mut state = locked(&engine.state);
                if let Some(Err(error)) = &state.initialized {
                    return Err(error.clone());
                }
                state.sinks.insert(id, on_event);
                let ready = can_start_now(state.initialized.as_ref(), state.tts.is_some());
                if ready && let Some(tts) = state.tts.clone() {
                    tts
                } else {
                    // Not ready: the utterance queues and a replay speaks it. An interrupting
                    // utterance queues too — it cannot start yet — and the entries it displaces
                    // settle Cancel now, each exactly once.
                    let cleared = if interrupt {
                        std::mem::take(&mut state.queued)
                    } else {
                        Vec::new()
                    };
                    state.queued.push(Queued {
                        id,
                        text: text.to_owned(),
                        interrupt,
                    });
                    drop(state);
                    for dropped in cleared {
                        fire(&engine, dropped.id, UtteranceEventKind::Cancel);
                    }
                    return Ok(Some(id));
                }
            };
            let spoken = engine.vm.attach_current_thread(|env| {
                speak_utterance(env, &engine, tts.as_obj(), id, text, interrupt)
            });
            match spoken {
                Ok(()) => Ok(Some(id)),
                Err(error) => {
                    fire(&engine, id, UtteranceEventKind::Error(error.clone()));
                    Err(error)
                }
            }
        })();
        Box::pin(ready(result))
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            // Android's stop flushes its queue without reporting the flushed utterances, so every
            // sink settles here; the engine's own callbacks find nothing left to fire.
            let (sinks, syntheses) = {
                let mut state = locked(&engine.state);
                state.queued.clear();
                (
                    std::mem::take(&mut state.sinks),
                    std::mem::take(&mut state.syntheses),
                )
            };
            for (id, sink) in sinks {
                sink(UtteranceEvent {
                    utterance_id: id,
                    kind: UtteranceEventKind::Cancel,
                });
            }
            for (_, done) in syntheses {
                let _ = done.send(Err(SpeechError::new(ErrorKind::Cancelled)));
            }
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else { return Ok(()) };
            engine
                .vm
                .attach_current_thread(|env| -> Result<(), SpeechError> {
                    let rv = env
                        .call_method(tts.as_obj(), jni_str!("stop"), jni_sig!("()I"), &[])?
                        .i()?;
                    call_result(rv)
                })
        })();
        Box::pin(ready(result))
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        // Android's TTS has no pause; a coded failure beats pretending.
        Box::pin(ready(Err(SpeechError::new(ErrorKind::Backend))))
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        // Android's TTS has no resume either.
        Box::pin(ready(Err(SpeechError::new(ErrorKind::Backend))))
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else { return Ok(false) };
            engine.vm.attach_current_thread(|env| {
                Ok(env
                    .call_method(tts.as_obj(), jni_str!("isSpeaking"), jni_sig!("()Z"), &[])?
                    .z()?)
            })
        })();
        Box::pin(ready(result))
    }

    fn synthesize(&self, text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            wait_ready(&engine)?;
            let id = NEXT_UTTERANCE_ID.fetch_add(1, Ordering::Relaxed);
            let (tts, context, volume) = {
                let state = locked(&engine.state);
                (state.tts.clone(), state.context.clone(), state.volume)
            };
            // Both are stored together at the end of construction; absent only in the brief
            // window where another thread's construction is still running.
            let (Some(tts), Some(context)) = (tts, context) else {
                return Err(SpeechError::new(ErrorKind::NotAvailable));
            };
            // The engine only synthesizes to a file; the cache directory, so the OS evicts what
            // a hard kill leaves behind.
            let path = engine
                .vm
                .attach_current_thread(|env| -> Result<String, SpeechError> {
                    let dir = env
                        .call_method(
                            context.as_obj(),
                            jni_str!("getCacheDir"),
                            jni_sig!("()Ljava/io/File;"),
                            &[],
                        )?
                        .l()?;
                    let dir = env
                        .call_method(
                            &dir,
                            jni_str!("getAbsolutePath"),
                            jni_sig!("()Ljava/lang/String;"),
                            &[],
                        )?
                        .l()?;
                    let dir = env.cast_local::<JString>(dir)?.try_to_string(env)?;
                    Ok(format!("{dir}/nsh-tts-synthesis-{id}.wav"))
                })?;
            let (done, completion) = mpsc::channel();
            locked(&engine.state).syntheses.insert(id, done);
            let started = engine
                .vm
                .attach_current_thread(|env| -> Result<(), SpeechError> {
                    let bundle = volume_bundle(env, volume)?;
                    let text = env.new_string(text)?;
                    let path = env.new_string(&path)?;
                    let file = env.new_object(
                        jni_str!("java/io/File"),
                        jni_sig!("(Ljava/lang/String;)V"),
                        &[(&path).into()],
                    )?;
                    let utterance = env.new_string(synth_id(id))?;
                    let null = JObject::null();
                    let bundle = match &bundle {
                        Some(bundle) => bundle.into(),
                        None => (&null).into(),
                    };
                    let rv = env
                        .call_method(
                            tts.as_obj(),
                            jni_str!("synthesizeToFile"),
                            jni_sig!(
                                "(Ljava/lang/CharSequence;Landroid/os/Bundle;Ljava/io/File;Ljava/lang/String;)I"
                            ),
                            &[(&text).into(), bundle, (&file).into(), (&utterance).into()],
                        )?
                        .i()?;
                    call_result(rv)
                });
            let answer = match started {
                Ok(()) => match completion.recv_timeout(SYNTHESIS_TIMEOUT) {
                    Ok(Ok(())) => std::fs::read(&path)
                        .map_err(|_| SpeechError::new(ErrorKind::SynthesisFailed)),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(SpeechError::new(ErrorKind::Backend)),
                },
                Err(error) => Err(error),
            };
            if answer.is_err() {
                locked(&engine.state).syntheses.remove(&id);
            }
            let _ = std::fs::remove_file(&path);
            answer
        })();
        Box::pin(ready(result))
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                return Ok(Vec::new());
            };
            engine
                .vm
                .attach_current_thread(|env| voices_of(env, tts.as_obj()))
        })();
        Box::pin(ready(result))
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                return Ok(Vec::new());
            };
            engine
                .vm
                .attach_current_thread(|env| -> Result<Vec<Engine>, SpeechError> {
                    let default = env
                        .call_method(
                            tts.as_obj(),
                            jni_str!("getDefaultEngine"),
                            jni_sig!("()Ljava/lang/String;"),
                            &[],
                        )?
                        .l()?;
                    let default = if default.is_null() {
                        None
                    } else {
                        Some(env.cast_local::<JString>(default)?.try_to_string(env)?)
                    };
                    let list = env
                        .call_method(
                            tts.as_obj(),
                            jni_str!("getEngines"),
                            jni_sig!("()Ljava/util/List;"),
                            &[],
                        )?
                        .l()?;
                    let mut engines = Vec::new();
                    for_each_element(env, &list, |env, info| {
                        // EngineInfo exposes public fields, not getters.
                        let name = env
                            .get_field(&info, jni_str!("name"), jni_sig!("Ljava/lang/String;"))?
                            .l()?;
                        let name = if name.is_null() {
                            return Ok(());
                        } else {
                            env.cast_local::<JString>(name)?.try_to_string(env)?
                        };
                        let label = env
                            .get_field(&info, jni_str!("label"), jni_sig!("Ljava/lang/String;"))?
                            .l()?;
                        let label = if label.is_null() {
                            String::new()
                        } else {
                            env.cast_local::<JString>(label)?.try_to_string(env)?
                        };
                        engines.push(Engine {
                            default: default.as_deref() == Some(name.as_str()),
                            label,
                            name,
                        });
                        Ok(())
                    })?;
                    Ok(engines)
                })
        })();
        Box::pin(ready(result))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                return Ok(Vec::new());
            };
            engine
                .vm
                .attach_current_thread(|env| -> Result<Vec<String>, SpeechError> {
                    let mut languages = Vec::new();
                    for voice in voices_of(env, tts.as_obj())? {
                        if !languages.contains(&voice.language) {
                            languages.push(voice.language);
                        }
                    }
                    Ok(languages)
                })
        })();
        Box::pin(ready(result))
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            engine
                .vm
                .attach_current_thread(|env| -> Result<Option<usize>, SpeechError> {
                    let rv = env
                        .call_static_method(
                            jni_str!("android/speech/tts/TextToSpeech"),
                            jni_str!("getMaxSpeechInputLength"),
                            jni_sig!("()I"),
                            &[],
                        )?
                        .i()?;
                    Ok(max_length(rv))
                })
        })();
        Box::pin(ready(result))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                locked(&engine.state).language = Some(tag.to_owned());
                return Ok(());
            };
            let applied = engine
                .vm
                .attach_current_thread(|env| apply_language(env, tts.as_obj(), tag));
            if applied.is_ok() {
                locked(&engine.state).language = Some(tag.to_owned());
            }
            applied
        })();
        Box::pin(ready(result))
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                locked(&engine.state).voice = Some(id.to_owned());
                return Ok(());
            };
            let applied = engine
                .vm
                .attach_current_thread(|env| apply_voice(env, tts.as_obj(), id));
            if applied.is_ok() {
                locked(&engine.state).voice = Some(id.to_owned());
            }
            applied
        })();
        Box::pin(ready(result))
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                locked(&engine.state).rate = Some(rate);
                return Ok(());
            };
            let applied = engine
                .vm
                .attach_current_thread(|env| apply_rate(env, tts.as_obj(), rate));
            if applied.is_ok() {
                locked(&engine.state).rate = Some(rate);
            }
            applied
        })();
        Box::pin(ready(result))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        let result = (|| {
            let engine = ensure()?;
            let tts = locked(&engine.state).tts.clone();
            let Some(tts) = tts else {
                locked(&engine.state).pitch = Some(pitch);
                return Ok(());
            };
            let applied = engine
                .vm
                .attach_current_thread(|env| apply_pitch(env, tts.as_obj(), pitch));
            if applied.is_ok() {
                locked(&engine.state).pitch = Some(pitch);
            }
            applied
        })();
        Box::pin(ready(result))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        // The engine takes volume per utterance, in the speak bundle, not up front.
        let result = ensure().map(|engine| locked(&engine.state).volume = Some(volume));
        Box::pin(ready(result))
    }

    fn set_silence(&self, ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        // Spoken as a silent utterance ahead of each queued text; 0 starts immediately.
        let result = ensure().map(|engine| locked(&engine.state).silence_ms = ms);
        Box::pin(ready(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_mode_flushes_on_interrupt_and_adds_on_queue() {
        assert_eq!(queue_mode(true), QUEUE_FLUSH);
        assert_eq!(queue_mode(false), QUEUE_ADD);
    }

    #[test]
    fn speech_rate_doubles_so_half_is_normal() {
        assert!((speech_rate(0.5) - 1.0).abs() < f32::EPSILON);
        assert!(speech_rate(0.0).abs() < f32::EPSILON);
        assert!((speech_rate(1.0) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn language_result_maps_engine_codes() {
        assert!(language_result(0).is_ok());
        assert!(language_result(1).is_ok());
        assert!(language_result(2).is_ok());
        assert_eq!(
            language_result(LANG_MISSING_DATA).unwrap_err().kind(),
            ErrorKind::LanguageMissingData
        );
        assert_eq!(
            language_result(LANG_NOT_SUPPORTED).unwrap_err().kind(),
            ErrorKind::LanguageNotSupported
        );
        let other = language_result(-3).unwrap_err();
        assert_eq!(other.kind(), ErrorKind::Backend);
        assert_eq!(other.status(), Some(-3));
    }

    #[test]
    fn init_result_maps_failure_to_engine_missing() {
        assert!(init_result(0).is_ok());
        assert_eq!(
            init_result(-1).unwrap_err().kind(),
            ErrorKind::EngineMissing
        );
    }

    #[test]
    fn call_result_carries_the_engine_status() {
        assert!(call_result(0).is_ok());
        let error = call_result(-2).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Backend);
        assert_eq!(error.status(), Some(-2));
    }

    #[test]
    fn utterance_ids_round_trip() {
        assert_eq!(parse_utterance(&speak_id(7)), Some(Utterance::Speak(7)));
        assert_eq!(parse_utterance(&synth_id(9)), Some(Utterance::Synthesis(9)));
        assert_eq!(parse_utterance(&silence_id(7)), Some(Utterance::Silence));
        assert_eq!(parse_utterance("not a number"), None);
        assert_eq!(parse_utterance(""), None);
    }

    #[test]
    fn progress_keeps_range_and_clamps_negative() {
        assert_eq!(
            progress(4, 10),
            UtteranceEventKind::Progress {
                location: 4,
                length: 6
            }
        );
        assert_eq!(
            progress(-1, 5),
            UtteranceEventKind::Progress {
                location: 0,
                length: 6
            }
        );
    }

    #[test]
    fn max_length_reports_none_for_a_negative_answer() {
        assert_eq!(max_length(4000), Some(4000));
        assert_eq!(max_length(0), Some(0));
        assert_eq!(max_length(-1), None);
        assert_eq!(max_length(jint::MIN), None);
    }

    #[test]
    fn speak_starts_only_once_on_init_succeeded_and_object_stored() {
        assert!(!can_start_now(None, true));
        assert!(!can_start_now(Some(&Ok(())), false));
        assert!(can_start_now(Some(&Ok(())), true));
        let failed = Err(SpeechError::new(ErrorKind::EngineMissing));
        assert!(!can_start_now(Some(&failed), true));
    }
}
