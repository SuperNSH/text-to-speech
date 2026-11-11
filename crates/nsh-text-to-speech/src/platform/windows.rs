//! Windows: `SpeechSynthesizer` renders each utterance to a stream and `MediaPlayer` plays it,
//! as tts-rs does. WinRT reports no progress inside an utterance, so one utterance's events are
//! `start`, then exactly one of `finish`, `cancel` or `error` — a queue failure before the
//! `start` fires its `error` alone, as a refused render does. The queue runs on its own threads,
//! so `speak` resolves at once with its id; awaiting the utterance's end is
//! `await_speak_completion`'s job. A starter spawn the OS refuses settles the refused call's own
//! utterance with `error` (the same `Backend` failure the call reports) and dequeues it, so
//! nothing speaks once its call reported failure.

use std::{
    collections::VecDeque,
    future::ready,
    sync::{LazyLock, Mutex, MutexGuard, PoisonError},
};

use windows::{
    Foundation::TypedEventHandler,
    Media::{
        Core::MediaSource,
        Playback::{MediaPlayer, MediaPlayerAudioCategory, MediaPlayerFailedEventArgs},
        SpeechSynthesis::{SpeechSynthesisStream, SpeechSynthesizer, VoiceInformation},
    },
    Storage::Streams::DataReader,
    Win32::Foundation::REGDB_E_CLASSNOTREG,
    core::{Error as WinError, HSTRING, Ref},
};

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, UtteranceId, Voice},
};

pub(crate) struct NativeBackend;

/// One utterance the backend holds: the one playing, or one waiting behind it.
struct Held {
    id: UtteranceId,
    text: String,
    sink: EventSink,
}

#[derive(Clone)]
struct Settings {
    rate: f32,
    pitch: f32,
    volume: f32,
    voice: Option<VoiceInformation>,
}

impl Default for Settings {
    /// flutter_tts's defaults; its rate 0.5 is normal speed on Windows.
    fn default() -> Self {
        Self {
            rate: 0.5,
            pitch: 1.0,
            volume: 1.0,
            voice: None,
        }
    }
}

/// The queue and the settings, behind one lock; the WinRT objects live in [`OBJECTS`].
///
/// At most one of `pending_start`, `starting` and `playing` is set: an utterance is accepted,
/// then held by its starter while it renders (stop and interrupt cannot reach its sink then, so
/// they set `revoke_start` and the starter fires the cancel itself), then attached and live —
/// the starter's claim ends under the same lock that sets `playing`, so no state ever holds
/// both. `waiting` holds the rest, first in, first out.
struct Inner {
    pending_start: Option<Held>,
    waiting: VecDeque<Held>,
    starting: bool,
    revoke_start: bool,
    playing: Option<Held>,
    paused: bool,
    ended_token: Option<i64>,
    failed_token: Option<i64>,
    next_id: UtteranceId,
    settings: Settings,
}

static INNER: LazyLock<Mutex<Inner>> = LazyLock::new(|| {
    Mutex::new(Inner {
        pending_start: None,
        waiting: VecDeque::new(),
        starting: false,
        revoke_start: false,
        playing: None,
        paused: false,
        ended_token: None,
        failed_token: None,
        next_id: 1,
        settings: Settings::default(),
    })
});

#[derive(Clone)]
struct Objects {
    synth: SpeechSynthesizer,
    player: MediaPlayer,
}

/// One activation for the process. A device that cannot activate the synthesizer never can, so
/// the first failure is kept and every call reports it again.
static OBJECTS: LazyLock<Result<Objects, SpeechError>> = LazyLock::new(create_objects);

fn create_objects() -> Result<Objects, SpeechError> {
    let synth = SpeechSynthesizer::new().map_err(engine_error)?;
    let player = MediaPlayer::new().map_err(win_error)?;
    player.SetRealTimePlayback(true).map_err(win_error)?;
    player
        .SetAudioCategory(MediaPlayerAudioCategory::Speech)
        .map_err(win_error)?;
    // The player's events are registered per attached utterance, in `register_events`.
    Ok(Objects { synth, player })
}

fn lock() -> MutexGuard<'static, Inner> {
    INNER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn objects() -> Result<Objects, SpeechError> {
    (*OBJECTS).clone()
}

