//! Web: the Web Speech API, through `window.speechSynthesis`.
//!
//! The browser's `cancel` clears its whole queue, so this backend keeps the queue itself: one
//! utterance is handed to the browser at a time, and each utterance's callbacks settle it exactly
//! once. Interrupt and `stop` settle the cancelled utterance themselves — Chrome reports
//! `canceled` only after the call returned — and the terminal guard drops whatever arrives late.
//! Every hand-over waits one macrotask, because Chromium swallows a `speak` issued in the same
//! task as a `cancel`, leaving the utterance without events; while an utterance is live, a
//! heartbeat resumes the browser every few seconds, because Chrome stops firing events partway
//! through long speech on remote voices until it is resumed. The mapping tests below compile for
//! `wasm32-unknown-unknown` and would run in a browser; this workflow does not drive browsers
//! (AGENTS.md forbids local simulators), so CI compile-checks them only.

use std::{cell::RefCell, collections::VecDeque, future::ready, rc::Rc};

use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::{
    SpeechSynthesis, SpeechSynthesisEvent, SpeechSynthesisUtterance, SpeechSynthesisVoice,
};

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice},
};

#[wasm_bindgen]
unsafe extern "C" {
    /// The browser's SpeechSynthesisErrorEvent, declared here for its `error` field alone:
    /// web-sys gates the typed getter behind a feature, and the field's wire value is the
    /// code string itself — an unknown future code still maps, through the catch-all arm.
    #[wasm_bindgen(js_name = "SpeechSynthesisErrorEvent")]
    type ErrorEvent;

    #[wasm_bindgen(method, getter)]
    fn error(this: &ErrorEvent) -> String;
}

pub(crate) struct NativeBackend;

/// What the app set, applied when an utterance is built. An unset setting keeps the browser's
/// default, so a fresh backend speaks at the browser's normal rate.
#[derive(Clone)]
struct Settings {
    rate: Option<f32>,
    pitch: Option<f32>,
    volume: Option<f32>,
    language: Option<String>,
    voice: Option<String>,
}

/// One utterance waiting for the browser.
struct Waiting {
    id: UtteranceId,
    text: String,
    sink: EventSink,
}

/// One utterance the browser may still speak. `terminal` keeps a late browser report from
/// settling the utterance twice.
struct Live {
    sink: Rc<dyn Fn(UtteranceEvent) + Send>,
    terminal: bool,
}

struct WebState {
    settings: Settings,
    next_id: UtteranceId,
    current: Option<(UtteranceId, Rc<RefCell<Live>>)>,
    queue: VecDeque<Waiting>,
    paused: bool,
}

std::thread_local! {
    // The page has one speechSynthesis; every Speech shares it, so the queue behind it is one
    // too. Wasm runs on one thread, so the queue needs no locks — only short borrows: nothing
    // borrows the state while a sink runs or the browser is called.
    static STATE: RefCell<WebState> = const { RefCell::new(WebState {
        settings: Settings {
            rate: None,
            pitch: None,
            volume: None,
            language: None,
            voice: None,
        },
        next_id: 1,
        current: None,
        queue: VecDeque::new(),
        paused: false,
    }) };
}

fn with_state<T>(run: impl FnOnce(&mut WebState) -> T) -> T {
    STATE.with(|state| run(&mut state.borrow_mut()))
}

/// The browser's speech synthesis, when this page has one.
fn synthesis() -> Option<SpeechSynthesis> {
    web_sys::window().and_then(|window| {
        window
            .speech_synthesis()
            .ok()
            // Old browsers leave the attribute undefined rather than absent.
            .filter(|synthesis| !JsValue::from(synthesis.clone()).is_undefined())
    })
}

/// One event into the utterance's sink. The sink is taken out of the borrow first: the handler
/// may call the backend back, and the borrow must not be held while it runs.
fn report(live: &Rc<RefCell<Live>>, id: UtteranceId, kind: UtteranceEventKind, terminal: bool) {
    let sink = {
        let mut live = live.borrow_mut();
        if live.terminal {
            return;
        }
        if terminal {
            live.terminal = true;
        }
        Rc::clone(&live.sink)
    };
    sink(UtteranceEvent {
        utterance_id: id,
        kind,
    });
}

