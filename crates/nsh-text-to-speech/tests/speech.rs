//! `Speech` through the fake backend: one completion per call, the queue, cancellation, the
//! settings' limits and the stable error codes.

use std::{
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use nsh_text_to_speech::{
    Backend, BackendFuture, Engine, ErrorKind, EventSink, FakeBackend, Speech, SpeechError,
    UtteranceEvent, UtteranceEventKind, Voice,
};

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            // Backend futures are not Send, so concurrent calls spawn on the local set.
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

fn speech_with_fake() -> (Speech, FakeBackend) {
    let fake = FakeBackend::new();
    (Speech::with_backend(Box::new(fake.clone())), fake)
}

/// Collects the events the handler receives.
fn collector(speech: &Speech) -> Arc<Mutex<Vec<UtteranceEvent>>> {
    let events: Arc<Mutex<Vec<UtteranceEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    speech.set_event_handler(move |event| sink.lock().unwrap().push(event));
    events
}

fn kinds(events: &[UtteranceEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match &event.kind {
            UtteranceEventKind::Start => "start".to_owned(),
            UtteranceEventKind::Progress { .. } => "progress".to_owned(),
            UtteranceEventKind::Finish => "finish".to_owned(),
            UtteranceEventKind::Cancel => "cancel".to_owned(),
            UtteranceEventKind::Error(_) => "error".to_owned(),
        })
        .collect()
}

#[test]
fn speaks_and_finishes_once() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        let id = speech.speak("Hello", true).await.unwrap();
        assert_eq!(id, 1);
        assert_eq!(fake.pending_id(), Some(1));
        assert!(speech.is_speaking().await.unwrap());

        assert!(fake.finish_utterance());
        assert!(!speech.is_speaking().await.unwrap());
        // A second finish has nothing to settle: the utterance finished exactly once.
        assert!(!fake.finish_utterance());

        let events = events.lock().unwrap();
        assert_eq!(kinds(&events), ["start", "finish"]);
        assert!(events.iter().all(|event| event.utterance_id == 1));
    });
}

#[test]
fn speak_rejects_empty_text() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.speak("", true).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert_eq!(fake.speaks(), 0, "the backend is never asked");
    });
}

#[test]
fn speak_rejects_blank_text() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.speak(" \n\t ", true).await.unwrap_err();
        assert_eq!(error.code(), "ERR_TTS_INVALID_ARGUMENT");
        assert_eq!(fake.speaks(), 0);
    });
}

#[test]
fn speak_rejects_oversize_text() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_max_speech_input_length(Some(10));
        let error = speech.speak(&"a".repeat(11), true).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert_eq!(fake.speaks(), 0);

        // The limit itself still speaks: it rejects only what runs past it.
        let id = speech.speak(&"a".repeat(10), true).await.unwrap();
        assert_eq!(id, 1);
        assert!(fake.finish_utterance());
    });
}

#[test]
fn speak_queues_while_speaking() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        assert_eq!(speech.speak("one", false).await.unwrap(), 1);
        assert_eq!(speech.speak("two", false).await.unwrap(), 2);
        assert_eq!(fake.pending_id(), Some(1));
        assert_eq!(fake.queued(), 1);
        assert!(speech.is_speaking().await.unwrap());

        // The first utterance finishing starts the queued one, in order.
        assert!(fake.finish_utterance());
        assert_eq!(fake.pending_id(), Some(2));
        assert!(fake.finish_utterance());
        assert!(!speech.is_speaking().await.unwrap());

        let events = events.lock().unwrap();
        assert_eq!(kinds(&events), ["start", "finish", "start", "finish"]);
        assert_eq!(
            events.iter().map(|e| e.utterance_id).collect::<Vec<_>>(),
            [1, 1, 2, 2]
        );
    });
}

#[test]
fn speak_interrupts_current_utterance() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        assert_eq!(speech.speak("one", false).await.unwrap(), 1);

        let two = speech.speak("two", true).await.unwrap();
        assert_eq!(two, 2);
        assert_eq!(fake.pending_id(), Some(2));
        assert_eq!(fake.queued(), 0, "the interrupting utterance runs at once");

        let events = events.lock().unwrap();
        assert_eq!(kinds(&events), ["start", "cancel", "start"]);
        assert_eq!(
            events.iter().map(|e| e.utterance_id).collect::<Vec<_>>(),
            [1, 1, 2]
        );
        drop(events);
        assert!(fake.finish_utterance());
    });
}

