//! Apple (iOS and macOS): `AVSpeechSynthesizer` behind the [`Backend`] trait.
//!
//! One runtime thread owns the synthesizer and its delegate (`Retained` is not `Send`), and a
//! dispatcher thread hands delegate events to each utterance's sink, so a handler may call back
//! into the backend. The queue is ours, not AVFoundation's: `speak` queues FIFO, `interrupt`
//! cancels only the running utterance, and `stop` keeps what is queued. Synthesis starts
//! `writeUtterance:toBufferCallback:` on the runtime thread and gathers its buffers on a thread
//! of its own, one synthesis at a time, so commands keep being answered while it runs and a
//! second write never interleaves its buffers.

use std::{
    collections::{HashMap, VecDeque},
    future::ready,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    slice,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

use block2::RcBlock;
use objc2::{
    AllocAnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, ProtocolObject},
    sel,
};
use objc2_avf_audio::{
    AVSpeechBoundary, AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechSynthesizerDelegate,
    AVSpeechUtterance, AVSpeechUtteranceMaximumSpeechRate, AVSpeechUtteranceMinimumSpeechRate,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRange, NSString};
use tokio::sync::oneshot;

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, Voice},
};

pub(crate) struct NativeBackend;

/// The process's one runtime: Apple has one speech service. Built on first use; a build failure
/// is permanent and every later call reports it.
static RUNTIME: OnceLock<Result<Sender<Command>, SpeechError>> = OnceLock::new();

/// One command's answer back to its awaiting caller.
type Reply<T> = oneshot::Sender<Result<T, SpeechError>>;

enum Command {
    /// A terminal event freed the synthesizer; the queue may feed the next utterance.
    Advance,
    /// The gathering thread finished one synthesis; the slot is free again.
    SynthesisDone,
    Speak {
        text: String,
        interrupt: bool,
        sink: EventSink,
        reply: Reply<u64>,
    },
    Stop {
        reply: Reply<()>,
    },
    Pause {
        reply: Reply<()>,
    },
    Resume {
        reply: Reply<()>,
    },
    IsSpeaking {
        reply: Reply<bool>,
    },
    Init {
        reply: Reply<()>,
    },
    Synthesize {
        text: String,
        reply: Reply<Vec<u8>>,
    },
    Voices {
        reply: Reply<Vec<Voice>>,
    },
    Languages {
        reply: Reply<Vec<String>>,
    },
    SetLanguage {
        tag: String,
        reply: Reply<()>,
    },
    SetVoice {
        id: String,
        reply: Reply<()>,
    },
    SetRate {
        rate: f32,
        reply: Reply<()>,
    },
    SetPitch {
        pitch: f32,
        reply: Reply<()>,
    },
    SetVolume {
        volume: f32,
        reply: Reply<()>,
    },
    SetSilence {
        ms: u32,
        reply: Reply<()>,
    },
}

/// What the delegate saw, keyed by the utterance's address; the dispatcher turns it into the
/// event the sink takes.
#[derive(Debug)]
enum DelegateEvent {
    Start(usize),
    Progress(usize, u64, u64),
    Finish(usize),
    Cancel(usize),
}

#[derive(Debug)]
struct DelegateIvars {
    events: Sender<DelegateEvent>,
}

define_class!(
    #[derive(Debug)]
    #[unsafe(super(NSObject))]
    #[name = "NSHTTSSpeechSynthesizerDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    // Delegate methods run on AVFoundation's thread; the dispatcher thread does the rest.
    unsafe impl AVSpeechSynthesizerDelegate for Delegate {
        #[unsafe(method(speechSynthesizer:didStartSpeechUtterance:))]
        fn did_start(&self, _synthesizer: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            let _ = self
                .ivars()
                .events
                .send(DelegateEvent::Start(address(utterance)));
        }

        #[unsafe(method(speechSynthesizer:willSpeakRangeOfSpeechString:utterance:))]
        fn will_speak_range(
            &self,
            _synthesizer: &AVSpeechSynthesizer,
            character_range: NSRange,
            utterance: &AVSpeechUtterance,
        ) {
            let _ = self.ivars().events.send(DelegateEvent::Progress(
                address(utterance),
                character_range.location as u64,
                character_range.length as u64,
            ));
        }

        #[unsafe(method(speechSynthesizer:didFinishSpeechUtterance:))]
        fn did_finish(&self, _synthesizer: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            let _ = self
                .ivars()
                .events
                .send(DelegateEvent::Finish(address(utterance)));
        }

        #[unsafe(method(speechSynthesizer:didCancelSpeechUtterance:))]
        fn did_cancel(&self, _synthesizer: &AVSpeechSynthesizer, utterance: &AVSpeechUtterance) {
            let _ = self
                .ivars()
                .events
                .send(DelegateEvent::Cancel(address(utterance)));
        }
    }
);

/// Utterances the delegate may still report on, by address: the runtime inserts on speak, the
/// dispatcher delivers and removes.
type Registry = Mutex<HashMap<usize, Active>>;

struct Active {
    id: u64,
    /// A sink is only ever called from the dispatcher thread; the Mutex makes the Arc
    /// shareable, since `Box<dyn Fn + Send>` is not `Sync`.
    sink: Arc<Mutex<EventSink>>,
    /// Pins the utterance for as long as the entry lives: the registry is keyed by address, and
    /// a freed utterance's address could otherwise be recycled for the next utterance before
    /// this one's terminal event is delivered.
    _utterance: PinnedUtterance,
}

/// An utterance held across threads so its address stays taken until its terminal event has
/// been delivered. The handle is for its lifetime, not for reading: the drop releases it.
struct PinnedUtterance {
    _utterance: Retained<AVSpeechUtterance>,
}

// SAFETY: only the reference count crosses threads — ObjC's final release may run on any
// thread, and no selector is called on the dispatcher thread.
unsafe impl Send for PinnedUtterance {}

/// The settings one utterance is built from; read on the runtime thread at feed time.
struct Parameters {
    language: Option<String>,
    voice_id: Option<String>,
    rate: f32,
    pitch: f32,
    volume: f32,
    silence_ms: u32,
}

impl Parameters {
    /// AVFoundation's own defaults: mid rate, plain pitch, full volume, no leading silence.
    fn new() -> Self {
        Self {
            language: None,
            voice_id: None,
            rate: 0.5,
            pitch: 1.0,
            volume: 1.0,
            silence_ms: 0,
        }
    }
}

/// An utterance waiting for the synthesizer, with the sink its events belong to.
struct Pending {
    id: u64,
    text: String,
    sink: EventSink,
}

