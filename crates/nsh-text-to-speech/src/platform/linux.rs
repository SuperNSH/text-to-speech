//! Linux: Speech Dispatcher over SSIP, one connection the process shares.
//!
//! speechd answers through notifications on the same socket as command replies, so a dedicated
//! thread owns the connection: it runs each command and, between commands, reads the running
//! utterance's events into its `EventSink`. Connecting fails with `NotAvailable` — speechd is
//! not running. Voices are the output module's synthesis voices; SSIP cannot hand back audio
//! bytes, so `synthesize` reports `NotAvailable`.
//!
//! Queue order: `stop` and an interrupting `speak` use SSIP `STOP`, which ends the running
//! utterance and keeps the queue. On `interrupt` the new utterance joins behind whatever was
//! already queued — it does not leap the queue as the fake backend has it, because SSIP has no
//! command that starts one message before another while keeping both: `CANCEL` drops this
//! client's whole queue, and a higher priority would make the interrupting utterance
//! unpausable.

use std::{
    collections::{HashMap, VecDeque},
    future::ready,
    io,
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use ssip_client_async::{
    Client, ClientError, ClientName, ClientScope, MessageScope, NotificationType, Priority,
    Request, Response, SynthesisVoice,
    constants::{
        ERR_COULDNT_SET_LANGUAGE, ERR_COULDNT_SET_VOICE, EVENT_BEGIN, EVENT_CANCELED, EVENT_END,
        EVENT_PAUSED, EVENT_RESUMED, OK_MESSAGE_QUEUED,
    },
    fifo::synchronous::{Builder, UnixStream},
};
use tokio::{sync::oneshot, time::timeout};

use crate::{
    backend::{Backend, BackendFuture, EventSink},
    error::{ErrorKind, SpeechError},
    options::{Engine, UtteranceEvent, UtteranceEventKind, Voice},
};

/// How long one socket read waits before the thread checks for commands again.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// A read that times out is retried this often, about a second, before a command gives up.
const RECEIVE_RETRIES: usize = 100;
/// How long the thread waits for a command before reading the socket for events.
const COMMAND_POLL: Duration = Duration::from_millis(1);
/// Every call resolves within this, even when speechd wedges mid-command.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a begun utterance may run from its last begin or resume before it fails.
/// speechd reports no progress mid-utterance, so nothing shorter can tell a long read
/// from a wedged output module; an hour bounds the wedge while outlasting real reading.
const UTTERANCE_LIMIT: Duration = Duration::from_secs(60 * 60);
/// How long speechd may answer nothing — no reply, no refusal, no notification —
/// before utterances still waiting to begin or resume fail. Any answer restarts it,
/// so a busy queue never fails a waiting utterance; a day outlasts any honest pause.
const SILENCE_LIMIT: Duration = Duration::from_secs(60 * 60 * 24);

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn init(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(async { session().await.map(|_| ()) })
    }

    fn is_available(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(async {
            match session().await {
                Ok(_) => Ok(true),
                // Speechd being absent is the honest "no", not a failure of the query.
                Err(error) if error.kind() == ErrorKind::NotAvailable => Ok(false),
                Err(error) => Err(error),
            }
        })
    }

    fn speak(
        &self,
        text: &str,
        interrupt: bool,
        on_event: EventSink,
    ) -> BackendFuture<'_, Result<Option<u64>, SpeechError>> {
        let text = text.to_owned();
        Box::pin(call(
            move |reply| Command::Speak {
                text,
                interrupt,
                sink: on_event,
                reply,
            },
            reply_id,
        ))
    }

    fn stop(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(call(|reply| Command::Stop { reply }, reply_done))
    }

    fn pause(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(call(|reply| Command::Pause { reply }, reply_done))
    }

    fn resume(&self) -> BackendFuture<'_, Result<(), SpeechError>> {
        Box::pin(call(|reply| Command::Resume { reply }, reply_done))
    }

    fn is_speaking(&self) -> BackendFuture<'_, Result<bool, SpeechError>> {
        Box::pin(call(|reply| Command::IsSpeaking { reply }, reply_flag))
    }

    fn synthesize(&self, _text: &str) -> BackendFuture<'_, Result<Vec<u8>, SpeechError>> {
        // SSIP speaks through the daemon's own audio output; no command returns bytes.
        Box::pin(ready(Err(SpeechError::new(ErrorKind::NotAvailable))))
    }

    fn voices(&self) -> BackendFuture<'_, Result<Vec<Voice>, SpeechError>> {
        Box::pin(call(|reply| Command::Voices { reply }, reply_voices))
    }

    fn engines(&self) -> BackendFuture<'_, Result<Vec<Engine>, SpeechError>> {
        Box::pin(ready(Ok(vec![Engine {
            name: "speech-dispatcher".to_owned(),
            label: "Speech Dispatcher".to_owned(),
            default: true,
        }])))
    }

    fn languages(&self) -> BackendFuture<'_, Result<Vec<String>, SpeechError>> {
        Box::pin(async {
            // speechd has no LIST LANGUAGES; the voices carry the languages there are.
            let voices = call(|reply| Command::Voices { reply }, reply_voices).await?;
            Ok(languages_of(&voices))
        })
    }

    fn max_speech_input_length(&self) -> BackendFuture<'_, Result<Option<usize>, SpeechError>> {
        Box::pin(ready(Ok(None)))
    }

    fn set_language(&self, tag: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let tag = tag.to_owned();
        Box::pin(call(
            move |reply| Command::SetLanguage { tag, reply },
            reply_done,
        ))
    }

    fn set_voice(&self, id: &str) -> BackendFuture<'_, Result<(), SpeechError>> {
        let id = id.to_owned();
        Box::pin(call(
            move |reply| Command::SetVoice { id, reply },
            reply_done,
        ))
    }

    fn set_rate(&self, rate: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        let value = ssip_rate(rate);
        Box::pin(call(
            move |reply| Command::SetRate { value, reply },
            reply_done,
        ))
    }

    fn set_pitch(&self, pitch: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        let value = ssip_pitch(pitch);
        Box::pin(call(
            move |reply| Command::SetPitch { value, reply },
            reply_done,
        ))
    }

    fn set_volume(&self, volume: f32) -> BackendFuture<'_, Result<(), SpeechError>> {
        let value = ssip_volume(volume);
        Box::pin(call(
            move |reply| Command::SetVolume { value, reply },
            reply_done,
        ))
    }

    fn set_silence(&self, _ms: u32) -> BackendFuture<'_, Result<(), SpeechError>> {
        // speechd has no leading-silence parameter; accepted so the settings surface stays uniform.
        Box::pin(ready(Ok(())))
    }
}