#[test]
fn awaited_speak_resolves_once_on_finish() {
    let (speech, fake) = speech_with_fake();
    run(async {
        speech.await_speak_completion(true);
        let call = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.speak("Hello", false).await })
        };
        settle(|| fake.pending_id().is_some()).await;
        assert!(!call.is_finished(), "the call waits for its utterance");

        assert!(fake.finish_utterance());
        assert_eq!(call.await.unwrap().unwrap(), 1);
        assert!(!fake.finish_utterance(), "the call settled exactly once");
    });
}

#[test]
fn stop_cancels_awaited_speak_once() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        speech.await_speak_completion(true);
        let call = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.speak("Hello", false).await })
        };
        settle(|| fake.pending_id().is_some()).await;

        speech.stop().await.unwrap();
        let error = call.await.unwrap().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Cancelled);
        assert_eq!(error.code(), "ERR_TTS_CANCELLED");
        assert_eq!(fake.pending_id(), None);
        assert!(!fake.finish_utterance(), "nothing is left to settle");

        let events = events.lock().unwrap();
        assert_eq!(kinds(&events), ["start", "cancel"]);
    });
}

#[test]
fn pause_and_resume_round_trip() {
    let (speech, fake) = speech_with_fake();
    run(async {
        speech.speak("Hello", false).await.unwrap();
        speech.pause().await.unwrap();
        speech.resume().await.unwrap();
        assert_eq!(fake.pauses(), 1);
        assert_eq!(fake.resumes(), 1);
        assert!(speech.is_speaking().await.unwrap());
        assert!(fake.finish_utterance());
    });
}

#[test]
fn pause_when_idle_reports_not_speaking() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.pause().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotSpeaking);
        assert_eq!(error.code(), "ERR_TTS_NOT_SPEAKING");
        assert_eq!(fake.pauses(), 0, "the backend is never asked");
    });
}

#[test]
fn resume_when_not_paused_reports_not_speaking() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.resume().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotSpeaking);
        assert_eq!(fake.resumes(), 0);
    });
}

#[test]
fn set_rate_rejects_out_of_range() {
    let (speech, fake) = speech_with_fake();
    run(async {
        for rate in [1.5, -0.1, f32::NAN] {
            let error = speech.set_speech_rate(rate).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "rate {rate}");
            assert_eq!(fake.last_rate(), None);
        }
        speech.set_speech_rate(0.0).await.unwrap();
        speech.set_speech_rate(1.0).await.unwrap();
        assert_eq!(fake.last_rate(), Some(1.0));
    });
}

#[test]
fn set_pitch_rejects_out_of_range() {
    let (speech, fake) = speech_with_fake();
    run(async {
        for pitch in [0.4, 2.1] {
            let error = speech.set_pitch(pitch).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "pitch {pitch}");
            assert_eq!(fake.last_pitch(), None);
        }
        speech.set_pitch(0.5).await.unwrap();
        speech.set_pitch(2.0).await.unwrap();
        assert_eq!(fake.last_pitch(), Some(2.0));
    });
}

#[test]
fn set_volume_rejects_out_of_range() {
    let (speech, fake) = speech_with_fake();
    run(async {
        for volume in [-0.1, 1.1] {
            let error = speech.set_volume(volume).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "volume {volume}");
            assert_eq!(fake.last_volume(), None);
        }
        speech.set_volume(0.0).await.unwrap();
        speech.set_volume(1.0).await.unwrap();
        assert_eq!(fake.last_volume(), Some(1.0));
    });
}

#[test]
fn set_language_rejects_empty() {
    let (speech, fake) = speech_with_fake();
    run(async {
        for tag in ["", "   "] {
            let error = speech.set_language(tag).await.unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "tag {tag:?}");
            assert_eq!(fake.last_language(), None);
        }
    });
}

#[test]
fn unknown_language_reports_unavailable() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_languages(vec!["en-US".to_owned()]);
        let error = speech.set_language("xx-XX").await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::LanguageNotSupported);
        assert_eq!(error.code(), "ERR_TTS_LANGUAGE_NOT_SUPPORTED");

        assert!(!speech.is_language_available("xx-XX").await);
        assert!(speech.is_language_available("en-US").await);
        // Tags match as BCP 47 does: the case does not matter.
        assert!(speech.is_language_available("en-us").await);

        speech.set_language("en-US").await.unwrap();
        assert_eq!(fake.last_language().as_deref(), Some("en-US"));
    });
}