/// The runtime thread's state: the synthesizer, its delegate and our queue.
struct Runtime {
    synth: Retained<AVSpeechSynthesizer>,
    /// The synthesizer's delegate is a weak property; holding it keeps the delegate alive.
    _delegate: Retained<Delegate>,
    params: Parameters,
    queue: VecDeque<Pending>,
    next_id: u64,
    registry: Arc<Registry>,
    /// Whether one synthesis is gathering. Overlapping writes could interleave their buffers,
    /// so a second write while one gathers is answered `Busy`; `SynthesisDone` clears the slot.
    synthesizing: bool,
    /// The runtime's own command line, so a gathering thread can report its end.
    commands: Sender<Command>,
}

/// One synthesized buffer, in the engine's native sample type.
enum Chunk {
    F32 {
        channels: u16,
        rate: u32,
        samples: Vec<f32>,
    },
    I16 {
        channels: u16,
        rate: u32,
        samples: Vec<i16>,
    },
    /// The zero-length buffer that ends a write.
    Done,
    Unsupported,
}

/// AVAudioCommonFormat's raw values, as the raw `msg_send!` path reads them.
const COMMON_FORMAT_FLOAT32: isize = 1;
const COMMON_FORMAT_INT16: isize = 3;

/// The bound on one buffer's wait, so a stalled write cannot hang the call.
const SYNTHESIS_CHUNK_TIMEOUT: Duration = Duration::from_secs(30);

impl NativeBackend {
    /// The runtime's command line, starting the runtime on first use.
    fn commands() -> Result<&'static Sender<Command>, SpeechError> {
        match RUNTIME.get_or_init(start_runtime) {
            Ok(commands) => Ok(commands),
            Err(error) => Err(error.clone()),
        }
    }

    /// Sends one command and awaits its answer; a dead runtime settles the call either way.
    async fn request<T>(build: impl FnOnce(Reply<T>) -> Command) -> Result<T, SpeechError> {
        let commands = Self::commands()?;
        let (reply, answered) = oneshot::channel();
        commands.send(build(reply)).map_err(service_gone)?;
        answered.await.map_err(service_gone)?
    }
}

impl Backend for NativeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Init { reply }))
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(ready(Ok(synthesizer_class().is_some())))
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        let text = text.to_owned();
        Box::pin(async move {
            NativeBackend::request(move |reply| Command::Speak {
                text,
                interrupt,
                sink: on_event,
                reply,
            })
            .await
            .map(Some)
        })
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Stop { reply }))
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Pause { reply }))
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Resume { reply }))
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::IsSpeaking {
            reply,
        }))
    }

    fn synthesize(&self, text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        let text = text.to_owned();
        Box::pin(async move {
            NativeBackend::request(move |reply| Command::Synthesize { text, reply }).await
        })
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Voices { reply }))
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        // Desktops and Apple report one: the system synthesizer itself.
        Box::pin(ready(Ok(vec![engine()])))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        Box::pin(NativeBackend::request(|reply| Command::Languages { reply }))
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        // AVFoundation documents no limit on an utterance's text.
        Box::pin(ready(Ok(None)))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let tag = tag.to_owned();
        Box::pin(async move {
            NativeBackend::request(move |reply| Command::SetLanguage { tag, reply }).await
        })
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let id = id.to_owned();
        Box::pin(async move {
            NativeBackend::request(move |reply| Command::SetVoice { id, reply }).await
        })
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(move |reply| Command::SetRate {
            rate,
            reply,
        }))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(move |reply| Command::SetPitch {
            pitch,
            reply,
        }))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(move |reply| Command::SetVolume {
            volume,
            reply,
        }))
    }

    fn set_silence(&self, ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(NativeBackend::request(move |reply| Command::SetSilence {
            ms,
            reply,
        }))
    }
}

/// AVSpeechSynthesizer ships with the OS; a missing class means this build cannot run here.
fn synthesizer_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"AVSpeechSynthesizer")
}

/// A runtime that cannot start, or died, settles every call as a plain backend fault.
fn service_gone<E>(_: E) -> SpeechError {
    SpeechError::new(ErrorKind::Backend)
}

fn start_runtime() -> Result<Sender<Command>, SpeechError> {
    if synthesizer_class().is_none() {
        return Err(SpeechError::new(ErrorKind::NotAvailable));
    }
    let (commands, inbox) = channel();
    let (events, reported) = channel();
    let registry: Arc<Registry> = Arc::new(Mutex::new(HashMap::new()));
    let advance = commands.clone();
    let dispatched = Arc::clone(&registry);
    thread::Builder::new()
        .name("nsh-tts-events".into())
        .spawn(move || dispatch(reported, dispatched, advance))
        .map_err(service_gone)?;
    let answering = commands.clone();
    thread::Builder::new()
        .name("nsh-tts-avfoundation".into())
        .spawn(move || run(inbox, events, registry, answering))
        .map_err(service_gone)?;
    Ok(commands)
}

/// Answers commands until the runtime is unreachable; owns the synthesizer throughout.
fn run(
    inbox: Receiver<Command>,
    events: Sender<DelegateEvent>,
    registry: Arc<Registry>,
    commands: Sender<Command>,
) {
    let mut runtime = Runtime::new(events, registry, commands);
    for command in inbox {
        runtime.handle(command);
    }
}