fn set_current(id: UtteranceId, live: &Rc<RefCell<Live>>) {
    with_state(|state| state.current = Some((id, Rc::clone(live))));
}

/// Clears the running utterance only if it is still `id`: a late event must not unseat the
/// utterance that replaced the one it belongs to.
fn clear_current(id: UtteranceId) {
    with_state(|state| {
        if state
            .current
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
        {
            state.current = None;
        }
    });
}

fn is_current(id: UtteranceId) -> bool {
    with_state(|state| {
        state
            .current
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
    })
}

/// A pause outlives the utterance it paused: left on, the browser would hold the next utterance
/// too. It is taken off when that utterance ends, is cancelled, is interrupted or dies without
/// the browser.
fn unlatch() {
    if with_state(|state| state.paused) {
        with_state(|state| state.paused = false);
        if let Some(synthesis) = synthesis() {
            synthesis.resume();
        }
    }
}

/// The utterance is done, by the browser or without it: settle its sink once, take any pause
/// off, then run whatever still waits.
fn finished(live: &Rc<RefCell<Live>>, id: UtteranceId, kind: UtteranceEventKind) {
    report(live, id, kind, true);
    clear_current(id);
    unlatch();
    run_queue();
}

/// Speaks now, or queues behind the running utterance. `interrupt` cancels that utterance
/// first, as a cancel event into its own sink.
fn speak(text: &str, interrupt: bool, sink: EventSink) -> Result<Option<UtteranceId>, SpeechError> {
    let Some(synthesis) = synthesis() else {
        return Err(SpeechError::new(ErrorKind::NotAvailable));
    };
    let (id, queued, settings, cancelled) = with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        let cancelled = if interrupt {
            state.current.take()
        } else {
            None
        };
        (
            id,
            state.current.is_some(),
            state.settings.clone(),
            cancelled,
        )
    });
    if queued {
        with_state(|state| {
            state.queue.push_back(Waiting {
                id,
                text: text.to_owned(),
                sink,
            })
        });
        return Ok(Some(id));
    }
    if let Some((old_id, old_live)) = cancelled {
        synthesis.cancel();
        unlatch();
        report(&old_live, old_id, UtteranceEventKind::Cancel, true);
    }
    if !begin(settings, id, text.to_owned(), sink) {
        run_queue();
    }
    Ok(Some(id))
}

/// Schedules the utterance's hand-over to the browser one macrotask out, and marks it current
/// from now: it counts as speaking, and a `stop` or interrupt in the gap settles it. Returns
/// `false`, after settling the utterance, when even the hand-over cannot be scheduled, so the
/// caller runs what still waits.
fn begin(settings: Settings, id: UtteranceId, text: String, sink: EventSink) -> bool {
    let live = Rc::new(RefCell::new(Live {
        sink: Rc::from(sink),
        terminal: false,
    }));
    set_current(id, &live);
    let Some(window) = web_sys::window() else {
        finished(
            &live,
            id,
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::NotAvailable)),
        );
        return false;
    };
    let hand_over = Closure::<dyn Fn()>::new({
        let live = Rc::clone(&live);
        move || {
            // Stopped or interrupted while the macrotask was pending: already settled.
            if !is_current(id) {
                return;
            }
            let Some(synthesis) = synthesis() else {
                finished(
                    &live,
                    id,
                    UtteranceEventKind::Error(SpeechError::new(ErrorKind::NotAvailable)),
                );
                return;
            };
            if !start(&synthesis, &settings, id, &text, &live) {
                finished(
                    &live,
                    id,
                    UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
                );
            }
        }
    });
    if window
        .set_timeout_with_callback(hand_over.into_js_value().unchecked_ref())
        .is_err()
    {
        finished(
            &live,
            id,
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
        );
        return false;
    }
    true
}