/// Runs `make`'s command and decodes its one answer, bounded by `CALL_TIMEOUT` whatever speechd
/// does: the thread's reply, its death, or the clock all resolve the call exactly once.
async fn call<T: Send + 'static>(
    make: impl FnOnce(ReplySlot) -> Command,
    decode: fn(Reply) -> Result<T, SpeechError>,
) -> Result<T, SpeechError> {
    let link = session().await?;
    let (reply, answer) = oneshot::channel();
    link.commands
        .send(make(reply))
        .map_err(|_| SpeechError::new(ErrorKind::Backend))?;
    let answer = timeout(CALL_TIMEOUT, answer)
        .await
        .map_err(|_| SpeechError::new(ErrorKind::Backend))?
        .unwrap_or_else(|_| Err(SpeechError::new(ErrorKind::Backend)));
    answer.and_then(decode)
}

fn reply_id(reply: Reply) -> Result<Option<u64>, SpeechError> {
    match reply {
        Reply::Id(id) => Ok(Some(id)),
        _ => Err(SpeechError::new(ErrorKind::Backend)),
    }
}

fn reply_done(reply: Reply) -> Result<(), SpeechError> {
    match reply {
        Reply::Done => Ok(()),
        _ => Err(SpeechError::new(ErrorKind::Backend)),
    }
}

fn reply_flag(reply: Reply) -> Result<bool, SpeechError> {
    match reply {
        Reply::Flag(flag) => Ok(flag),
        _ => Err(SpeechError::new(ErrorKind::Backend)),
    }
}

fn reply_voices(reply: Reply) -> Result<Vec<Voice>, SpeechError> {
    match reply {
        Reply::Voices(voices) => Ok(voices),
        _ => Err(SpeechError::new(ErrorKind::Backend)),
    }
}

/// The handle the futures share: a mailbox to the thread that owns the connection, and the
/// flag the thread clears on its way out.
struct Link {
    id: u64,
    commands: mpsc::Sender<Command>,
    alive: Arc<AtomicBool>,
}

/// The process's speechd link, once connected; cleared when its thread ends.
static LINK: LazyLock<Mutex<Option<Arc<Link>>>> = LazyLock::new(|| Mutex::new(None));
static NEXT_LINK_ID: AtomicU64 = AtomicU64::new(0);

fn lock_link() -> MutexGuard<'static, Option<Arc<Link>>> {
    LINK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The process's speechd link, connecting on first use. A link whose thread already ended is
/// dropped from the registry so this call reconnects; a concurrent connect that wins the
/// registry makes the loser's thread idle out; a failed connect leaves the slot empty.
async fn session() -> Result<Arc<Link>, SpeechError> {
    if let Some(link) = lock_link().clone() {
        if link.alive.load(Ordering::Acquire) {
            return Ok(link);
        }
        // Its thread ended; clear that link only, so one installed meanwhile survives.
        let mut current = lock_link();
        if current
            .as_ref()
            .is_some_and(|installed| Arc::ptr_eq(installed, &link))
        {
            *current = None;
        }
    }
    let id = NEXT_LINK_ID.fetch_add(1, Ordering::Relaxed);
    let (commands, mailbox) = mpsc::channel();
    let (ready, connected) = oneshot::channel();
    let alive = Arc::new(AtomicBool::new(true));
    thread::Builder::new()
        .name("nsh-speech-dispatcher".into())
        .spawn({
            let alive = Arc::clone(&alive);
            move || run(mailbox, ready, id, alive)
        })
        .map_err(|_| SpeechError::new(ErrorKind::Backend))?;
    let link = Arc::new(Link {
        id,
        commands,
        alive,
    });
    match timeout(CALL_TIMEOUT, connected).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(error))) => return Err(error),
        // The thread died before answering, or the handshake outstayed the call bound.
        Ok(Err(_)) | Err(_) => return Err(SpeechError::new(ErrorKind::Backend)),
    }
    let mut current = lock_link();
    if let Some(installed) = current.as_ref() {
        return Ok(Arc::clone(installed));
    }
    // Rechecked under the lock: the thread may have ended while this call held the door.
    if !link.alive.load(Ordering::Acquire) {
        return Err(SpeechError::new(ErrorKind::Backend));
    }
    *current = Some(Arc::clone(&link));
    Ok(link)
}

/// Removes the registry entry when this thread's link is the installed one, so the next call
/// reconnects instead of mailing a dead thread.
fn deregister(link_id: u64) {
    let mut current = lock_link();
    if current.as_ref().is_some_and(|link| link.id == link_id) {
        *current = None;
    }
}