#[test]
fn set_unknown_voice_reports_voice_not_found() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.set_voice("no-such-voice").await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::VoiceNotFound);
        assert_eq!(error.code(), "ERR_TTS_VOICE_NOT_FOUND");
        assert_eq!(fake.last_voice(), None, "the backend is never asked");
    });
}

#[test]
fn uninstalled_voice_reports_voice_not_found() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let error = speech.set_voice("fake-net-voice").await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::VoiceNotFound);
        assert_eq!(fake.last_voice(), None);

        // An installed voice still passes.
        speech.set_voice("fake-voice").await.unwrap();
        assert_eq!(fake.last_voice().as_deref(), Some("fake-voice"));
    });
}

#[test]
fn init_reports_missing_engine() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_init(Err(SpeechError::new(ErrorKind::EngineMissing)));
        let error = speech.init().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::EngineMissing);
        assert_eq!(error.code(), "ERR_TTS_ENGINE_MISSING");
    });
}

#[test]
fn backend_failure_carries_status() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_init(Err(SpeechError::new(ErrorKind::Backend).with_status(500)));
        let error = speech.init().await.unwrap_err();
        assert_eq!(error.code(), "ERR_TTS_BACKEND");
        assert_eq!(error.status(), Some(500));
    });
}

#[test]
fn concurrent_synthesize_reports_busy() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.hold_synthesize();
        let held = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.synthesize("held text").await })
        };
        settle(|| fake.synth_calls() == 1).await;

        let busy = speech.synthesize("second text").await.unwrap_err();
        assert_eq!(busy.kind(), ErrorKind::Busy);
        assert_eq!(busy.code(), "ERR_TTS_BUSY");

        fake.release_synthesize();
        let bytes = held.await.unwrap().unwrap();
        assert!(!bytes.is_empty());
        // The gate is free again once the first call answered.
        assert!(speech.synthesize("third text").await.is_ok());
    });
}

#[test]
fn synthesize_returns_wav_bytes() {
    let (speech, _fake) = speech_with_fake();
    run(async {
        let bytes = speech.synthesize("hello").await.unwrap();
        assert!(bytes.starts_with(b"RIFF"), "a WAV starts with RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.len() >= 44);

        let error = speech.synthesize("").await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    });
}

#[test]
fn progress_events_report_location() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        speech.speak("Hello there", false).await.unwrap();

        assert!(fake.progress_utterance(5, 11));
        assert!(fake.finish_utterance());

        let events = events.lock().unwrap();
        let progress = events
            .iter()
            .find(|event| matches!(event.kind, UtteranceEventKind::Progress { .. }))
            .expect("a progress event arrived");
        assert_eq!(progress.utterance_id, 1);
        match &progress.kind {
            UtteranceEventKind::Progress { location, length } => {
                assert_eq!((*location, *length), (5, 11));
            }
            other => panic!("not a progress event: {other:?}"),
        }
    });
}

#[test]
fn set_silence_reaches_backend() {
    let (speech, fake) = speech_with_fake();
    run(async {
        speech.set_silence(250).await.unwrap();
        assert_eq!(fake.last_silence(), Some(250));
        // 0 means no leading silence at all, not an error.
        speech.set_silence(0).await.unwrap();
        assert_eq!(fake.last_silence(), Some(0));
    });
}

#[test]
fn error_codes_are_stable_and_unique() {
    const ALL: [(ErrorKind, &str); 11] = [
        (ErrorKind::NotAvailable, "ERR_TTS_NOT_AVAILABLE"),
        (ErrorKind::EngineMissing, "ERR_TTS_ENGINE_MISSING"),
        (ErrorKind::VoiceNotFound, "ERR_TTS_VOICE_NOT_FOUND"),
        (
            ErrorKind::LanguageNotSupported,
            "ERR_TTS_LANGUAGE_NOT_SUPPORTED",
        ),
        (
            ErrorKind::LanguageMissingData,
            "ERR_TTS_LANGUAGE_MISSING_DATA",
        ),
        (ErrorKind::NotSpeaking, "ERR_TTS_NOT_SPEAKING"),
        (ErrorKind::InvalidArgument, "ERR_TTS_INVALID_ARGUMENT"),
        (ErrorKind::Busy, "ERR_TTS_BUSY"),
        (ErrorKind::Cancelled, "ERR_TTS_CANCELLED"),
        (ErrorKind::SynthesisFailed, "ERR_TTS_SYNTHESIS_FAILED"),
        (ErrorKind::Backend, "ERR_TTS_BACKEND"),
    ];
    let mut codes: Vec<&str> = Vec::new();
    for (kind, code) in ALL {
        assert_eq!(kind.code(), code);
        assert_eq!(SpeechError::new(kind).code(), code);
        assert!(!kind.message().is_empty());
        assert!(!SpeechError::new(kind).to_string().is_empty());
        codes.push(code);
    }
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), ALL.len(), "every code is its own");
}