/// Builds one utterance with `settings` and hands it to the browser, callbacks wired. `false`
/// when the browser refuses to build it; the caller settles the utterance then.
fn start(
    synthesis: &SpeechSynthesis,
    settings: &Settings,
    id: UtteranceId,
    text: &str,
    live: &Rc<RefCell<Live>>,
) -> bool {
    let Ok(utterance) = SpeechSynthesisUtterance::new_with_text(text) else {
        return false;
    };
    if let Some(rate) = settings.rate {
        utterance.set_rate(map_rate(rate));
    }
    if let Some(pitch) = settings.pitch {
        utterance.set_pitch(map_pitch(pitch));
    }
    if let Some(volume) = settings.volume {
        utterance.set_volume(volume);
    }
    if let Some(language) = &settings.language {
        utterance.set_lang(language);
    }
    if let Some(voice_id) = &settings.voice {
        // The voice is looked up per utterance: the list fills asynchronously, so it cannot be
        // held from set_voice to speak. An id that no longer resolves keeps the default voice.
        for voice in synthesis.get_voices().to_vec() {
            let Ok(voice) = voice.dyn_into::<SpeechSynthesisVoice>() else {
                continue;
            };
            if voice.voice_uri() == *voice_id {
                utterance.set_voice(Some(&voice));
                break;
            }
        }
    }
    wire(&utterance, id, live);
    synthesis.speak(&utterance);
    schedule_heartbeat(id);
    true
}

/// Gives the utterance its four callbacks. Each closure is released to the utterance with
/// `into_js_value`: the utterance keeps it alive while it may still fire, the JS GC frees it
/// after, and no event can land in a closure that was dropped.
fn wire(utterance: &SpeechSynthesisUtterance, id: UtteranceId, live: &Rc<RefCell<Live>>) {
    let onstart = Closure::<dyn Fn(JsValue)>::new({
        let live = Rc::clone(live);
        move |_event: JsValue| report(&live, id, UtteranceEventKind::Start, false)
    });
    utterance.set_onstart(Some(onstart.into_js_value().unchecked_ref()));
    let onboundary = Closure::<dyn Fn(JsValue)>::new({
        let live = Rc::clone(live);
        move |event: JsValue| {
            let Ok(event) = event.dyn_into::<SpeechSynthesisEvent>() else {
                return;
            };
            report(
                &live,
                id,
                UtteranceEventKind::Progress {
                    location: u64::from(event.char_index()),
                    length: u64::from(event.char_length().unwrap_or(0)),
                },
                false,
            );
        }
    });
    utterance.set_onboundary(Some(onboundary.into_js_value().unchecked_ref()));
    let onend = Closure::<dyn Fn(JsValue)>::new({
        let live = Rc::clone(live);
        move |_event: JsValue| finished(&live, id, UtteranceEventKind::Finish)
    });
    utterance.set_onend(Some(onend.into_js_value().unchecked_ref()));
    let onerror = Closure::<dyn Fn(JsValue)>::new({
        let live = Rc::clone(live);
        move |event: JsValue| {
            let kind = event.dyn_into::<ErrorEvent>().ok().map_or_else(
                || UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
                |event| terminal_for_error(&event.error()),
            );
            finished(&live, id, kind);
        }
    });
    utterance.set_onerror(Some(onerror.into_js_value().unchecked_ref()));
}

/// The browser is resumed every few seconds while an utterance is live: Chrome stalls long
/// speech on remote voices — events stop about fifteen seconds in, until resume() is called.
const HEARTBEAT_MS: i32 = 5000;

/// One heartbeat tick. A tick whose utterance is no longer current schedules nothing further,
/// so the heartbeat dies with its utterance; a deliberately paused one is left alone, since
/// resume() would unpause it.
fn schedule_heartbeat(id: UtteranceId) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let tick = Closure::<dyn Fn()>::new(move || {
        if !is_current(id) {
            return;
        }
        if !with_state(|state| state.paused)
            && let Some(synthesis) = synthesis()
        {
            synthesis.resume();
        }
        schedule_heartbeat(id);
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        tick.into_js_value().unchecked_ref(),
        HEARTBEAT_MS,
    );
}

/// Hands the browser the next waiting utterance, settling each it cannot take.
fn run_queue() {
    loop {
        let next = with_state(|state| {
            if state.current.is_none() {
                state
                    .queue
                    .pop_front()
                    .map(|waiting| (waiting, state.settings.clone()))
            } else {
                None
            }
        });
        let Some((Waiting { id, text, sink }, settings)) = next else {
            return;
        };
        if begin(settings, id, text, sink) {
            return;
        }
    }
}