impl Runtime {
    fn new(
        events: Sender<DelegateEvent>,
        registry: Arc<Registry>,
        commands: Sender<Command>,
    ) -> Self {
        // SAFETY: all ObjC here stays on this thread, and each call is thread-safe on its own.
        unsafe {
            let synth = AVSpeechSynthesizer::new();
            let delegate = Delegate::alloc().set_ivars(DelegateIvars { events });
            let delegate: Retained<Delegate> = msg_send![super(delegate), init];
            synth.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            Self {
                synth,
                _delegate: delegate,
                params: Parameters::new(),
                queue: VecDeque::new(),
                next_id: 1,
                registry,
                synthesizing: false,
                commands,
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Advance => self.advance(),
            Command::SynthesisDone => self.synthesizing = false,
            Command::Speak {
                text,
                interrupt,
                sink,
                reply,
            } => {
                let id = self.next_id;
                self.next_id += 1;
                let pending = Pending { id, text, sink };
                let running = unsafe { self.synth.isSpeaking() };
                if interrupt && running {
                    // The cancel event frees the synthesizer; the interrupter waits in front.
                    unsafe {
                        self.synth
                            .stopSpeakingAtBoundary(AVSpeechBoundary::Immediate)
                    };
                    self.queue.push_front(pending);
                } else if running || !self.queue.is_empty() {
                    self.queue.push_back(pending);
                } else {
                    self.feed(pending);
                }
                let _ = reply.send(Ok(id));
            }
            Command::Stop { reply } => {
                // Only the running utterance is cancelled; the queue is ours and survives.
                unsafe {
                    self.synth
                        .stopSpeakingAtBoundary(AVSpeechBoundary::Immediate)
                };
                let _ = reply.send(Ok(()));
            }
            Command::Pause { reply } => {
                let paused = unsafe {
                    self.synth
                        .pauseSpeakingAtBoundary(AVSpeechBoundary::Immediate)
                };
                let _ = reply.send(pause_answer(paused));
            }
            Command::Resume { reply } => {
                let resumed = unsafe { self.synth.continueSpeaking() };
                let _ = reply.send(pause_answer(resumed));
            }
            Command::IsSpeaking { reply } => {
                let speaking = unsafe { self.synth.isSpeaking() } || !self.queue.is_empty();
                let _ = reply.send(Ok(speaking));
            }
            Command::Init { reply } => {
                // SAFETY: a class-method query, safe from any thread.
                let answer = if unsafe { system_voices() }.is_empty() {
                    Err(SpeechError::new(ErrorKind::EngineMissing))
                } else {
                    Ok(())
                };
                let _ = reply.send(answer);
            }
            Command::Synthesize { text, reply } => {
                self.start_synthesis(&text, reply);
            }
            Command::Voices { reply } => {
                // SAFETY: a class-method query, safe from any thread.
                let _ = reply.send(Ok(unsafe { system_voices() }));
            }
            Command::Languages { reply } => {
                // SAFETY: a class-method query, safe from any thread.
                let voices = unsafe { system_voices() };
                let _ = reply.send(Ok(languages_of(&voices)));
            }
            Command::SetLanguage { tag, reply } => {
                // SAFETY: a class-method query, safe from any thread.
                let answer = match unsafe {
                    AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(&tag)))
                } {
                    Some(_) => {
                        self.params.language = Some(tag);
                        Ok(())
                    }
                    None => Err(SpeechError::new(ErrorKind::LanguageNotSupported)),
                };
                let _ = reply.send(answer);
            }
            Command::SetVoice { id, reply } => {
                // SAFETY: a class-method query, safe from any thread.
                let answer = match unsafe {
                    AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(&id))
                } {
                    Some(_) => {
                        self.params.voice_id = Some(id);
                        Ok(())
                    }
                    None => Err(SpeechError::new(ErrorKind::VoiceNotFound)),
                };
                let _ = reply.send(answer);
            }
            Command::SetRate { rate, reply } => {
                self.params.rate = rate;
                let _ = reply.send(Ok(()));
            }
            Command::SetPitch { pitch, reply } => {
                self.params.pitch = pitch;
                let _ = reply.send(Ok(()));
            }
            Command::SetVolume { volume, reply } => {
                self.params.volume = volume;
                let _ = reply.send(Ok(()));
            }
            Command::SetSilence { ms, reply } => {
                self.params.silence_ms = ms;
                let _ = reply.send(Ok(()));
            }
        }
    }

    /// Starts the next queued utterance when the synthesizer is free.
    fn advance(&mut self) {
        if unsafe { self.synth.isSpeaking() } {
            return;
        }
        if let Some(pending) = self.queue.pop_front() {
            self.feed(pending);
        }
    }

    /// Registers the utterance, then hands it to the synthesizer, which retains it while
    /// speaking. The entry pins the utterance too, so the address it is registered under stays
    /// this one's until the dispatcher has delivered its terminal event — the address is what
    /// the delegate reports back, and a freed address could be recycled for the next utterance.
    fn feed(&mut self, pending: Pending) {
        // SAFETY: the utterance is built and spoken on this thread.
        let utterance = unsafe { build_utterance(&self.params, &pending.text) };
        let address = std::ptr::from_ref(&*utterance) as usize;
        let Pending { id, sink, .. } = pending;
        lock(&self.registry).insert(
            address,
            Active {
                id,
                sink: Arc::new(Mutex::new(sink)),
                _utterance: PinnedUtterance {
                    _utterance: utterance.clone(),
                },
            },
        );
        // SAFETY: speakUtterance runs on the thread that owns the synthesizer.
        unsafe { self.synth.speakUtterance(&utterance) };
    }

    /// Starts one synthesis and answers it on a thread of its own: gathering the write's
    /// buffers can take as long as the audio, and the runtime thread keeps answering commands
    /// meanwhile. Only one gathers at a time — overlapping writes could interleave their
    /// buffers — so a second call while one runs is answered `Busy`. `writeUtterance:
    /// toBufferCallback:` is macOS 10.15 and iOS 13; an older system can speak but not
    /// synthesize, which is `NotAvailable` rather than a fault.
    fn start_synthesis(&mut self, text: &str, reply: Reply<Vec<u8>>) {
        let capable = self
            .synth
            .respondsToSelector(sel!(writeUtterance:toBufferCallback:));
        if let Some(error) = synthesis_refusal(capable, self.synthesizing) {
            let _ = reply.send(Err(error));
            return;
        }
        self.synthesizing = true;
        // SAFETY: the utterance is built on this thread.
        let utterance = unsafe { build_utterance(&self.params, text) };
        let (chunks, assembled) = channel();
        // The ObjC signature is void(^)(AVAudioBuffer*); AnyObject stands in for the buffer
        // type, which the block never needs by name.
        let block = RcBlock::new(move |buffer: NonNull<AnyObject>| {
            // SAFETY: the buffer is valid for the call, and read_chunk checks its class.
            let chunk = unsafe { read_chunk(buffer.as_ref()) };
            let _ = chunks.send(chunk);
        });
        let synth: &AVSpeechSynthesizer = &self.synth;
        // SAFETY: the selector was checked above, and AVFoundation copies the block for the
        // write's duration, so dropping ours below takes no live callback with it.
        unsafe {
            let _: () = msg_send![
                synth,
                writeUtterance: &*utterance,
                toBufferCallback: RcBlock::as_ptr(&block)
            ];
        }
        let done = self.commands.clone();
        let started = thread::Builder::new()
            .name("nsh-tts-synthesis".into())
            .spawn(move || settle_synthesis(&assembled, reply, done));
        if started.is_err() {
            // The failed spawn dropped `reply` with its closure — its receiver settles as a
            // backend fault — and freed the slot here, because nothing will report Done.
            self.synthesizing = false;
        }
    }
}