/// flutter_tts's Windows mapping: rate 0 to 1 becomes SpeakingRate 0.5 to 1.5, so 0.5 speaks at
/// normal speed.
fn winrt_rate(rate: f32) -> f64 {
    f64::from(rate) + 0.5
}

fn win_error(error: WinError) -> SpeechError {
    SpeechError::new(ErrorKind::Backend).with_status(i64::from(error.code().0))
}

/// The synthesizer's class is missing on Windows installs without the speech feature.
fn engine_error(error: WinError) -> SpeechError {
    if error.code() == REGDB_E_CLASSNOTREG {
        SpeechError::new(ErrorKind::EngineMissing).with_status(i64::from(error.code().0))
    } else {
        win_error(error)
    }
}

/// A render WinRT refused is a synthesis fault; a failure around it is a backend fault.
fn synthesis_error(error: WinError) -> SpeechError {
    SpeechError::new(ErrorKind::SynthesisFailed).with_status(i64::from(error.code().0))
}

/// BCP-47 tags differ in case alone (`en-US` and `en-us`).
fn language_matches(voice_language: &str, tag: &str) -> bool {
    voice_language.eq_ignore_ascii_case(tag)
}

/// Fires one event from outside the queue lock: a handler may call the backend back.
fn fire(sink: &EventSink, id: UtteranceId, kind: UtteranceEventKind) {
    sink(UtteranceEvent {
        utterance_id: id,
        kind,
    });
}

fn installed_voices() -> Result<Vec<VoiceInformation>, SpeechError> {
    Ok(SpeechSynthesizer::AllVoices()
        .map_err(win_error)?
        .into_iter()
        .collect())
}

fn to_voice(voice: VoiceInformation) -> Result<Voice, SpeechError> {
    // AllVoices lists installed voices only, and Windows exposes no per-voice network flag.
    Ok(Voice {
        id: voice.Id().map_err(win_error)?.to_string(),
        name: voice.DisplayName().map_err(win_error)?.to_string(),
        language: voice.Language().map_err(win_error)?.to_string(),
        network_required: false,
        installed: true,
    })
}

/// The installed voice for a language tag, if one exists.
fn voice_for_language(tag: &str) -> Result<Option<VoiceInformation>, SpeechError> {
    for voice in installed_voices()? {
        let language = voice.Language().map_err(win_error)?.to_string();
        if language_matches(&language, tag) {
            return Ok(Some(voice));
        }
    }
    Ok(None)
}

/// The installed voice with `id`, if one exists.
fn voice_for_id(id: &str) -> Result<Option<VoiceInformation>, SpeechError> {
    for voice in installed_voices()? {
        if voice.Id().map_err(win_error)? == id {
            return Ok(Some(voice));
        }
    }
    Ok(None)
}

/// Applies the settings to the one synthesizer that serves every utterance. Two renders that
/// interleave across a cancel can trade settings; the abandoned render is the only casualty.
fn apply_settings(objects: &Objects, settings: &Settings) -> Result<(), SpeechError> {
    let options = objects.synth.Options().map_err(win_error)?;
    options
        .SetSpeakingRate(winrt_rate(settings.rate))
        .map_err(win_error)?;
    options
        .SetAudioPitch(f64::from(settings.pitch))
        .map_err(win_error)?;
    options
        .SetAudioVolume(f64::from(settings.volume))
        .map_err(win_error)?;
    if let Some(voice) = &settings.voice {
        objects.synth.SetVoice(voice).map_err(win_error)?;
    }
    Ok(())
}

async fn render(
    objects: &Objects,
    settings: &Settings,
    text: &str,
) -> Result<SpeechSynthesisStream, SpeechError> {
    apply_settings(objects, settings)?;
    let spoken = HSTRING::from(text);
    objects
        .synth
        .SynthesizeTextToStreamAsync(&spoken)
        .map_err(synthesis_error)?
        .await
        .map_err(synthesis_error)
}

/// The blocking render, for the player's own event threads: no executor runs there, so `join` is
/// the only wait. tts-rs starts the next utterance the same way.
fn render_blocking(
    objects: &Objects,
    settings: &Settings,
    text: &str,
) -> Result<SpeechSynthesisStream, SpeechError> {
    apply_settings(objects, settings)?;
    let spoken = HSTRING::from(text);
    objects
        .synth
        .SynthesizeTextToStreamAsync(&spoken)
        .map_err(synthesis_error)?
        .join()
        .map_err(synthesis_error)
}