/// The app's 0.0..=1.0 rate onto the utterance's 0.1..=10.0, with the API's 0.5 "normal" on
/// the browser's default 1.0: the same rate must sound equally normal on every platform.
fn map_rate(rate: f32) -> f32 {
    if rate <= 0.5 {
        0.1 + rate * 1.8
    } else {
        1.0 + (rate - 0.5) * 18.0
    }
}

/// The app's 0.5..=2.0 pitch onto the utterance's 0.0..=2.0.
fn map_pitch(pitch: f32) -> f32 {
    (pitch - 0.5) * (2.0 / 1.5)
}

/// The web's error taxonomy, as the browser's wire strings: `canceled` and `interrupted` are
/// cancels, a rejected language or a missing voice carries its own code, another synthesis
/// fault is `SynthesisFailed`, and the service failing some other way is `Backend`.
fn terminal_for_error(error: &str) -> UtteranceEventKind {
    match error {
        "canceled" | "interrupted" => UtteranceEventKind::Cancel,
        "language-unavailable" => {
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::LanguageNotSupported))
        }
        "voice-unavailable" => {
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::VoiceNotFound))
        }
        "synthesis-failed" | "text-too-long" | "invalid-argument" => {
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::SynthesisFailed))
        }
        _ => UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
    }
}

/// A browser voice as the crate's. Every listed voice can be set; a non-local one needs the
/// network.
fn voice_of(voice: SpeechSynthesisVoice) -> Voice {
    Voice {
        id: voice.voice_uri(),
        name: voice.name(),
        language: voice.lang(),
        network_required: !voice.local_service(),
        installed: true,
    }
}

impl Backend for NativeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        // A missing speechSynthesis (workers, old browsers) is the visible-fallback signal.
        Box::pin(ready(if synthesis().is_some() {
            Ok(())
        } else {
            Err(SpeechError::new(ErrorKind::NotAvailable))
        }))
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(ready(if synthesis().is_some() {
            Ok(true)
        } else {
            Err(SpeechError::new(ErrorKind::NotAvailable))
        }))
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        Box::pin(ready(speak(text, interrupt, on_event)))
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let current = with_state(|state| state.current.take());
        Box::pin(ready(if let Some((id, live)) = current {
            if let Some(synthesis) = synthesis() {
                synthesis.cancel();
            }
            unlatch();
            report(&live, id, UtteranceEventKind::Cancel, true);
            run_queue();
            Ok(())
        } else {
            Ok(())
        }))
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let current = with_state(|state| state.current.is_some());
        Box::pin(ready(match synthesis() {
            // The backend's own current counts as speaking: an utterance handed over but not
            // yet started is in flight, not idle.
            Some(synthesis) if current || synthesis.speaking() => {
                // The browser pauses only what it is speaking, so the pause of an utterance
                // still in the hand-over gap is remembered and applied by resume.
                with_state(|state| state.paused = true);
                synthesis.pause();
                Ok(())
            }
            Some(_) => Err(SpeechError::new(ErrorKind::NotSpeaking)),
            None => Err(SpeechError::new(ErrorKind::NotAvailable)),
        }))
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        let paused = with_state(|state| state.paused);
        Box::pin(ready(match synthesis() {
            Some(synthesis) if paused => {
                with_state(|state| state.paused = false);
                synthesis.resume();
                Ok(())
            }
            Some(_) => Err(SpeechError::new(ErrorKind::NotSpeaking)),
            None => Err(SpeechError::new(ErrorKind::NotAvailable)),
        }))
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        let queued = with_state(|state| state.current.is_some() || !state.queue.is_empty());
        let speaking = queued || synthesis().is_some_and(|synthesis| synthesis.speaking());
        Box::pin(ready(Ok(speaking)))
    }

    fn synthesize(&self, _text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        // The Web Speech API speaks aloud only; it cannot hand out audio bytes.
        Box::pin(ready(Err(SpeechError::new(ErrorKind::NotAvailable))))
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        Box::pin(ready(match synthesis() {
            Some(synthesis) => {
                // The list fills asynchronously: an early empty read is an honest empty.
                Ok(synthesis
                    .get_voices()
                    .to_vec()
                    .into_iter()
                    .filter_map(|value| value.dyn_into::<SpeechSynthesisVoice>().ok())
                    .map(voice_of)
                    .collect())
            }
            None => Err(SpeechError::new(ErrorKind::NotAvailable)),
        }))
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        Box::pin(ready(if synthesis().is_some() {
            // The web has one engine, the browser's own.
            Ok(vec![Engine {
                name: "web-speech".to_owned(),
                label: "Web Speech API".to_owned(),
                default: true,
            }])
        } else {
            Err(SpeechError::new(ErrorKind::NotAvailable))
        }))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        Box::pin(ready(match synthesis() {
            Some(synthesis) => {
                let mut languages: Vec<String> = synthesis
                    .get_voices()
                    .to_vec()
                    .into_iter()
                    .filter_map(|value| value.dyn_into::<SpeechSynthesisVoice>().ok())
                    .map(|voice| voice.lang())
                    .collect();
                languages.sort();
                languages.dedup();
                Ok(languages)
            }
            None => Err(SpeechError::new(ErrorKind::NotAvailable)),
        }))
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        // The Web Speech API reports no length limit.
        Box::pin(ready(Ok(None)))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        with_state(|state| state.settings.language = Some(tag.to_owned()));
        Box::pin(ready(Ok(())))
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        with_state(|state| state.settings.voice = Some(id.to_owned()));
        Box::pin(ready(Ok(())))
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        with_state(|state| state.settings.rate = Some(rate));
        Box::pin(ready(Ok(())))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        with_state(|state| state.settings.pitch = Some(pitch));
        Box::pin(ready(Ok(())))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        with_state(|state| state.settings.volume = Some(volume));
        Box::pin(ready(Ok(())))
    }

    fn set_silence(&self, _ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        // The Web Speech API has no leading silence to set; the value is accepted and dropped.
        Box::pin(ready(Ok(())))
    }
}