/// Delivers delegate events to their sinks and frees the queue after each terminal one.
fn dispatch(events: Receiver<DelegateEvent>, registry: Arc<Registry>, commands: Sender<Command>) {
    for event in events {
        let terminal = match event {
            DelegateEvent::Start(address) => {
                deliver(&registry, address, UtteranceEventKind::Start, false);
                false
            }
            DelegateEvent::Progress(address, location, length) => {
                deliver(
                    &registry,
                    address,
                    UtteranceEventKind::Progress { location, length },
                    false,
                );
                false
            }
            DelegateEvent::Finish(address) => {
                deliver(&registry, address, UtteranceEventKind::Finish, true);
                true
            }
            DelegateEvent::Cancel(address) => {
                deliver(&registry, address, UtteranceEventKind::Cancel, true);
                true
            }
        };
        if terminal {
            // Unbounded, so this never blocks; the runtime may be busy synthesizing.
            let _ = commands.send(Command::Advance);
        }
    }
}

/// Hands one event to its utterance's sink. The sink runs outside the registry lock, so a
/// handler may call back into the backend; a terminal event removes the entry, and a later
/// duplicate for the same utterance settles nothing.
fn deliver(registry: &Registry, address: usize, kind: UtteranceEventKind, terminal: bool) {
    let found = {
        let mut entries = lock(registry);
        match terminal {
            true => entries
                .remove(&address)
                .map(|active| (active.id, active.sink)),
            false => entries
                .get(&address)
                .map(|active| (active.id, Arc::clone(&active.sink))),
        }
    };
    if let Some((id, sink)) = found {
        run_sink(
            &sink,
            UtteranceEvent {
                utterance_id: id,
                kind,
            },
        );
    }
}

/// Runs one sink. A panicking handler is caught here: it would otherwise unwind the dispatcher
/// thread, and every later terminal event — and every awaited speak behind one — would be lost.
/// The app's own panic hook still reports the panic, and `lock` rides out the poisoned mutex.
fn run_sink(sink: &Mutex<EventSink>, event: UtteranceEvent) {
    let sink = lock(sink);
    let _ = catch_unwind(AssertUnwindSafe(|| sink(event)));
}

/// The address AVFoundation reports an utterance by: unique while it speaks, because the
/// synthesizer retains it.
fn address(utterance: &AVSpeechUtterance) -> usize {
    std::ptr::from_ref(utterance) as usize
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(PoisonError::into_inner)
}

/// AVFoundation answers pause and resume with a boolean; false means the state did not hold.
fn pause_answer(accepted: bool) -> Result<(), SpeechError> {
    if accepted {
        Ok(())
    } else {
        Err(SpeechError::new(ErrorKind::NotSpeaking))
    }
}

/// Whether a synthesis may start: the OS must offer the write, and only one gathers at a time.
/// Each refusal names its code.
fn synthesis_refusal(capable: bool, gathering: bool) -> Option<SpeechError> {
    if !capable {
        return Some(SpeechError::new(ErrorKind::NotAvailable));
    }
    if gathering {
        return Some(SpeechError::new(ErrorKind::Busy));
    }
    None
}

/// The API's 0.0..=1.0 rate onto AVFoundation's minimum..=maximum.
fn av_rate(rate: f32) -> f32 {
    // SAFETY: framework constants, present on every Apple target.
    let (min, max) = unsafe {
        (
            AVSpeechUtteranceMinimumSpeechRate,
            AVSpeechUtteranceMaximumSpeechRate,
        )
    };
    min + rate.clamp(0.0, 1.0) * (max - min)
}

/// The API's 0.5..=2.0 pitch onto AVFoundation's `pitchMultiplier` range.
fn av_pitch(pitch: f32) -> f32 {
    pitch.clamp(0.5, 2.0)
}

fn av_volume(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0)
}

/// `set_silence`'s milliseconds onto AVFoundation's seconds-based `preUtteranceDelay`.
fn pre_utterance_delay(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

/// Apple lists installed voices only, and its voice traits mean novelty or personal, not
/// network.
fn voice_from_system(id: &str, name: &str, language: &str) -> Voice {
    Voice {
        id: id.to_owned(),
        name: name.to_owned(),
        language: language.to_owned(),
        network_required: false,
        installed: true,
    }
}

/// The distinct BCP 47 tags among the voices, sorted so `languages()` is stable.
fn languages_of(voices: &[Voice]) -> Vec<String> {
    let mut tags: Vec<String> = voices.iter().map(|voice| voice.language.clone()).collect();
    tags.sort();
    tags.dedup();
    tags
}

/// Apple's one engine: the system synthesizer.
fn engine() -> Engine {
    Engine {
        name: "com.apple.avspeechsynthesis".to_owned(),
        label: "AVSpeechSynthesizer".to_owned(),
        default: true,
    }
}

/// The voices `speechVoices` lists, as the API reports them.
///
/// # Safety
///
/// Safe from any thread; the class method is thread-safe.
unsafe fn system_voices() -> Vec<Voice> {
    // SAFETY: see the safety note on the function.
    unsafe { AVSpeechSynthesisVoice::speechVoices() }
        .iter()
        .map(|voice| {
            // SAFETY: getters on a valid voice object.
            let id = unsafe { voice.identifier() }.to_string();
            let name = unsafe { voice.name() }.to_string();
            let language = unsafe { voice.language() }.to_string();
            voice_from_system(&id, &name, &language)
        })
        .collect()
}

/// Builds one utterance with the current settings applied. A voice that vanished since it was
/// set falls back to the language voice, or the system default.
///
/// # Safety
///
/// Must run on the runtime thread.
unsafe fn build_utterance(params: &Parameters, text: &str) -> Retained<AVSpeechUtterance> {
    // SAFETY: see the safety note on the function.
    unsafe {
        let ns_text = NSString::from_str(text);
        let utterance = AVSpeechUtterance::initWithString(AVSpeechUtterance::alloc(), &ns_text);
        utterance.setRate(av_rate(params.rate));
        utterance.setPitchMultiplier(av_pitch(params.pitch));
        utterance.setVolume(av_volume(params.volume));
        utterance.setPreUtteranceDelay(pre_utterance_delay(params.silence_ms));
        let voice = params
            .voice_id
            .as_deref()
            .and_then(|id| AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(id)))
            .or_else(|| {
                params.language.as_deref().and_then(|tag| {
                    AVSpeechSynthesisVoice::voiceWithLanguage(Some(&NSString::from_str(tag)))
                })
            });
        if let Some(voice) = voice {
            utterance.setVoice(Some(&voice));
        }
        utterance
    }
}