/// Wraps a rendered stream for the player.
fn to_source(stream: &SpeechSynthesisStream) -> Result<MediaSource, SpeechError> {
    let content_type = stream.ContentType().map_err(win_error)?;
    MediaSource::CreateFromStream(stream, &content_type).map_err(win_error)
}

/// Reads a rendered stream whole: the WAV bytes `synthesize` resolves with.
async fn read_all(stream: &SpeechSynthesisStream) -> Result<Vec<u8>, SpeechError> {
    let size = stream.Size().map_err(win_error)?;
    let size = u32::try_from(size).map_err(|_| SpeechError::new(ErrorKind::Backend))?;
    let input = stream.GetInputStreamAt(0).map_err(win_error)?;
    let reader = DataReader::CreateDataReader(&input).map_err(win_error)?;
    reader
        .LoadAsync(size)
        .map_err(win_error)?
        .await
        .map_err(win_error)?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).map_err(win_error)?;
    Ok(bytes)
}

/// Runs the queue on its own thread: `speak` and `stop` resolve at once, and a caller that goes
/// away never strands the queue.
///
/// A thread is spawned only when something is to start, nothing is attached, and no starter
/// holds a claim — a starter between utterances re-claims on its own, and attached audio
/// advances the queue through its own handlers — so a burst of calls does not spawn a thread
/// each. The spawn runs under the lock, so a refusal settles the refused call's own utterance
/// (`id`) in the same critical section: no racing starter can claim work whose call is about to
/// report failure. A spawn the OS refuses settles as a coded error, never a panic.
fn nudge(id: Option<UtteranceId>) -> Result<(), SpeechError> {
    let mut inner = lock();
    let needed = !inner.starting
        && inner.playing.is_none()
        && (inner.pending_start.is_some() || !inner.waiting.is_empty());
    if !needed {
        return Ok(());
    }
    let (settled, error) = match std::thread::Builder::new().spawn(start_queue_blocking) {
        Ok(_detached) => return Ok(()),
        Err(_) => (
            id.and_then(|id| take_held_by_id(&mut inner, id)),
            SpeechError::new(ErrorKind::Backend),
        ),
    };
    drop(inner);
    if let Some(held) = settled {
        fire(
            &held.sink,
            held.id,
            UtteranceEventKind::Error(error.clone()),
        );
    }
    Err(error)
}

/// Takes the utterance a starter runs next, promoting the waiting front only when it takes it:
/// what is promoted always has a starter, and its claim is minted with the flag. `None` when
/// audio is attached (its end advances the queue), a starter already runs, or nothing waits.
fn claim_front() -> Option<(Held, Settings, StartClaim)> {
    let mut inner = lock();
    if inner.starting || inner.playing.is_some() {
        return None;
    }
    let held = inner
        .pending_start
        .take()
        .or_else(|| inner.waiting.pop_front())?;
    inner.starting = true;
    Some((held, inner.settings.clone(), StartClaim::granted()))
}

/// Where a stop's cancel goes. Attached audio wins even over a `starting` flag: revoking a
/// starter whose utterance already plays would drop the cancel silently (the claim ends with
/// the attach, but the preference is pinned here).
#[derive(Debug, PartialEq, Eq)]
enum StopRoute {
    /// The starter holds the utterance; only it can fire the cancel, at its attach.
    RevokeStarter,
    /// The queue's own utterance — pending or attached — is taken right here.
    TakeHeld,
}

fn stop_route(starting: bool, playing: bool) -> StopRoute {
    if starting && !playing {
        StopRoute::RevokeStarter
    } else {
        StopRoute::TakeHeld
    }
}

/// Takes the running utterance out for a cancel. While its starter renders it, the starter is
/// told instead (`revoke_start`): it holds the utterance, so only it can fire the event.
fn take_cancelled(inner: &mut Inner) -> Option<Held> {
    match stop_route(inner.starting, inner.playing.is_some()) {
        StopRoute::RevokeStarter => {
            inner.revoke_start = true;
            None
        }
        StopRoute::TakeHeld => {
            inner.paused = false;
            // Pause only cuts the audio; the next utterance's SetSource replaces it.
            if let Ok(objects) = &*OBJECTS {
                let _ = objects.player.Pause();
            }
            inner.pending_start.take().or_else(|| inner.playing.take())
        }
    }
}