#[test]
fn missing_voice_list_is_empty() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_voices(Err(SpeechError::new(ErrorKind::Backend)));
        assert!(speech.voices().await.is_empty());
        // With no list to check against, no voice can be known.
        let error = speech.set_voice("fake-voice").await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::VoiceNotFound);
    });
}

#[test]
fn a_speak_that_fails_to_start_reports_its_error() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        fake.fail_next_speak(SpeechError::new(ErrorKind::Backend).with_status(-9));
        let error = speech.speak("Hello", true).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Backend);
        assert_eq!(error.status(), Some(-9));
        // Nothing started and nothing was left speaking; the next speak is a clean one.
        assert_eq!(fake.pending_id(), None);
        assert!(events.lock().unwrap().is_empty());
        assert!(speech.speak("Hello", true).await.is_ok());
        assert!(fake.finish_utterance());
    });
}

#[test]
fn awaited_speak_resolves_once_on_error() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let events = collector(&speech);
        speech.await_speak_completion(true);
        let call = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.speak("Hello", false).await })
        };
        settle(|| fake.pending_id().is_some()).await;

        assert!(fake.error_utterance(SpeechError::new(ErrorKind::SynthesisFailed)));
        let error = call.await.unwrap().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::SynthesisFailed);
        assert_eq!(error.code(), "ERR_TTS_SYNTHESIS_FAILED");
        // A second answer has nothing to settle: the call resolved exactly once.
        assert!(!fake.error_utterance(SpeechError::new(ErrorKind::Backend)));

        let events = events.lock().unwrap();
        assert_eq!(kinds(&events), ["start", "error"]);
    });
}

#[test]
fn cancelled_synthesize_frees_the_gate() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.hold_synthesize();
        let held = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.synthesize("held text").await })
        };
        settle(|| fake.synth_calls() == 1).await;

        // The caller gives up on the call; dropping its future must free the gate.
        held.abort();
        let _ = held.await;
        fake.release_synthesize();
        let answer = speech.synthesize("again").await;
        assert!(
            answer.is_ok(),
            "a cancelled synthesize must not poison later ones: {answer:?}"
        );
    });
}

/// A backend whose platform gives speak no id: `None` back, and a stamp of its own choosing
/// on every event, so the ids never match what Speech numbered.
#[derive(Clone)]
struct IdLessBackend {
    sink: Arc<Mutex<Option<EventSink>>>,
}

impl IdLessBackend {
    fn new() -> Self {
        Self {
            sink: Arc::new(Mutex::new(None)),
        }
    }

    /// Ends the running utterance, stamped as the platform chose.
    fn finish_with(&self, id: u64) -> bool {
        let sink = self.sink.lock().unwrap().take();
        sink.map(|sink| {
            sink(UtteranceEvent {
                utterance_id: id,
                kind: UtteranceEventKind::Finish,
            })
        })
        .is_some()
    }
}

fn trivial<T: 'static>(answer: T) -> BackendFuture<'static, Result<T, SpeechError>> {
    Box::pin(ready(Ok(answer)))
}