// These compile for wasm32-unknown-unknown and would run in a browser; this workflow does not
// drive browsers, so CI compile-checks them only (see the module docs).
#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1e-4
    }

    #[test]
    fn map_rate_covers_the_utterance_range() {
        assert!(close(map_rate(0.0), 0.1));
        assert!(close(map_rate(1.0), 10.0));
    }

    #[test]
    fn map_rate_puts_the_api_normal_on_the_browser_default() {
        assert!(close(map_rate(0.5), 1.0));
    }

    #[test]
    fn map_rate_is_linear_on_each_half() {
        assert!(close(map_rate(0.25), 0.55));
        assert!(close(map_rate(0.75), 5.5));
    }

    #[test]
    fn map_pitch_covers_the_utterance_range() {
        assert!(close(map_pitch(0.5), 0.0));
        assert!(close(map_pitch(2.0), 2.0));
    }

    #[test]
    fn canceled_and_interrupted_are_cancels() {
        assert_eq!(terminal_for_error("canceled"), UtteranceEventKind::Cancel);
        assert_eq!(
            terminal_for_error("interrupted"),
            UtteranceEventKind::Cancel
        );
    }

    #[test]
    fn language_and_voice_faults_carry_their_own_codes() {
        assert_eq!(
            terminal_for_error("language-unavailable"),
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::LanguageNotSupported))
        );
        assert_eq!(
            terminal_for_error("voice-unavailable"),
            UtteranceEventKind::Error(SpeechError::new(ErrorKind::VoiceNotFound))
        );
    }

    #[test]
    fn synthesis_faults_are_synthesis_failed() {
        for error in ["synthesis-failed", "text-too-long", "invalid-argument"] {
            assert_eq!(
                terminal_for_error(error),
                UtteranceEventKind::Error(SpeechError::new(ErrorKind::SynthesisFailed))
            );
        }
    }

    #[test]
    fn service_faults_are_backend() {
        for error in [
            "audio-busy",
            "audio-hardware",
            "network",
            "synthesis-unavailable",
            "a-code-invented-later",
        ] {
            assert_eq!(
                terminal_for_error(error),
                UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend))
            );
        }
    }
}