/// Takes one utterance back out of the queue by id: the settlement of a refused starter spawn,
/// so nothing of a call that reported failure is left to speak. `None` when no slot holds the
/// id — a concurrent stop already cancelled it, and its cancel fired there.
fn take_held_by_id(inner: &mut Inner, id: UtteranceId) -> Option<Held> {
    if inner
        .pending_start
        .as_ref()
        .is_some_and(|held| held.id == id)
    {
        return inner.pending_start.take();
    }
    let index = inner.waiting.iter().position(|held| held.id == id)?;
    inner.waiting.remove(index)
}

/// Whether an admitted utterance goes straight to `pending_start`: only when nothing plays,
/// nothing renders and nothing waits to be started. Everything else queues behind, in order.
fn admits_to_pending_start(inner: &Inner) -> bool {
    inner.playing.is_none() && !inner.starting && inner.pending_start.is_none()
}

/// Registers the events of one attached utterance, removing the previous attach's handlers
/// first. `MediaEnded` and `MediaFailed` carry no utterance, so each attach's handlers capture
/// theirs and ignore a firing whose utterance is not the one playing: a stale firing delivered
/// to a removed handler cannot land, and one delivered to these handlers is ignored unless this
/// utterance plays. What the id cannot tell apart is a firing for replaced audio that WinRT
/// delivers to these handlers after re-registration; WinRT documents no such firing for a
/// paused player whose source is replaced, so that residual is accepted, not excluded.
fn register_events(
    inner: &mut Inner,
    player: &MediaPlayer,
    id: UtteranceId,
) -> Result<(), WinError> {
    if let Some(token) = inner.ended_token.take() {
        player.RemoveMediaEnded(token)?;
    }
    if let Some(token) = inner.failed_token.take() {
        player.RemoveMediaFailed(token)?;
    }
    let ended = player.MediaEnded(&TypedEventHandler::new(move |_, _| handle_ended(id)))?;
    match player.MediaFailed(&TypedEventHandler::new(move |_, args| {
        handle_failed(id, args)
    })) {
        Ok(failed) => {
            inner.ended_token = Some(ended);
            inner.failed_token = Some(failed);
            Ok(())
        }
        Err(error) => {
            let _ = player.RemoveMediaEnded(ended);
            Err(error)
        }
    }
}

/// The outcome of attaching a rendered utterance: it plays, it was cancelled while it rendered,
/// or the player refused it.
enum Attached {
    Playing,
    Cancelled(Held),
    Failed(Held, SpeechError),
}

fn attach(
    inner: &mut Inner,
    player: &MediaPlayer,
    source: &MediaSource,
    held: Held,
    claim: &mut StartClaim,
) -> Attached {
    if inner.revoke_start {
        inner.revoke_start = false;
        return Attached::Cancelled(held);
    }
    let id = held.id;
    // A pause that began while the utterance rendered still holds; resume plays it.
    let attached = register_events(inner, player, id)
        .and_then(|()| player.SetSource(source))
        .and_then(|()| if inner.paused { Ok(()) } else { player.Play() });
    match attached {
        Ok(()) => {
            inner.playing = Some(held);
            // The claim ends where `playing` begins, under this same lock: no state holds both
            // flags, so a stop here cannot misroute to the starter, and the queue's own
            // handlers never see a stale claim.
            claim.end(inner);
            Attached::Playing
        }
        Err(error) => Attached::Failed(held, win_error(error)),
    }
}

/// Holds a starter's claim for as long as it lives. `starting` is set in exactly one place —
/// `claim_front`, which mints the claim — and cleared in exactly one, [`StartClaim::end`]:
/// called by `attach` under its own lock when the utterance goes live, and by `Drop` for every
/// other exit — return, failed render, refused attach, even a panic — so no exit path wedges
/// the flag and stalls the queue. The stale `revoke_start` of a claim that never reached its
/// attach goes with it.
struct StartClaim {
    active: bool,
}

impl StartClaim {
    /// The claim `claim_front` grants alongside its flag.
    fn granted() -> Self {
        Self { active: true }
    }

    /// Ends the claim on state the caller already holds the lock for, so the end is atomic with
    /// the caller's own change; the later `Drop` is then a no-op.
    fn end(&mut self, inner: &mut Inner) {
        inner.starting = false;
        inner.revoke_start = false;
        self.active = false;
    }
}