/// Owns the connection: connects, then serves commands and reads events until either side ends.
/// Whatever ends the service — error, disconnect, panic — every sink settles and the registry
/// entry goes, so no call waits on a dead link.
fn run(
    mailbox: Receiver<Command>,
    ready: oneshot::Sender<Result<(), SpeechError>>,
    link_id: u64,
    alive: Arc<AtomicBool>,
) {
    let mut session = match connect() {
        Ok(session) => session,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return; // The caller left before the handshake answered.
    }
    // ssip-client-async's receive panics on a status code it does not enumerate; the cleanup
    // below still runs, keeping the backend's never-hangs promise.
    let _ = panic::catch_unwind(AssertUnwindSafe(|| serve(&mut session, &mailbox)));
    session.fail_all();
    alive.store(false, Ordering::Release);
    deregister(link_id);
}

fn serve(session: &mut Session, mailbox: &Receiver<Command>) {
    loop {
        match mailbox.recv_timeout(COMMAND_POLL) {
            Ok(command) => {
                if !session.execute(command) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        match session.client.receive() {
            Ok(response) => session.notify(response),
            Err(ClientError::Io(error)) if is_timeout(&error) => {}
            Err(_) => return,
        }
        session.expire();
    }
}

/// Connects to speechd and runs its opening handshake: a name, a pausable priority, notifications.
fn connect() -> Result<Session, SpeechError> {
    let mut session = Session::new(
        Builder::new()
            .timeout(POLL_INTERVAL)
            .build()
            // A missing or refusing socket is speechd not running, not a failure to speak.
            .map_err(|_| SpeechError::new(ErrorKind::NotAvailable))?,
    );
    let setup = [
        Request::SetName(ClientName::with_component(
            "nsh",
            "nsh-text-to-speech",
            "speech",
        )),
        // Priority Text queues first in, first out and stays pausable; ssip 0.5.0's Progress
        // serializes as "important", which resists pausing.
        Request::SetPriority(Priority::Text),
        Request::SetNotification(NotificationType::Begin, true),
        Request::SetNotification(NotificationType::End, true),
        Request::SetNotification(NotificationType::Cancel, true),
        Request::SetNotification(NotificationType::Pause, true),
        Request::SetNotification(NotificationType::Resume, true),
    ];
    for request in setup {
        session
            .command(request)
            .map_err(|error| map_client_error(&error))?;
    }
    Ok(session)
}

/// One call the futures make, answered once through the channel inside.
enum Command {
    Speak {
        text: String,
        interrupt: bool,
        sink: EventSink,
        reply: ReplySlot,
    },
    Stop {
        reply: ReplySlot,
    },
    Pause {
        reply: ReplySlot,
    },
    Resume {
        reply: ReplySlot,
    },
    IsSpeaking {
        reply: ReplySlot,
    },
    Voices {
        reply: ReplySlot,
    },
    SetLanguage {
        tag: String,
        reply: ReplySlot,
    },
    SetVoice {
        id: String,
        reply: ReplySlot,
    },
    SetRate {
        value: i8,
        reply: ReplySlot,
    },
    SetPitch {
        value: i8,
        reply: ReplySlot,
    },
    SetVolume {
        value: i8,
        reply: ReplySlot,
    },
}

/// Where a command's answer goes; the thread sends exactly once.
type ReplySlot = oneshot::Sender<Result<Reply, SpeechError>>;

/// A command's answer.
enum Reply {
    Id(u64),
    Done,
    Flag(bool),
    Voices(Vec<Voice>),
}

/// A command's answer, and whether the connection can take another.
struct Outcome {
    answer: Result<Reply, SpeechError>,
    alive: bool,
}

impl Outcome {
    /// A wire result that ends a command: speechd's refusal answers with `map` and the session
    /// lives on; a broken connection answers `Backend` and ends the session.
    fn from_wire(result: Result<(), ClientError>, map: fn(&ClientError) -> SpeechError) -> Self {
        match result {
            Ok(()) => Self {
                answer: Ok(Reply::Done),
                alive: true,
            },
            Err(error) if is_broken(&error) => Self {
                answer: Err(map_client_error(&error)),
                alive: false,
            },
            Err(error) => Self {
                answer: Err(map(&error)),
                alive: true,
            },
        }
    }
}

fn broken(error: SpeechError) -> Outcome {
    Outcome {
        answer: Err(error),
        alive: false,
    }
}

/// An utterance's sink, and which clock bounds its patience with speechd.
struct AwaitingSink {
    sink: EventSink,
    patience: Patience,
}

/// What a sink waits on. `Waiting` covers queued and paused utterances alike: neither
/// is speaking, so only silence counts against them, never time passing.
#[derive(Clone, Copy)]
enum Patience {
    /// The instant past which the utterance's speaking, since its last begin or
    /// resume, has run too long.
    Speaking(Instant),
    /// Bounded by silence since the session's last answer.
    Waiting,
}

/// The deadline past which a sink fails itself: a begun utterance at its own limit
/// since its last begin or resume — activity elsewhere must not stretch a wedged
/// speak — a waiting one a day past the session's last answer.
fn sink_deadline(patience: Patience, last_activity: Instant) -> Instant {
    match patience {
        Patience::Speaking(expires) => expires,
        Patience::Waiting => last_activity + SILENCE_LIMIT,
    }
}

/// The utterance lifecycle of one connection, and the sinks awaiting each utterance's events.
struct Session {
    client: Client<UnixStream>,
    /// Submitted to speechd, not yet begun, oldest first.
    queued: VecDeque<u64>,
    active: Option<u64>,
    speaking: bool,
    paused: bool,
    /// When speechd last answered: a notification, a reply or a refusal. Waiting sinks
    /// are bounded by silence since this instant.
    last_activity: Instant,
    /// Each utterance's sink, until its terminal event.
    sinks: HashMap<u64, AwaitingSink>,
}

impl Session {
    fn new(client: Client<UnixStream>) -> Self {
        Self {
            client,
            queued: VecDeque::new(),
            active: None,
            speaking: false,
            paused: false,
            last_activity: Instant::now(),
            sinks: HashMap::new(),
        }
    }

    /// Runs one command, answering its caller once. `false` when the connection is broken.
    fn execute(&mut self, command: Command) -> bool {
        let (reply, outcome) = match command {
            Command::Speak {
                text,
                interrupt,
                sink,
                reply,
            } => (reply, self.speak(&text, interrupt, sink)),
            Command::Stop { reply } => (reply, self.stop_running()),
            Command::Pause { reply } => (reply, self.pause()),
            Command::Resume { reply } => (reply, self.resume()),
            Command::IsSpeaking { reply } => (
                reply,
                Outcome {
                    answer: Ok(Reply::Flag(self.is_speaking())),
                    alive: true,
                },
            ),
            Command::Voices { reply } => (reply, self.voices()),
            Command::SetLanguage { tag, reply } => (reply, self.set_language(&tag)),
            Command::SetVoice { id, reply } => (reply, self.set_voice(&id)),
            Command::SetRate { value, reply } => (
                reply,
                self.set_parameter(Request::SetRate(ClientScope::Current, value)),
            ),
            Command::SetPitch { value, reply } => (
                reply,
                self.set_parameter(Request::SetPitch(ClientScope::Current, value)),
            ),
            Command::SetVolume { value, reply } => (
                reply,
                self.set_parameter(Request::SetVolume(ClientScope::Current, value)),
            ),
        };
        let _ = reply.send(outcome.answer);
        outcome.alive
    }

    /// Whether an utterance is being spoken, now or queued.
    fn is_speaking(&self) -> bool {
        self.speaking || self.active.is_some() || !self.queued.is_empty()
    }

    /// Submits `text`, stopping the running utterance first when `interrupt`; whatever else is
    /// queued stays ahead of it, first in, first out.
    fn speak(&mut self, text: &str, interrupt: bool, sink: EventSink) -> Outcome {
        if interrupt {
            let outcome = self.stop_running();
            if !outcome.alive || outcome.answer.is_err() {
                return outcome;
            }
        }
        match self.submit(text) {
            Ok(id) => {
                self.queued.push_back(id);
                self.sinks.insert(
                    id,
                    AwaitingSink {
                        sink,
                        patience: Patience::Waiting,
                    },
                );
                Outcome {
                    answer: Ok(Reply::Id(id)),
                    alive: true,
                }
            }
            Err(error) if is_broken(&error) => broken(map_client_error(&error)),
            Err(error) => Outcome {
                answer: Err(map_client_error(&error)),
                alive: true,
            },
        }
    }

    /// Submits `text` to speechd and returns the message id it reports.
    fn submit(&mut self, text: &str) -> Result<u64, ClientError> {
        self.client.send(Request::Speak)?;
        self.receive_answer()?; // Consumes the 230 OK RECEIVING DATA that precedes the text.
        self.client.send_lines(&data_lines(text))?;
        self.message_id()
    }

    /// Stops the running utterance with SSIP `STOP` — the queue is kept and plays next, as the
    /// trait promises. The stopped utterance's sink is answered here already; the `cancel`
    /// speechd fires for it finds no sink and only tidies the lifecycle.
    fn stop_running(&mut self) -> Outcome {
        // Nothing submitted and nothing paused means nothing can be running.
        if !self.is_speaking() && !self.paused {
            return Outcome {
                answer: Ok(Reply::Done),
                alive: true,
            };
        }
        let outcome = Outcome::from_wire(
            self.command(Request::Stop(MessageScope::Last)),
            map_client_error,
        );
        if outcome.alive
            && outcome.answer.is_ok()
            && let Some(id) = self.active.take()
        {
            self.speaking = false;
            self.paused = false;
            self.fire(id, UtteranceEventKind::Cancel);
        }
        outcome
    }

    fn pause(&mut self) -> Outcome {
        if self.paused {
            return Outcome {
                answer: Ok(Reply::Done),
                alive: true,
            };
        }
        if !self.is_speaking() {
            return Outcome {
                answer: Err(SpeechError::new(ErrorKind::NotSpeaking)),
                alive: true,
            };
        }
        let outcome = Outcome::from_wire(
            self.command(Request::Pause(MessageScope::Last)),
            map_client_error,
        );
        if outcome.alive && outcome.answer.is_ok() {
            self.paused = true;
            self.speaking = false;
        }
        outcome
    }

    fn resume(&mut self) -> Outcome {
        if !self.paused {
            return Outcome {
                answer: Err(SpeechError::new(ErrorKind::NotSpeaking)),
                alive: true,
            };
        }
        let outcome = Outcome::from_wire(
            self.command(Request::Resume(MessageScope::Last)),
            map_client_error,
        );
        if outcome.alive && outcome.answer.is_ok() {
            self.paused = false;
            self.speaking = true;
        }
        outcome
    }

    fn set_language(&mut self, tag: &str) -> Outcome {
        Outcome::from_wire(
            self.command(Request::SetLanguage(ClientScope::Current, tag.to_owned())),
            map_language_error,
        )
    }

    fn set_parameter(&mut self, request: Request) -> Outcome {
        Outcome::from_wire(self.command(request), map_client_error)
    }

    /// The voices the current output module lists; any other answer has desynchronized the
    /// protocol, so the session ends instead of guessing.
    fn voices(&mut self) -> Outcome {
        match self.request(Request::ListSynthesisVoices) {
            Ok(Response::VoicesListSent(listed)) => Outcome {
                answer: Ok(Reply::Voices(voices_of(&listed))),
                alive: true,
            },
            Ok(_) => broken(unexpected_reply()),
            Err(error) if is_broken(&error) => broken(map_client_error(&error)),
            Err(error) => Outcome {
                answer: Err(map_client_error(&error)),
                alive: true,
            },
        }
    }

    /// Sets the voice of later utterances. The module is asked which voices exist first, so an
    /// unknown name is refused without touching the synthesizer.
    fn set_voice(&mut self, id: &str) -> Outcome {
        let listed = match self.request(Request::ListSynthesisVoices) {
            Ok(Response::VoicesListSent(listed)) => listed,
            Ok(_) => return broken(unexpected_reply()),
            Err(error) if is_broken(&error) => return broken(map_client_error(&error)),
            Err(error) => {
                return Outcome {
                    answer: Err(map_client_error(&error)),
                    alive: true,
                };
            }
        };
        if !listed.iter().any(|voice| voice.name == id) {
            return Outcome {
                answer: Err(SpeechError::new(ErrorKind::VoiceNotFound)),
                alive: true,
            };
        }
        Outcome::from_wire(
            self.command(Request::SetSynthesisVoice(
                ClientScope::Current,
                id.to_owned(),
            )),
            map_voice_error,
        )
    }

    /// Sends one request and waits for its answer.
    fn command(&mut self, request: Request) -> Result<(), ClientError> {
        self.request(request).map(|_| ())
    }

    /// Sends one request and returns its answer, applying events met on the way.
    fn request(&mut self, request: Request) -> Result<Response, ClientError> {
        self.client.send(request)?;
        self.receive_answer()
    }

    /// Receives until a non-notification answer, applying any event met on the way.
    fn receive_answer(&mut self) -> Result<Response, ClientError> {
        for _ in 0..RECEIVE_RETRIES {
            match self.client.receive() {
                Ok(response) => match notification_of(&response) {
                    Some((notification, id)) => self.apply(notification, id),
                    None => {
                        self.last_activity = Instant::now();
                        return Ok(response);
                    }
                },
                Err(ClientError::Io(error)) if is_timeout(&error) => {}
                Err(error) => {
                    // A refusal answers too; a dead connection fails everything regardless.
                    self.last_activity = Instant::now();
                    return Err(error);
                }
            }
        }
        Err(timeout_error())
    }

    /// Reads the message id speechd reports with 225, applying any notification that came first
    /// and is known, from lifecycle order, to belong to this client.
    fn message_id(&mut self) -> Result<u64, ClientError> {
        for _ in 0..RECEIVE_RETRIES {
            match self.client.receive_lines(OK_MESSAGE_QUEUED) {
                Ok(lines) => {
                    return lines
                        .first()
                        .and_then(|line| line.parse().ok())
                        .ok_or_else(|| ClientError::invalid_data("no message id in the reply"));
                }
                Err(ClientError::UnexpectedStatus(code)) if is_event_code(code) => {
                    let queued = self.queued.make_contiguous();
                    if let Some((notification, id)) = infer_event(queued, self.active, code) {
                        self.apply(notification, id);
                    }
                }
                Err(ClientError::Io(error)) if is_timeout(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Err(timeout_error())
    }

    /// Routes one answer read while idle: an event for its utterance, anything else unexpected.
    fn notify(&mut self, response: Response) {
        if let Some((notification, id)) = notification_of(&response) {
            self.apply(notification, id);
        }
    }

    /// Applies one notification to the lifecycle and fires its event, if our model has one.
    fn apply(&mut self, notification: Notification, id: u64) {
        // Any notification is speechd alive: restart the silence clock waiting sinks run on.
        self.last_activity = Instant::now();
        match notification {
            Notification::Begin => {
                self.queued.retain(|queued| *queued != id);
                self.active = Some(id);
                self.speaking = true;
                self.speaking_patience(id);
            }
            Notification::End => {
                if self.active == Some(id) {
                    self.active = None;
                }
                self.speaking = false;
                self.paused = false;
            }
            Notification::Cancel => {
                self.queued.retain(|queued| *queued != id);
                if self.active == Some(id) {
                    self.active = None;
                }
                self.speaking = false;
            }
            Notification::Pause => {
                self.speaking = false;
                self.paused = true;
                self.waiting_patience(id);
            }
            Notification::Resume => {
                self.speaking = true;
                self.paused = false;
                self.speaking_patience(id);
            }
        }
        if let Some(kind) = event_kind(notification) {
            self.fire(id, kind);
        }
    }

    /// Bounds a sink by its utterance's own speaking: it began or resumed, so it is
    /// legitimately speaking, and only a later begin or resume extends it.
    fn speaking_patience(&mut self, id: u64) {
        if let Some(awaiting) = self.sinks.get_mut(&id) {
            awaiting.patience = Patience::Speaking(Instant::now() + UTTERANCE_LIMIT);
        }
    }

    /// Bounds a sink by the session's silence: paused is waiting, not speaking.
    fn waiting_patience(&mut self, id: u64) {
        if let Some(awaiting) = self.sinks.get_mut(&id) {
            awaiting.patience = Patience::Waiting;
        }
    }

    /// Fires one event into its utterance's sink, dropping the sink when it is terminal.
    fn fire(&mut self, id: u64, kind: UtteranceEventKind) {
        let terminal = matches!(
            kind,
            UtteranceEventKind::Finish | UtteranceEventKind::Cancel
        );
        if terminal {
            if let Some(awaiting) = self.sinks.remove(&id) {
                (awaiting.sink)(UtteranceEvent {
                    utterance_id: id,
                    kind,
                });
            }
        } else if let Some(awaiting) = self.sinks.get(&id) {
            (awaiting.sink)(UtteranceEvent {
                utterance_id: id,
                kind,
            });
        }
    }

    /// Fails the sinks past their deadline — a begun utterance whose terminal notification
    /// never came, or a waiting one after the session fell silent — so no awaited speak
    /// outlives either bound; a late notification for one only tidies the lifecycle.
    fn expire(&mut self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .sinks
            .iter()
            .filter(|(_, awaiting)| sink_deadline(awaiting.patience, self.last_activity) <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if self.active == Some(id) {
                self.active = None;
                self.speaking = false;
                self.paused = false;
            }
            self.queued.retain(|queued| *queued != id);
            if let Some(awaiting) = self.sinks.remove(&id) {
                (awaiting.sink)(UtteranceEvent {
                    utterance_id: id,
                    kind: UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
                });
            }
        }
    }

    /// Ends every pending utterance with an error event: the connection is gone.
    fn fail_all(&mut self) {
        self.queued.clear();
        self.active = None;
        self.speaking = false;
        self.paused = false;
        for (id, awaiting) in self.sinks.drain() {
            (awaiting.sink)(UtteranceEvent {
                utterance_id: id,
                kind: UtteranceEventKind::Error(SpeechError::new(ErrorKind::Backend)),
            });
        }
    }
}

/// A speechd notification, stripped of its wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notification {
    Begin,
    End,
    Cancel,
    Pause,
    Resume,
}

/// The notification a response carries, with the utterance id it names; `None` for every other
/// response and for an id speechd did not make a number.
fn notification_of(response: &Response) -> Option<(Notification, u64)> {
    let (notification, id) = match response {
        Response::EventBegin(id) => (Notification::Begin, id),
        Response::EventEnd(id) => (Notification::End, id),
        Response::EventCanceled(id) => (Notification::Cancel, id),
        Response::EventPaused(id) => (Notification::Pause, id),
        Response::EventResumed(id) => (Notification::Resume, id),
        _ => return None,
    };
    let id = id.message.parse().ok()?;
    Some((notification, id))
}

/// The event kind our model has for a notification; pause and resume change state, not events.
fn event_kind(notification: Notification) -> Option<UtteranceEventKind> {
    match notification {
        Notification::Begin => Some(UtteranceEventKind::Start),
        Notification::End => Some(UtteranceEventKind::Finish),
        Notification::Cancel => Some(UtteranceEventKind::Cancel),
        Notification::Pause | Notification::Resume => None,
    }
}

/// Attributes a notification that `receive_lines` swallowed whole, leaving only its status code:
/// at a single priority the front of the queue is the next to begin and the active utterance the
/// one that ends, pauses or resumes; a cancel falls back to the queue's front.
fn infer_event(queued: &[u64], active: Option<u64>, code: u16) -> Option<(Notification, u64)> {
    match code {
        EVENT_BEGIN => queued.first().map(|id| (Notification::Begin, *id)),
        EVENT_END => active.map(|id| (Notification::End, id)),
        EVENT_CANCELED => active
            .or_else(|| queued.first().copied())
            .map(|id| (Notification::Cancel, id)),
        EVENT_PAUSED => active.map(|id| (Notification::Pause, id)),
        EVENT_RESUMED => active.map(|id| (Notification::Resume, id)),
        _ => None,
    }
}

/// Whether a status code is one of the five lifecycle notifications, which can arrive in place
/// of an awaited reply. Index marks are excluded: never requested, and unattributable.
fn is_event_code(code: u16) -> bool {
    (EVENT_BEGIN..=EVENT_RESUMED).contains(&code)
}

/// SET LANGUAGE's SSIP refusal: 302 names the language itself, anything else is speechd's own
/// failure.
fn map_language_error(error: &ClientError) -> SpeechError {
    match error {
        ClientError::Ssip(status) if status.code == ERR_COULDNT_SET_LANGUAGE => {
            SpeechError::new(ErrorKind::LanguageNotSupported)
        }
        other => map_client_error(other),
    }
}

/// SET SYNTHESIS_VOICE's SSIP refusal: 309 names the voice, anything else is speechd's own
/// failure.
fn map_voice_error(error: &ClientError) -> SpeechError {
    match error {
        ClientError::Ssip(status) if status.code == ERR_COULDNT_SET_VOICE => {
            SpeechError::new(ErrorKind::VoiceNotFound)
        }
        other => map_client_error(other),
    }
}

/// An answer whose shape does not fit its request; the session restarts rather than guess what
/// speechd meant.
fn unexpected_reply() -> SpeechError {
    map_client_error(&ClientError::invalid_data(
        "an answer that does not fit its request",
    ))
}

/// A client fault becomes `Backend`, carrying speechd's own status code when it gave one.
fn map_client_error(error: &ClientError) -> SpeechError {
    let status = match error {
        ClientError::Ssip(status) => Some(i64::from(status.code)),
        ClientError::UnexpectedStatus(code) => Some(i64::from(*code)),
        _ => None,
    };
    status.map_or_else(
        || SpeechError::new(ErrorKind::Backend),
        |status| SpeechError::new(ErrorKind::Backend).with_status(status),
    )
}

/// Whether an error ends the connection: every fault but a read that merely timed out.
fn is_broken(error: &ClientError) -> bool {
    matches!(error, ClientError::Io(error) if !is_timeout(error))
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn timeout_error() -> ClientError {
    ClientError::Io(io::Error::new(
        io::ErrorKind::TimedOut,
        "speechd did not answer",
    ))
}

/// Maps the normalized rate 0.0..=1.0 onto SSIP's -100..=100; 0.5 is the unmodified rate.
fn ssip_rate(rate: f32) -> i8 {
    scaled(rate * 200.0 - 100.0)
}

/// Maps the pitch multiplier 0.5..=2.0 around its normal 1.0, not linearly through the range:
/// half pitch reaches SSIP's -100, double pitch its +100, and normal its 0.
fn ssip_pitch(pitch: f32) -> i8 {
    let step = if pitch < 1.0 { 200.0 } else { 100.0 };
    scaled((pitch - 1.0) * step)
}

/// Maps the normalized volume 0.0..=1.0 onto SSIP's -100..=100, as tts-rs maps it.
fn ssip_volume(volume: f32) -> i8 {
    scaled(volume * 200.0 - 100.0)
}

/// Rounds onto SSIP's range, clamped to its ends.
fn scaled(value: f32) -> i8 {
    value.round().clamp(-100.0, 100.0) as i8
}

/// Doubles leading dots so no data line reads as the end-of-data marker.
fn data_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            if line.starts_with('.') {
                format!(".{line}")
            } else {
                line.to_owned()
            }
        })
        .collect()
}

/// The voices speechd listed, as ours: its synthesis-voice name is the id SET takes, and every
/// listed voice is a local, installed synthesizer. A voice with no language cannot be named, so
/// it is left out, as tts-rs leaves it.
fn voices_of(listed: &[SynthesisVoice]) -> Vec<Voice> {
    listed
        .iter()
        .filter_map(|voice| {
            let language = voice_language(voice.language.as_deref(), voice.dialect.as_deref())?;
            Some(Voice {
                id: voice.name.clone(),
                name: voice.name.clone(),
                language,
                network_required: false,
                installed: true,
            })
        })
        .collect()
}

/// The BCP 47 tag of a listed voice: its language, with its dialect as the region when it has
/// one; no language, no tag.
fn voice_language(language: Option<&str>, dialect: Option<&str>) -> Option<String> {
    let language = language?;
    Some(match dialect {
        Some(dialect) => format!("{language}-{dialect}"),
        None => language.to_owned(),
    })
}

/// The distinct languages of the voices, in the order speechd listed them.
fn languages_of(voices: &[Voice]) -> Vec<String> {
    let mut languages: Vec<String> = Vec::new();
    for voice in voices {
        if !languages.contains(&voice.language) {
            languages.push(voice.language.clone());
        }
    }
    languages
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssip_client_async::{EventId, StatusLine, constants::EVENT_INDEX_MARK};

    #[test]
    fn normalized_settings_map_onto_ssip_hundred_steps() {
        assert_eq!(ssip_rate(0.0), -100);
        assert_eq!(ssip_rate(0.5), 0);
        assert_eq!(ssip_rate(1.0), 100);
        // Pitch runs through its normal, both ways: 0.75 and 1.5 are its half steps.
        assert_eq!(ssip_pitch(0.5), -100);
        assert_eq!(ssip_pitch(0.75), -50);
        assert_eq!(ssip_pitch(1.0), 0);
        assert_eq!(ssip_pitch(1.5), 50);
        assert_eq!(ssip_pitch(2.0), 100);
        assert_eq!(ssip_volume(0.0), -100);
        assert_eq!(ssip_volume(0.5), 0);
        assert_eq!(ssip_volume(1.0), 100);
    }

    #[test]
    fn settings_past_the_range_clamp_to_the_ssip_ends() {
        // Speech checks ranges first; the clamp keeps a stray value inside SSIP's own bounds.
        assert_eq!(ssip_rate(1.5), 100);
        assert_eq!(ssip_rate(-1.0), -100);
        assert_eq!(ssip_pitch(3.0), 100);
        assert_eq!(ssip_volume(-0.5), -100);
    }

    #[test]
    fn data_lines_double_leading_dots() {
        assert_eq!(
            data_lines("hello\n.world"),
            ["hello", "..world"].map(String::from)
        );
        assert_eq!(data_lines("plain text"), ["plain text"].map(String::from));
        assert_eq!(data_lines("."), [".."].map(String::from));
        assert_eq!(data_lines("a\n\nb"), ["a", "", "b"].map(String::from));
    }

    #[test]
    fn set_language_refusal_maps_to_language_not_supported() {
        let refusal = ClientError::Ssip(StatusLine {
            code: ERR_COULDNT_SET_LANGUAGE,
            message: String::new(),
        });
        assert_eq!(
            map_language_error(&refusal).kind(),
            ErrorKind::LanguageNotSupported
        );
    }

    #[test]
    fn other_ssip_faults_carry_speechd_status_as_backend() {
        let internal = ClientError::Ssip(StatusLine {
            code: 300,
            message: String::new(),
        });
        assert_eq!(map_client_error(&internal).kind(), ErrorKind::Backend);
        assert_eq!(map_client_error(&internal).status(), Some(300));
        assert_eq!(
            map_client_error(&ClientError::UnexpectedStatus(409)).status(),
            Some(409)
        );
        assert_eq!(map_client_error(&ClientError::NotReady).status(), None);
    }

    #[test]
    fn notification_status_codes_are_the_five_lifecycle_events() {
        for code in [
            EVENT_BEGIN,
            EVENT_END,
            EVENT_CANCELED,
            EVENT_PAUSED,
            EVENT_RESUMED,
        ] {
            assert!(is_event_code(code));
        }
        assert!(!is_event_code(EVENT_INDEX_MARK));
        assert!(!is_event_code(OK_MESSAGE_QUEUED));
    }

    #[test]
    fn responses_carry_their_notification_and_utterance_id() {
        assert_eq!(
            notification_of(&Response::EventBegin(EventId::new("8", "2"))),
            Some((Notification::Begin, 8))
        );
        assert_eq!(
            notification_of(&Response::EventEnd(EventId::new("9", "2"))),
            Some((Notification::End, 9))
        );
        assert_eq!(notification_of(&Response::RateSet), None);
        assert_eq!(
            notification_of(&Response::EventCanceled(EventId::new("x", "2"))),
            None
        );
    }

    #[test]
    fn pause_and_resume_change_state_but_fire_no_event() {
        assert_eq!(event_kind(Notification::Pause), None);
        assert_eq!(event_kind(Notification::Resume), None);
        assert_eq!(
            event_kind(Notification::Begin),
            Some(UtteranceEventKind::Start)
        );
        assert_eq!(
            event_kind(Notification::End),
            Some(UtteranceEventKind::Finish)
        );
        assert_eq!(
            event_kind(Notification::Cancel),
            Some(UtteranceEventKind::Cancel)
        );
    }

    #[test]
    fn swallowed_notifications_are_attributed_by_lifecycle_order() {
        let queued = [4, 5];
        assert_eq!(
            infer_event(&queued, Some(3), EVENT_BEGIN),
            Some((Notification::Begin, 4))
        );
        assert_eq!(
            infer_event(&queued, Some(3), EVENT_END),
            Some((Notification::End, 3))
        );
        assert_eq!(
            infer_event(&queued, None, EVENT_CANCELED),
            Some((Notification::Cancel, 4))
        );
        assert_eq!(
            infer_event(&queued, Some(3), EVENT_PAUSED),
            Some((Notification::Pause, 3))
        );
        assert_eq!(
            infer_event(&queued, Some(3), EVENT_RESUMED),
            Some((Notification::Resume, 3))
        );
        assert_eq!(infer_event(&queued, Some(3), OK_MESSAGE_QUEUED), None);
    }

    #[test]
    fn listed_voices_become_our_voices() {
        let listed = [
            SynthesisVoice::new("Festival", Some("pt"), Some("BR")),
            SynthesisVoice::new("Esperanto", Some("eo"), None),
            SynthesisVoice::new("Nameless", None, Some("BR")),
        ];
        let voices = voices_of(&listed);
        assert_eq!(voices.len(), 2); // A voice with no language cannot be named.
        assert_eq!(
            voices[0],
            Voice {
                id: "Festival".to_owned(),
                name: "Festival".to_owned(),
                language: "pt-BR".to_owned(),
                network_required: false,
                installed: true,
            }
        );
        assert_eq!(voices[1].language, "eo");
        assert_eq!(voices[1].id, "Esperanto");
    }

    #[test]
    fn voice_language_joins_dialect_as_region() {
        assert_eq!(
            voice_language(Some("pt"), Some("BR")),
            Some("pt-BR".to_owned())
        );
        assert_eq!(voice_language(Some("eo"), None), Some("eo".to_owned()));
        assert_eq!(voice_language(None, Some("BR")), None);
    }

    #[test]
    fn languages_are_distinct_in_listed_order() {
        let listed = voices_of(&[
            SynthesisVoice::new("A", Some("en"), Some("US")),
            SynthesisVoice::new("B", Some("pt"), Some("BR")),
            SynthesisVoice::new("C", Some("en"), Some("US")),
        ]);
        assert_eq!(languages_of(&listed), ["en-US", "pt-BR"].map(String::from));
    }

    #[test]
    fn set_voice_refusal_maps_to_voice_not_found() {
        let refusal = ClientError::Ssip(StatusLine {
            code: ERR_COULDNT_SET_VOICE,
            message: String::new(),
        });
        assert_eq!(map_voice_error(&refusal).kind(), ErrorKind::VoiceNotFound);
        let internal = ClientError::Ssip(StatusLine {
            code: 300,
            message: String::new(),
        });
        assert_eq!(map_voice_error(&internal).kind(), ErrorKind::Backend);
        assert_eq!(map_voice_error(&internal).status(), Some(300));
    }

    #[test]
    fn waiting_sink_survives_activity() {
        // Two hours of live speech past the utterance limit: the answers speechd gave
        // in between restart the silence clock, so the queued sink's deadline holds.
        let submitted = Instant::now();
        let last_activity = submitted + UTTERANCE_LIMIT + Duration::from_secs(60 * 60);
        assert!(sink_deadline(Patience::Waiting, last_activity) > submitted + SILENCE_LIMIT);
    }

    #[test]
    fn waiting_sink_expires_on_silence() {
        // A day after speechd's last answer the waiting sink is at its deadline; a
        // healthy session never goes that long without answering something.
        let last_activity = Instant::now();
        assert_eq!(
            sink_deadline(Patience::Waiting, last_activity),
            last_activity + SILENCE_LIMIT
        );
    }

    #[test]
    fn speaking_deadline_ignores_session_activity() {
        // A begun utterance answers only to its own begin or resume: replies to other
        // calls must not stretch a wedged speak past the utterance limit.
        let expires = Instant::now() + UTTERANCE_LIMIT;
        let last_activity = expires + SILENCE_LIMIT;
        assert_eq!(
            sink_deadline(Patience::Speaking(expires), last_activity),
            expires
        );
    }
}