/// Reads one synthesized buffer. The PCM buffer accessors run through `msg_send!`, written when
/// their generated bindings were still feature-gated out; the safety notes stand either way.
///
/// # Safety
///
/// `buffer` must be a valid AVAudioBuffer for the duration of the call.
unsafe fn read_chunk(buffer: &AnyObject) -> Chunk {
    // SAFETY: selectors and types match the AVAudioPCMBuffer interface; a missing class or a
    // failing lookup yields Unsupported instead of a call on the wrong type.
    unsafe {
        let Some(pcm) = AnyClass::get(c"AVAudioPCMBuffer") else {
            return Chunk::Unsupported;
        };
        let is_pcm: bool = msg_send![buffer, isKindOfClass: pcm];
        if !is_pcm {
            return Chunk::Unsupported;
        }
        let frames: u32 = msg_send![buffer, frameLength];
        if frames == 0 {
            return Chunk::Done;
        }
        let format: Retained<AnyObject> = msg_send![buffer, format];
        let rate: f64 = msg_send![&*format, sampleRate];
        let channels: u32 = msg_send![&*format, channelCount];
        let common: isize = msg_send![&*format, commonFormat];
        let stride: usize = msg_send![buffer, stride];
        // Sample rates are small positive numbers; anything else is not PCM we can encode.
        if !(0.0..=1_000_000.0).contains(&rate) || channels > u32::from(u16::MAX) {
            return Chunk::Unsupported;
        }
        let rate = rate as u32;
        let channels = channels as u16;
        match common {
            COMMON_FORMAT_FLOAT32 => {
                let data: *mut NonNull<f32> = msg_send![buffer, floatChannelData];
                if data.is_null() {
                    return Chunk::Unsupported;
                }
                let samples = gather(data, channels.into(), frames as usize, stride);
                Chunk::F32 {
                    channels,
                    rate,
                    samples,
                }
            }
            COMMON_FORMAT_INT16 => {
                let data: *mut NonNull<i16> = msg_send![buffer, int16ChannelData];
                if data.is_null() {
                    return Chunk::Unsupported;
                }
                let samples = gather(data, channels.into(), frames as usize, stride);
                Chunk::I16 {
                    channels,
                    rate,
                    samples,
                }
            }
            _ => Chunk::Unsupported,
        }
    }
}

/// Copies one buffer's samples out interleaved: `data` holds one pointer per channel, each
/// channel's frames `stride` apart — AVAudioPCMBuffer's channel layout.
///
/// # Safety
///
/// `data` must hold `channels` valid pointers to `frames` readable samples each.
unsafe fn gather<T: Copy>(
    data: *mut NonNull<T>,
    channels: usize,
    frames: usize,
    stride: usize,
) -> Vec<T> {
    // SAFETY: see the safety note on the function.
    unsafe {
        let pointers = slice::from_raw_parts(data, channels);
        let mut samples = Vec::with_capacity(frames * pointers.len());
        for frame in 0..frames {
            for pointer in pointers {
                samples.push(*pointer.as_ptr().add(frame * stride));
            }
        }
        samples
    }
}

/// The synthesized audio gathered so far, in the format the first buffer set.
#[derive(Default)]
struct Pcm {
    channels: u16,
    rate: u32,
    samples: Vec<i16>,
}

impl Pcm {
    fn push_f32(&mut self, channels: u16, rate: u32, samples: &[f32]) -> Result<(), SpeechError> {
        self.set_format(channels, rate)?;
        self.samples
            .extend(samples.iter().map(|sample| quantize(*sample)));
        Ok(())
    }

    fn push_i16(&mut self, channels: u16, rate: u32, samples: &[i16]) -> Result<(), SpeechError> {
        self.set_format(channels, rate)?;
        self.samples.extend_from_slice(samples);
        Ok(())
    }

    /// Buffers must agree on the format; a change means the stream is not one utterance.
    fn set_format(&mut self, channels: u16, rate: u32) -> Result<(), SpeechError> {
        if channels == 0 || rate == 0 {
            return Err(failed_synthesis());
        }
        if self.samples.is_empty() && self.rate == 0 {
            self.channels = channels;
            self.rate = rate;
        } else if self.channels != channels || self.rate != rate {
            return Err(failed_synthesis());
        }
        Ok(())
    }

    fn wav(&self) -> Result<Vec<u8>, SpeechError> {
        if self.samples.is_empty() {
            return Err(failed_synthesis());
        }
        Ok(encode_wav(&self.samples, self.channels, self.rate))
    }
}

/// A synthesis that produced no usable audio; AVFoundation reports no reason code for it.
fn failed_synthesis() -> SpeechError {
    SpeechError::new(ErrorKind::SynthesisFailed)
}

/// Gathers the write's buffers into WAV bytes. A write that stalls past its bound, or ends
/// without its zero-length terminator, is a backend fault.
fn assemble(chunks: &Receiver<Chunk>) -> Result<Vec<u8>, SpeechError> {
    let mut pcm = Pcm::default();
    loop {
        let chunk = chunks
            .recv_timeout(SYNTHESIS_CHUNK_TIMEOUT)
            .map_err(|_| SpeechError::new(ErrorKind::Backend))?;
        match chunk {
            Chunk::Done => return pcm.wav(),
            Chunk::Unsupported => return Err(failed_synthesis()),
            Chunk::F32 {
                channels,
                rate,
                samples,
            } => pcm.push_f32(channels, rate, &samples)?,
            Chunk::I16 {
                channels,
                rate,
                samples,
            } => pcm.push_i16(channels, rate, &samples)?,
        }
    }
}

/// One gathering thread's whole job: answer the caller with the assembled audio, then report
/// the synthesis slot free. A panic unwinding through this drops the reply, which settles the
/// caller as a backend fault; the guard frees the slot on that path too.
fn settle_synthesis(chunks: &Receiver<Chunk>, reply: Reply<Vec<u8>>, done: Sender<Command>) {
    let _free_slot = SynthesisDoneGuard(done);
    let _ = reply.send(assemble(chunks));
}

/// Sends `SynthesisDone` when dropped — on return and on unwind alike — so the one-write slot
/// frees whenever its gathering thread ends, panic included.
struct SynthesisDoneGuard(Sender<Command>);

impl Drop for SynthesisDoneGuard {
    fn drop(&mut self) {
        let _ = self.0.send(Command::SynthesisDone);
    }
}

/// Float samples land inside 16-bit PCM without wrapping.
fn quantize(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0) as i16
}