impl Drop for StartClaim {
    fn drop(&mut self) {
        if self.active {
            self.end(&mut lock());
        }
    }
}

/// Starts the next utterance: renders it, fires its `start`, and attaches its audio. A render or
/// attach failure fails that utterance and the next is tried, so the queue always drains.
fn start_queue_blocking() {
    while let Some((held, settings, mut claim)) = claim_front() {
        let objects = match objects() {
            Ok(objects) => objects,
            Err(error) => {
                fire(&held.sink, held.id, UtteranceEventKind::Error(error));
                continue;
            }
        };
        let stream = match render_blocking(&objects, &settings, &held.text) {
            Ok(stream) => stream,
            Err(error) => {
                fire(&held.sink, held.id, UtteranceEventKind::Error(error));
                continue;
            }
        };
        let source = match to_source(&stream) {
            Ok(source) => source,
            Err(error) => {
                fire(&held.sink, held.id, UtteranceEventKind::Error(error));
                continue;
            }
        };
        fire(&held.sink, held.id, UtteranceEventKind::Start);
        let outcome = {
            let mut inner = lock();
            attach(&mut inner, &objects.player, &source, held, &mut claim)
        };
        match outcome {
            Attached::Playing => return,
            Attached::Cancelled(held) => fire(&held.sink, held.id, UtteranceEventKind::Cancel),
            Attached::Failed(held, error) => {
                fire(&held.sink, held.id, UtteranceEventKind::Error(error));
            }
        }
    }
}

fn handle_ended(id: UtteranceId) -> Result<(), WinError> {
    let finished = {
        let mut inner = lock();
        // A stale firing: the utterance this handler was registered for is not playing.
        if inner.playing.as_ref().map(|held| held.id) != Some(id) {
            return Ok(());
        }
        inner.playing.take()
    };
    if let Some(held) = finished {
        fire(&held.sink, held.id, UtteranceEventKind::Finish);
    }
    start_queue_blocking();
    Ok(())
}

/// A playback failure is the platform service failing another way, not a synthesis fault.
fn playback_error(status: Option<i64>) -> SpeechError {
    let error = SpeechError::new(ErrorKind::Backend);
    match status {
        Some(status) => error.with_status(status),
        None => error,
    }
}

fn handle_failed(
    id: UtteranceId,
    args: Ref<'_, MediaPlayerFailedEventArgs>,
) -> Result<(), WinError> {
    let status = args
        .as_ref()
        .and_then(|args| args.ExtendedErrorCode().ok())
        .map(|code| i64::from(code.0));
    let failed = {
        let mut inner = lock();
        if inner.playing.as_ref().map(|held| held.id) != Some(id) {
            return Ok(());
        }
        inner.playing.take()
    };
    if let Some(held) = failed {
        fire(
            &held.sink,
            held.id,
            UtteranceEventKind::Error(playback_error(status)),
        );
    }
    start_queue_blocking();
    Ok(())
}