impl Backend for IdLessBackend {
    fn speak(
        &self,
        _text: &str,
        _interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        *self.sink.lock().unwrap() = Some(on_event);
        Box::pin(ready(Ok(None)))
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        trivial(None)
    }

    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        trivial(true)
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        trivial(false)
    }

    fn synthesize(&self, _text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        trivial(Vec::new())
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        trivial(Vec::new())
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        trivial(Vec::new())
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        trivial(Vec::new())
    }

    fn set_language(&self, _tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn set_voice(&self, _id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn set_rate(&self, _rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn set_pitch(&self, _pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn set_volume(&self, _volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }

    fn set_silence(&self, _ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        trivial(())
    }
}

#[test]
fn a_backend_without_ids_reports_one_id() {
    let backend = IdLessBackend::new();
    let speech = Speech::with_backend(Box::new(backend.clone()));
    run(async {
        // Unawaited: the id Speech numbered, not the platform's stamp.
        assert_eq!(speech.speak("one", false).await.unwrap(), 1);
        assert!(backend.finish_with(999));

        // Awaited: the same numbering, whatever the platform stamped on its finish.
        speech.await_speak_completion(true);
        let call = {
            let speech = speech.clone();
            tokio::task::spawn_local(async move { speech.speak("two", false).await })
        };
        settle(|| backend.sink.lock().unwrap().is_some()).await;
        assert!(backend.finish_with(999));
        assert_eq!(call.await.unwrap().unwrap(), 2);
    });
}

#[test]
fn the_fake_defaults_to_the_same_device_as_new() {
    let fake = FakeBackend::default();
    run(async {
        assert_eq!(fake.voices().await.unwrap().len(), 2);
        assert_eq!(fake.max_speech_input_length().await.unwrap(), Some(4000));
    });
}

#[test]
fn availability_is_scriptable() {
    let fake = FakeBackend::new();
    run(async {
        assert!(fake.is_available().await.unwrap());
        fake.set_available(Ok(false));
        assert!(!fake.is_available().await.unwrap());
        fake.set_available(Err(SpeechError::new(ErrorKind::NotAvailable)));
        assert_eq!(
            fake.is_available().await.unwrap_err().kind(),
            ErrorKind::NotAvailable
        );
    });
}

#[test]
fn engines_report_what_the_device_has() {
    let (speech, fake) = speech_with_fake();
    run(async {
        let other = Engine {
            name: "com.other.tts".to_owned(),
            label: "Other TTS".to_owned(),
            default: false,
        };
        fake.set_engines(Ok(vec![other.clone()]));
        assert_eq!(speech.engines().await, vec![other]);
        // A list the platform failed to give is an empty one, not a fault.
        fake.set_engines(Err(SpeechError::new(ErrorKind::Backend)));
        assert!(speech.engines().await.is_empty());
    });
}

#[test]
fn synthesize_answers_what_the_device_is_scripted_with() {
    let (speech, fake) = speech_with_fake();
    run(async {
        fake.set_synthesize(Err(SpeechError::new(ErrorKind::SynthesisFailed)));
        assert_eq!(
            speech.synthesize("hello").await.unwrap_err().kind(),
            ErrorKind::SynthesisFailed
        );
        fake.set_synthesize(Ok(vec![7; 8]));
        assert_eq!(speech.synthesize("hello").await.unwrap(), vec![7; 8]);
    });
}

#[test]
fn the_fake_records_what_reached_it() {
    let (speech, fake) = speech_with_fake();
    run(async {
        assert_eq!(fake.stops(), 0);
        speech.stop().await.unwrap();
        assert_eq!(fake.stops(), 1);

        speech.speak("Hello", true).await.unwrap();
        assert_eq!(fake.last_text().as_deref(), Some("Hello"));
        assert_eq!(fake.last_interrupt(), Some(true));
        // A second utterance while one runs queues instead of interrupting.
        speech.speak("Again", false).await.unwrap();
        assert_eq!(fake.last_interrupt(), Some(false));
    });
}

#[test]
fn progress_without_an_utterance_reports_nothing() {
    let fake = FakeBackend::new();
    assert!(!fake.progress_utterance(1, 2));
}

#[test]
fn languages_list_what_the_device_offers() {
    let (speech, _fake) = speech_with_fake();
    run(async {
        assert_eq!(speech.languages().await, ["en-US", "en-GB"]);
        // BCP 47 tags match whatever their case.
        assert!(speech.is_language_available("EN-gb").await);
        assert!(!speech.is_language_available("fr-FR").await);
    });
}

#[test]
fn max_speech_input_length_reports_the_limit_or_none() {
    let (speech, fake) = speech_with_fake();
    run(async {
        assert_eq!(speech.max_speech_input_length().await, Some(4000));
        fake.set_max_speech_input_length(None);
        assert_eq!(speech.max_speech_input_length().await, None);
    });
}

#[test]
#[cfg(target_vendor = "apple")]
fn speech_new_takes_the_platform_backend() {
    // Construction asks the platform for its backend and nothing else; no audio.
    assert!(Speech::new().is_ok());
}