/// A 16-bit PCM WAV file: 44-byte header, little-endian throughout.
fn encode_wav(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() as u32) * 2;
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use std::{future::Future, sync::mpsc};

    use tokio::time::timeout;

    use super::*;

    /// The runtime is process-wide: its callers take turns, so one test's settings and queries
    /// never interleave with another's.
    static RUNTIME_TURN: Mutex<()> = Mutex::new(());

    /// One backend call's bound, so a wedged runtime fails the test instead of hanging CI.
    const CALL_BOUND: Duration = Duration::from_secs(45);

    fn my_turn() -> MutexGuard<'static, ()> {
        lock(&RUNTIME_TURN)
    }

    /// Drives one backend call to its settle under the bound.
    fn drive<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                timeout(CALL_BOUND, future)
                    .await
                    .expect("the call settled in time")
            })
    }

    #[test]
    fn rate_spans_the_avfoundation_bounds() {
        // SAFETY: framework constants, present on every Apple target.
        let (min, max) = unsafe {
            (
                AVSpeechUtteranceMinimumSpeechRate,
                AVSpeechUtteranceMaximumSpeechRate,
            )
        };
        assert_eq!(av_rate(0.0), min);
        assert_eq!(av_rate(1.0), max);
        assert_eq!(av_rate(0.5), min + (max - min) / 2.0);
    }

    #[test]
    fn out_of_range_params_clamp_to_avfoundation() {
        assert_eq!(av_pitch(0.1), 0.5);
        assert_eq!(av_pitch(1.0), 1.0);
        assert_eq!(av_pitch(2.5), 2.0);
        assert_eq!(av_volume(-1.0), 0.0);
        assert_eq!(av_volume(0.25), 0.25);
        assert_eq!(av_volume(1.5), 1.0);
    }

    #[test]
    fn silence_becomes_pre_utterance_delay_seconds() {
        assert_eq!(pre_utterance_delay(0), 0.0);
        assert_eq!(pre_utterance_delay(1500), 1.5);
    }

    #[test]
    fn listed_voices_are_installed_and_offline() {
        let voice = voice_from_system("com.apple.voice.siri.Ava", "Ava", "en-US");
        assert!(voice.installed);
        assert!(!voice.network_required);
        assert_eq!(voice.language, "en-US");
    }

    #[test]
    fn languages_dedup_and_sort() {
        let voices = [
            voice_from_system("b", "B", "fr-FR"),
            voice_from_system("a", "A", "en-US"),
            voice_from_system("c", "C", "en-US"),
        ];
        assert_eq!(languages_of(&voices), ["en-US", "fr-FR"]);
    }

    #[test]
    fn apple_reports_the_one_engine() {
        let engine = engine();
        assert!(engine.default);
        assert_eq!(engine.label, "AVSpeechSynthesizer");
    }

    #[test]
    fn wav_bytes_carry_a_16_bit_pcm_header() {
        let wav = encode_wav(&[0, -1], 1, 22050);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 48);
        let word = |from: usize| u32::from_le_bytes(wav[from..from + 4].try_into().unwrap());
        let half = |from: usize| u16::from_le_bytes(wav[from..from + 2].try_into().unwrap());
        assert_eq!(word(4), 36 + 4);
        assert_eq!(half(20), 1);
        assert_eq!(half(22), 1);
        assert_eq!(word(24), 22050);
        assert_eq!(word(28), 22050 * 2);
        assert_eq!(half(32), 2);
        assert_eq!(half(34), 16);
        assert_eq!(word(40), 4);
    }

    #[test]
    fn floats_quantize_inside_the_16_bit_range() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(0.5), 16383);
        assert_eq!(quantize(2.0), 32767);
        assert_eq!(quantize(-2.0), -32767);
    }

    #[test]
    fn pcm_rejects_empty_and_mismatched_audio() {
        let mut pcm = Pcm::default();
        assert!(pcm.wav().is_err());
        pcm.push_f32(1, 22050, &[0.5]).unwrap();
        assert!(pcm.push_f32(2, 22050, &[0.5, -0.5]).is_err());
        assert!(pcm.push_i16(1, 44100, &[1]).is_err());
    }

    #[test]
    fn pcm_keeps_int16_samples_untouched() {
        let mut pcm = Pcm::default();
        pcm.push_i16(2, 44100, &[-32768, 32767]).unwrap();
        let wav = pcm.wav().unwrap();
        assert_eq!(wav.len(), 44 + 4);
        assert_eq!(&wav[44..46], &(-32768i16).to_le_bytes());
        assert_eq!(&wav[46..48], &32767i16.to_le_bytes());
    }

    #[test]
    fn assemble_encodes_the_streamed_buffers() {
        let (chunks, assembled) = mpsc::channel();
        chunks
            .send(Chunk::F32 {
                channels: 1,
                rate: 22050,
                samples: vec![0.0, 1.0],
            })
            .unwrap();
        chunks
            .send(Chunk::F32 {
                channels: 1,
                rate: 22050,
                samples: vec![-1.0],
            })
            .unwrap();
        chunks.send(Chunk::Done).unwrap();
        drop(chunks);
        let wav = assemble(&assembled).unwrap();
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(&wav[44..46], &0i16.to_le_bytes());
    }

    #[test]
    fn assemble_reports_failed_and_stalled_writes() {
        let (chunks, assembled) = mpsc::channel();
        drop(chunks);
        assert_eq!(assemble(&assembled).unwrap_err().kind(), ErrorKind::Backend);
        let (chunks, assembled) = mpsc::channel();
        chunks.send(Chunk::Unsupported).unwrap();
        drop(chunks);
        assert_eq!(
            assemble(&assembled).unwrap_err().kind(),
            ErrorKind::SynthesisFailed
        );
    }

    #[test]
    fn a_settled_synthesis_answers_then_frees_the_slot() {
        let (chunks, assembled) = mpsc::channel();
        chunks
            .send(Chunk::I16 {
                channels: 1,
                rate: 22050,
                samples: vec![0, -1],
            })
            .unwrap();
        chunks.send(Chunk::Done).unwrap();
        drop(chunks);
        let (answer, mut answered) = oneshot::channel();
        let (commands, inbox) = channel();
        settle_synthesis(&assembled, answer, commands);
        assert_eq!(answered.try_recv().unwrap().unwrap().len(), 44 + 4);
        assert!(matches!(inbox.try_recv(), Ok(Command::SynthesisDone)));
    }

    #[test]
    fn the_done_guard_frees_the_slot_when_unwound() {
        let (commands, inbox) = channel();
        let blown = catch_unwind(AssertUnwindSafe(|| {
            let _free = SynthesisDoneGuard(commands);
            panic!("gather bug");
        }));
        assert!(blown.is_err());
        assert!(matches!(
            inbox.recv_timeout(Duration::from_secs(5)),
            Ok(Command::SynthesisDone)
        ));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn a_panicking_gather_still_frees_the_slot_and_settles_the_caller() {
        // The audit's panic: channels at the u16 cap overflow encode_wav's block_align, which
        // debug builds turn into a unwind instead of a wrap.
        let (chunks, assembled) = mpsc::channel();
        chunks
            .send(Chunk::I16 {
                channels: u16::MAX,
                rate: 22050,
                samples: vec![0],
            })
            .unwrap();
        chunks.send(Chunk::Done).unwrap();
        drop(chunks);
        let (answer, mut answered) = oneshot::channel();
        let (commands, inbox) = channel();
        let blown = catch_unwind(AssertUnwindSafe(|| {
            settle_synthesis(&assembled, answer, commands)
        }));
        assert!(
            blown.is_err(),
            "the overflow should have unwound the gather"
        );
        assert!(
            matches!(
                inbox.recv_timeout(Duration::from_secs(5)),
                Ok(Command::SynthesisDone)
            ),
            "the unwound gather must free the slot, or every later synthesize answers Busy"
        );
        // The dropped reply settles the caller exactly once, as the backend fault.
        assert_eq!(
            answered
                .try_recv()
                .map_err(service_gone)
                .unwrap_err()
                .kind(),
            ErrorKind::Backend
        );
    }

    #[test]
    fn terminal_events_reach_one_sink_once() {
        let registry: Registry = Mutex::new(HashMap::new());
        // SAFETY: an allocated utterance, never spoken; only its address is used.
        let utterance =
            unsafe { AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str("x")) };
        let seen: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let sink: EventSink = Box::new(move |event| {
            recorded.lock().unwrap().push(match event.kind {
                UtteranceEventKind::Start => "start",
                UtteranceEventKind::Progress { .. } => "progress",
                UtteranceEventKind::Finish => "finish",
                UtteranceEventKind::Cancel => "cancel",
                UtteranceEventKind::Error(_) => "error",
            });
        });
        lock(&registry).insert(
            address(&utterance),
            Active {
                id: 3,
                sink: Arc::new(Mutex::new(sink)),
                _utterance: PinnedUtterance {
                    _utterance: utterance.clone(),
                },
            },
        );
        deliver(
            &registry,
            address(&utterance),
            UtteranceEventKind::Start,
            false,
        );
        deliver(
            &registry,
            address(&utterance),
            UtteranceEventKind::Progress {
                location: 2,
                length: 5,
            },
            false,
        );
        deliver(
            &registry,
            address(&utterance),
            UtteranceEventKind::Finish,
            true,
        );
        deliver(
            &registry,
            address(&utterance),
            UtteranceEventKind::Cancel,
            true,
        );
        assert_eq!(*seen.lock().unwrap(), ["start", "progress", "finish"]);
    }

    #[test]
    fn a_panicking_handler_does_not_stop_delivery() {
        let sink: Mutex<EventSink> = Mutex::new(Box::new(|_| panic!("handler bug")));
        let event = UtteranceEvent {
            utterance_id: 4,
            kind: UtteranceEventKind::Finish,
        };
        run_sink(&sink, event.clone());
        // The dispatcher lives on: the poisoned sink still takes its next event.
        let seen = Arc::new(Mutex::new(false));
        let recorded = Arc::clone(&seen);
        *lock(&sink) = Box::new(move |_| *recorded.lock().unwrap() = true);
        run_sink(&sink, event);
        assert!(*seen.lock().unwrap());
    }

    #[test]
    fn unheld_pause_or_resume_reports_not_speaking() {
        assert!(pause_answer(true).is_ok());
        assert_eq!(
            pause_answer(false).unwrap_err().kind(),
            ErrorKind::NotSpeaking
        );
    }

    #[test]
    fn a_gathering_synthesis_refuses_a_second() {
        assert_eq!(synthesis_refusal(true, false), None);
        assert_eq!(
            synthesis_refusal(true, true),
            Some(SpeechError::new(ErrorKind::Busy))
        );
        // A system without the write says so even while one would gather.
        assert_eq!(
            synthesis_refusal(false, true),
            Some(SpeechError::new(ErrorKind::NotAvailable))
        );
    }

    #[test]
    fn pcm_rejects_zero_channels_or_rate() {
        let mut pcm = Pcm::default();
        assert_eq!(
            pcm.push_i16(0, 22050, &[0]).unwrap_err().kind(),
            ErrorKind::SynthesisFailed
        );
        assert_eq!(
            pcm.push_f32(1, 0, &[0.0]).unwrap_err().kind(),
            ErrorKind::SynthesisFailed
        );
    }

    #[test]
    fn gather_interleaves_the_channels() {
        let left = [0.0f32, 1.0];
        let right = [-1.0f32, 0.5];
        let channels = [NonNull::from(&left[0]), NonNull::from(&right[0])];
        // SAFETY: two pointers to two readable samples each, stride 1, as AVAudioPCMBuffer
        // lays its channels out.
        let samples = unsafe { gather(NonNull::from(&channels[0]).as_ptr(), 2, 2, 1) };
        assert_eq!(samples, [0.0, -1.0, 1.0, 0.5]);
    }

    #[test]
    fn a_non_pcm_buffer_reads_as_unsupported() {
        let plain = NSString::from_str("not a buffer");
        // SAFETY: an upcast to the root object type is always valid; read_chunk checks the
        // class before touching any PCM accessor.
        let object: Retained<AnyObject> = unsafe { Retained::cast_unchecked(plain) };
        assert!(matches!(unsafe { read_chunk(&object) }, Chunk::Unsupported));
    }

    #[test]
    fn the_dispatcher_delivers_events_and_advances_on_terminals() {
        let registry: Arc<Registry> = Arc::new(Mutex::new(HashMap::new()));
        let (events, reported) = channel();
        let (commands, inbox) = channel();
        // SAFETY: an allocated utterance, never spoken; only its address is used.
        let utterance =
            unsafe { AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str("x")) };
        let seen: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let sink: EventSink = Box::new(move |event| {
            recorded.lock().unwrap().push(match event.kind {
                UtteranceEventKind::Start => "start",
                UtteranceEventKind::Progress { .. } => "progress",
                UtteranceEventKind::Finish => "finish",
                UtteranceEventKind::Cancel => "cancel",
                UtteranceEventKind::Error(_) => "error",
            });
        });
        lock(&registry).insert(
            address(&utterance),
            Active {
                id: 7,
                sink: Arc::new(Mutex::new(sink)),
                _utterance: PinnedUtterance {
                    _utterance: utterance.clone(),
                },
            },
        );
        events
            .send(DelegateEvent::Start(address(&utterance)))
            .unwrap();
        events
            .send(DelegateEvent::Progress(address(&utterance), 1, 1))
            .unwrap();
        events
            .send(DelegateEvent::Finish(address(&utterance)))
            .unwrap();
        // A terminal past removal settles nothing, and still frees the queue.
        events
            .send(DelegateEvent::Cancel(address(&utterance)))
            .unwrap();
        drop(events);
        dispatch(reported, Arc::clone(&registry), commands);
        assert_eq!(*seen.lock().unwrap(), ["start", "progress", "finish"]);
        assert!(matches!(inbox.try_recv(), Ok(Command::Advance)));
        assert!(matches!(inbox.try_recv(), Ok(Command::Advance)));
        assert!(lock(&registry).is_empty());
    }

    #[test]
    fn advance_on_an_idle_runtime_changes_nothing() {
        let (commands, _inbox) = channel();
        let (events, reported) = channel();
        drop(reported);
        let registry: Arc<Registry> = Arc::new(Mutex::new(HashMap::new()));
        let mut runtime = Runtime::new(events, registry, commands);
        runtime.handle(Command::Advance);
        let (reply, answered) = oneshot::channel();
        runtime.handle(Command::IsSpeaking { reply });
        assert!(!answered.blocking_recv().unwrap().unwrap());
    }

    #[test]
    fn an_empty_utterance_takes_the_speak_path_without_audio() {
        let (commands, _inbox) = channel();
        let (events, reported) = channel();
        drop(reported);
        let registry: Arc<Registry> = Arc::new(Mutex::new(HashMap::new()));
        let mut runtime = Runtime::new(events, Arc::clone(&registry), commands);
        let (reply, answered) = oneshot::channel();
        runtime.handle(Command::Speak {
            text: String::new(),
            interrupt: false,
            sink: Box::new(|_| ()),
            reply,
        });
        // The empty utterance has nothing to say: it was numbered, built and pinned, silently.
        assert_eq!(answered.blocking_recv().unwrap().unwrap(), 1);
        assert_eq!(lock(&registry).len(), 1);
    }

    #[test]
    fn queries_answer_headlessly() {
        let _turn = my_turn();
        let backend = NativeBackend;
        assert_eq!(drive(backend.is_available()), Ok(true));
        assert_eq!(drive(backend.engines()), Ok(vec![engine()]));
        assert_eq!(drive(backend.max_speech_input_length()), Ok(None));
    }

    #[test]
    fn voices_list_installed_offline_system_voices() {
        let _turn = my_turn();
        for voice in drive(NativeBackend.voices()).unwrap() {
            assert!(voice.installed);
            assert!(!voice.network_required);
            assert!(!voice.id.is_empty());
        }
    }

    #[test]
    fn languages_match_the_listed_voices_exactly() {
        let _turn = my_turn();
        let voices = drive(NativeBackend.voices()).unwrap();
        assert_eq!(
            drive(NativeBackend.languages()).unwrap(),
            languages_of(&voices)
        );
    }

    #[test]
    fn init_answers_ready_where_the_system_has_voices() {
        let _turn = my_turn();
        let voices = drive(NativeBackend.voices()).unwrap();
        // The whole contract: EngineMissing exactly where no voice exists.
        let expected = if voices.is_empty() {
            Err(ErrorKind::EngineMissing)
        } else {
            Ok(())
        };
        assert_eq!(
            drive(NativeBackend.init()).map_err(|error| error.kind()),
            expected
        );
    }

    #[test]
    fn numeric_setters_accept_their_values() {
        let _turn = my_turn();
        let backend = NativeBackend;
        assert_eq!(drive(backend.set_rate(0.25)), Ok(()));
        assert_eq!(drive(backend.set_pitch(1.5)), Ok(()));
        assert_eq!(drive(backend.set_volume(0.75)), Ok(()));
        assert_eq!(drive(backend.set_silence(250)), Ok(()));
    }

    #[test]
    fn unknown_tags_and_voices_are_refused() {
        let _turn = my_turn();
        let backend = NativeBackend;
        assert_eq!(
            drive(backend.set_language("no-such-language"))
                .unwrap_err()
                .kind(),
            ErrorKind::LanguageNotSupported
        );
        assert_eq!(
            drive(backend.set_voice("no.such.voice"))
                .unwrap_err()
                .kind(),
            ErrorKind::VoiceNotFound
        );
    }

    #[test]
    fn listed_voices_and_languages_set_cleanly() {
        let _turn = my_turn();
        let backend = NativeBackend;
        // A voiceless system cannot be set; anything listed must be.
        let voices = drive(backend.voices()).unwrap();
        let Some(voice) = voices.first() else {
            return;
        };
        assert_eq!(drive(backend.set_language(&voice.language)), Ok(()));
        assert_eq!(drive(backend.set_voice(&voice.id)), Ok(()));
    }

    #[test]
    fn idle_controls_settle_without_speaking() {
        let _turn = my_turn();
        let backend = NativeBackend;
        assert_eq!(drive(backend.is_speaking()), Ok(false));
        assert_eq!(drive(backend.stop()), Ok(()));
        assert_eq!(
            drive(backend.pause()).unwrap_err().kind(),
            ErrorKind::NotSpeaking
        );
        assert_eq!(
            drive(backend.resume()).unwrap_err().kind(),
            ErrorKind::NotSpeaking
        );
    }

    #[test]
    fn synthesize_settles_once_with_wav_or_a_coded_fault() {
        let _turn = my_turn();
        // writeUtterance:toBufferCallback: renders to buffers and plays nothing. A headless
        // runtime (CI, no speech session) never starts the write: the call then settles
        // through the chunk bound as `Backend`, or `NotAvailable` where the write is absent.
        match drive(NativeBackend.synthesize("synthesized, not spoken")) {
            Ok(bytes) => {
                assert_eq!(&bytes[0..4], b"RIFF");
                assert_eq!(&bytes[8..12], b"WAVE");
                assert!(bytes.len() > 44, "the write produced no audio");
            }
            Err(error) => assert!(
                matches!(error.kind(), ErrorKind::Backend | ErrorKind::NotAvailable),
                "unexpected error: {error:?}"
            ),
        }
    }
}