impl Backend for NativeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(async {
            objects()?;
            // A Windows device with no installed voice cannot speak at all.
            if installed_voices()?.is_empty() {
                return Err(SpeechError::new(ErrorKind::EngineMissing));
            }
            Ok(())
        })
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(ready(Ok(objects().is_ok())))
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        let text = text.to_owned();
        Box::pin(async move {
            objects()?;
            let (id, cancelled) = {
                let mut inner = lock();
                let cancelled = if interrupt {
                    take_cancelled(&mut inner)
                } else {
                    None
                };
                let id = inner.next_id;
                inner.next_id += 1;
                let held = Held {
                    id,
                    text,
                    sink: on_event,
                };
                if admits_to_pending_start(&inner) {
                    inner.pending_start = Some(held);
                } else {
                    inner.waiting.push_back(held);
                }
                (id, cancelled)
            };
            if let Some(held) = cancelled {
                fire(&held.sink, held.id, UtteranceEventKind::Cancel);
            }
            // A refused spawn settles this utterance inside `nudge` itself, under the lock that
            // saw the refusal, so nothing of this call is left queued to speak after the
            // failure it reports.
            nudge(Some(id))?;
            Ok(Some(id))
        })
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(async move {
            let cancelled = {
                let mut inner = lock();
                take_cancelled(&mut inner)
            };
            if let Some(held) = cancelled {
                fire(&held.sink, held.id, UtteranceEventKind::Cancel);
            }
            // The running utterance's cancel has fired by here; queued utterances stay queued,
            // as the trait holds — their calls promised a start, not an immediate one. A refused
            // starter spawn leaves them for the next nudge: failing this call would misreport a
            // cancel that already happened.
            let _ = nudge(None);
            Ok(())
        })
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(async move {
            let mut inner = lock();
            if inner.playing.is_none() && inner.pending_start.is_none() && !inner.starting {
                return Err(SpeechError::new(ErrorKind::NotSpeaking));
            }
            inner.paused = true;
            if let Ok(objects) = &*OBJECTS {
                let _ = objects.player.Pause();
            }
            Ok(())
        })
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(async move {
            let mut inner = lock();
            if !inner.paused {
                return Err(SpeechError::new(ErrorKind::NotSpeaking));
            }
            inner.paused = false;
            if inner.playing.is_some()
                && let Ok(objects) = &*OBJECTS
            {
                let _ = objects.player.Play();
            }
            Ok(())
        })
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(async move {
            let inner = lock();
            Ok(inner.pending_start.is_some()
                || inner.starting
                || inner.playing.is_some()
                || !inner.waiting.is_empty())
        })
    }

    fn synthesize(&self, text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        let text = text.to_owned();
        Box::pin(async move {
            let objects = objects()?;
            let settings = lock().settings.clone();
            let stream = render(&objects, &settings, &text).await?;
            read_all(&stream).await
        })
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        Box::pin(async move {
            installed_voices()?
                .into_iter()
                .map(to_voice)
                .collect::<Result<Vec<_>, _>>()
        })
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        Box::pin(ready(Ok(vec![Engine {
            name: "Windows.Media.SpeechSynthesis".to_owned(),
            label: "Windows Speech Synthesis".to_owned(),
            default: true,
        }])))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        Box::pin(async move {
            // One entry per language, in the order the voices report them.
            let mut languages: Vec<String> = Vec::new();
            for voice in installed_voices()? {
                let tag = voice.Language().map_err(win_error)?.to_string();
                if !languages.contains(&tag) {
                    languages.push(tag);
                }
            }
            Ok(languages)
        })
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        // WinRT documents no input limit.
        Box::pin(ready(Ok(None)))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        // WinRT has no language setting; a language is set by picking a voice that speaks it.
        let answer = match voice_for_language(tag) {
            Ok(Some(voice)) => {
                lock().settings.voice = Some(voice);
                Ok(())
            }
            Ok(None) => Err(SpeechError::new(ErrorKind::LanguageNotSupported)),
            Err(error) => Err(error),
        };
        Box::pin(ready(answer))
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let answer = match voice_for_id(id) {
            Ok(Some(voice)) => {
                lock().settings.voice = Some(voice);
                Ok(())
            }
            Ok(None) => Err(SpeechError::new(ErrorKind::VoiceNotFound)),
            Err(error) => Err(error),
        };
        Box::pin(ready(answer))
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        lock().settings.rate = rate;
        Box::pin(ready(Ok(())))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        lock().settings.pitch = pitch;
        Box::pin(ready(Ok(())))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        lock().settings.volume = volume;
        Box::pin(ready(Ok(())))
    }

    fn set_silence(&self, _ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        // Windows offers no leading-silence control; AppendedSilence is trailing and fixed.
        Box::pin(ready(Ok(())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::HRESULT;

    #[test]
    fn rate_maps_to_winrt_speaking_rate() {
        assert_eq!(winrt_rate(0.0), 0.5);
        assert_eq!(winrt_rate(0.5), 1.0);
        assert_eq!(winrt_rate(1.0), 1.5);
    }

    #[test]
    fn backend_errors_carry_the_hresult_status() {
        let error = win_error(WinError::from_hresult(HRESULT(0x8000_4005_u32 as i32)));
        assert_eq!(error.kind(), ErrorKind::Backend);
        assert_eq!(error.status(), Some(i64::from(0x8000_4005_u32 as i32)));
    }

    #[test]
    fn class_not_registered_reports_engine_missing() {
        let error = engine_error(WinError::from_hresult(REGDB_E_CLASSNOTREG));
        assert_eq!(error.kind(), ErrorKind::EngineMissing);
    }

    #[test]
    fn refused_renders_report_synthesis_failed() {
        let error = synthesis_error(WinError::from_hresult(HRESULT(0x8007_000E_u32 as i32)));
        assert_eq!(error.kind(), ErrorKind::SynthesisFailed);
        assert_eq!(error.status(), Some(i64::from(0x8007_000E_u32 as i32)));
    }

    #[test]
    fn language_tags_match_regardless_of_case() {
        assert!(language_matches("en-US", "en-us"));
        assert!(!language_matches("en-US", "en-GB"));
    }

    #[test]
    fn playback_failures_report_backend() {
        let error = playback_error(Some(i64::from(0x8007_000E_u32 as i32)));
        assert_eq!(error.kind(), ErrorKind::Backend);
        assert_eq!(error.status(), Some(i64::from(0x8007_000E_u32 as i32)));
        assert_eq!(playback_error(None).status(), None);
    }

    fn held_of(id: UtteranceId) -> Held {
        Held {
            id,
            text: format!("text-{id}"),
            sink: Box::new(|_| ()),
        }
    }

    fn inner_of(waiting: &[UtteranceId]) -> Inner {
        Inner {
            pending_start: None,
            waiting: waiting.iter().copied().map(held_of).collect(),
            starting: false,
            revoke_start: false,
            playing: None,
            paused: false,
            ended_token: None,
            failed_token: None,
            next_id: waiting.iter().copied().max().unwrap_or_default() + 1,
            settings: Settings::default(),
        }
    }

    fn waiting_ids(inner: &Inner) -> Vec<UtteranceId> {
        inner.waiting.iter().map(|held| held.id).collect()
    }

    #[test]
    fn a_stop_takes_attached_audio_even_while_a_claim_reads_set() {
        assert_eq!(stop_route(true, true), StopRoute::TakeHeld);
    }

    #[test]
    fn a_stop_revokes_only_a_starter_whose_utterance_is_not_attached() {
        assert_eq!(stop_route(true, false), StopRoute::RevokeStarter);
        assert_eq!(stop_route(false, true), StopRoute::TakeHeld);
        assert_eq!(stop_route(false, false), StopRoute::TakeHeld);
    }

    #[test]
    fn ending_a_claim_clears_the_start_flags_exactly_once() {
        let mut inner = inner_of(&[]);
        inner.starting = true;
        inner.revoke_start = true;
        let mut claim = StartClaim::granted();
        claim.end(&mut inner);
        assert!(!inner.starting);
        assert!(!inner.revoke_start);
        drop(claim);
        assert!(!inner.starting);
        assert!(!inner.revoke_start);
    }

    #[test]
    fn dropping_an_active_claim_releases_it() {
        let mut inner = lock();
        inner.starting = true;
        inner.revoke_start = true;
        drop(inner);
        drop(StartClaim::granted());
        let inner = lock();
        assert!(!inner.starting);
        assert!(!inner.revoke_start);
    }

    #[test]
    fn only_an_idle_queue_admits_to_pending_start() {
        assert!(admits_to_pending_start(&inner_of(&[])));
        let playing = Inner {
            playing: Some(held_of(1)),
            ..inner_of(&[])
        };
        assert!(!admits_to_pending_start(&playing));
        let starting = Inner {
            starting: true,
            ..inner_of(&[])
        };
        assert!(!admits_to_pending_start(&starting));
        let pending = Inner {
            pending_start: Some(held_of(1)),
            ..inner_of(&[])
        };
        assert!(!admits_to_pending_start(&pending));
    }

    #[test]
    fn a_refused_call_takes_its_utterance_back_from_pending_start() {
        let mut inner = inner_of(&[]);
        inner.pending_start = Some(held_of(7));
        let taken = take_held_by_id(&mut inner, 7);
        assert_eq!(taken.map(|held| held.id), Some(7));
        assert!(inner.pending_start.is_none());
    }

    #[test]
    fn a_refused_call_takes_its_utterance_back_from_mid_queue_in_order() {
        let mut inner = inner_of(&[5, 7, 9]);
        let taken = take_held_by_id(&mut inner, 7);
        assert_eq!(taken.map(|held| held.id), Some(7));
        assert_eq!(waiting_ids(&inner), vec![5, 9]);
    }

    #[test]
    fn a_refused_settlement_leaves_other_utterances_queued() {
        let mut inner = inner_of(&[5, 9]);
        assert!(take_held_by_id(&mut inner, 7).is_none());
        assert_eq!(waiting_ids(&inner), vec![5, 9]);
    }
}
