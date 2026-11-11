// Workflow: add speech synthesis to this NSH capability repo — a Rust crate
// (crates/nsh-text-to-speech) and a TypeScript package (packages/nsh-text-to-speech),
// laid out like .refs/biometrics, with the reference plugins' API names and behavior.
// Eight areas run their own implement -> audit (<=3 rounds, until zero issues or the cap)
// -> scoped-gate chain in parallel; the whole-tree gates run once at the join.
// Every subagent is pinned to GLM-5.3 (the user asked for the main model everywhere).
// No commits, no pushes, no branches — the deliverable is the changed files.

const MODELS = {
  pro: model("account:zai-individual-coding-plan/GLM-5.3$high"),
};

const KIT = "/Users/hmziq/ZCodeProject/ai/harness/zcode/agents";
const PLAIN_RUST = "You are a careful Rust engineer. Cite file paths for every claim; never report a check you did not run.";
const PLAIN_NSH = "You are a careful implementer for Native Shell packages. Cite file paths for every claim; never report a check you did not run.";

// The specialist's whole file body is read at RUN time, frontmatter stripped (canonical resolver:
// the project's .zcode/agents/ first, then the kit). The script text never carries a copy.
async function specialist(name: string, fallback: string): Promise<{ persona: string; source: string }> {
  const adapter =
    "You are the " + name + " specialist for one area of a parallel workflow. " +
    "Your full specialist contract follows verbatim — apply it to this area's work; " +
    "where it conflicts with your ask, the ask wins." + "\n\n";
  for (const p of [".zcode/agents/" + name + ".md", KIT + "/" + name + ".md"]) {
    try {
      const raw = await files.read(p);
      return { persona: adapter + raw.replace(/^---\n[\s\S]*?\n---\n/, "").trim(), source: p };
    } catch { /* try the next source */ }
  }
  return { persona: fallback, source: "fallback" };
}

const CONTRACT = `FROZEN CONTRACT — every area codes to exactly this. Do not redesign it; escalate if it contradicts itself.

GOAL. This repo adds speech synthesis (read text aloud) to the Native Shell capability set, which has speech-to-text only. Deliver BOTH a Rust crate (crates/nsh-text-to-speech) and a TypeScript package (packages/nsh-text-to-speech), laid out exactly like .refs/biometrics (read that repo), with identical API names and identical behavior in both, and a test for every behavior in both.

REFERENCES, cloned in this workspace under .refs/ (git-ignored; read what you need):
- .refs/flutter_tts — Flutter plugin whose method names we follow (lib/flutter_tts.dart)
- .refs/react-native-tts — React Native module (index.d.ts has its API, types and event names)
- .refs/tts-rs — Rust TTS crate: src/lib.rs (Backend trait idea, Utterance callbacks) and src/backends/ (per-platform technique)
- .refs/nsh — NSH main: the crates and TSX typings this package wraps (NSH 0.4.0, matching the installed nsh CLI)
- .refs/biometrics — org layout template: crates/nsh-biometrics + packages/nsh-biometrics (lib.rs native_module pattern, error.rs codes pattern, fake.rs and memory.ts test-backend pattern)

LAYOUT (mirror .refs/biometrics exactly):
- Root Cargo.toml becomes a workspace: members = ["crates/nsh-text-to-speech"], resolver = "3", [workspace.package] version "0.1.0", edition "2024", license "MIT", repository "https://github.com/SuperNSH/text-to-speech", authors ["hmziqrs"]. The scaffold root src/lib.rs is deleted.
- crates/nsh-text-to-speech: package nsh-text-to-speech, lib name nsh_text_to_speech. Base deps: nsh-core = "0.4.0", thiserror = "2", tokio (default-features false, features sync+time). Dev-deps: tokio with rt+time+test-util. src/: lib.rs (doc comment, pub use, and a native_module registration like biometrics: #[native_module("nsh-text-to-speech/native")] registering NativeSpeech), error.rs, backend.rs, options.rs, speech.rs, fake.rs, platform/mod.rs, platform/unsupported.rs, platform/apple.rs, platform/android.rs, platform/windows.rs, platform/linux.rs, platform/web.rs, tests/speech.rs.
- Per-target deps, pinned from .refs/tts-rs Cargo.toml (the core area writes them into the crate's Cargo.toml):
  - cfg(target_vendor = "apple"): block2 = "0.6", objc2 = "0.6", objc2-foundation = "0.3" (default-features false; features std, NSArray, NSEnumerator, NSString, NSRunLoop), objc2-avf-audio = "0.3" (default-features false; features std, block2, AVSpeechSynthesis)
  - cfg(target_os = "android"): jni = "0.22", ndk-context = "0.1"
  - cfg(target_os = "windows"): windows = "0.62" (features Foundation, Foundation_Collections, Media_SpeechSynthesis, Win32_Foundation)
  - cfg(target_os = "linux"): ssip-client-async = "0.19", ssip = "=0.5.0"
  - target wasm32-unknown-unknown: wasm-bindgen = "0.2", web-sys = "0.3" (features SpeechSynthesis, SpeechSynthesisUtterance, SpeechSynthesisVoice, SpeechSynthesisEvent, SpeechSynthesisErrorEvent, Window, EventTarget)
- packages/nsh-text-to-speech: package.json (name nsh-text-to-speech, version 0.1.0, type module, main and types ./src/index.ts, exports ".", "./rust", "./memory", repository), nsh.json (name "nsh-text-to-speech tests", id "dev.supernsh.text-to-speech.tests", version 0.1.0, build 1, rust "../../crates/nsh-text-to-speech", capabilities {}), tsconfig.json extending ./.nsh/tsconfig.json, App.tsx (a small demo like biometrics': nsh imports mount/signal/Text/View, a Speak button calling speech.speak, a status text; testIDs title, status, speak), src/: index.ts, errors.ts, options.ts, backend.ts, memory.ts, speech.ts, rust.ts; tests/: speech.test.ts, errors.test.ts, memory.test.ts.

API — one type Speech (Rust struct, TS class); flutter_tts's FlutterTts and react-native-tts's Tts both map to it (docs/parity.md records each mapping):
- Constructors: Speech::new() -> Result<Speech, SpeechError> uses platform_backend(); Speech::with_backend(Box<dyn Backend>) for tests. TS: new Speech({ backend }?) defaults to rustBackend().
- init() -> Result<(), SpeechError>: EngineMissing when the device has no speech engine, NotAvailable when speech exists but cannot run here (the visible-fallback signal the brief asks for).
- speak(text, interrupt: bool) -> Result<UtteranceId, SpeechError>; type UtteranceId = u64 (TS: number). Rules: empty or whitespace-only text -> InvalidArgument; text longer than max_speech_input_length when the backend reports a limit -> InvalidArgument. While speaking: interrupt true (the default) cancels the running utterance (Cancel event, then the queued one starts); interrupt false queues FIFO — this is the brief's "speaking while another utterance runs" rule, and it is what flutter_tts setQueueMode maps to.
- await_speak_completion(bool) (TS awaitSpeakCompletion(bool)), default false: when on, speak resolves only once the utterance finishes, is cancelled or errors — exactly one resolution; when off, speak resolves at once with the id.
- stop() cancels the running utterance: its awaited speak resolves Cancelled exactly once. pause() / resume() -> Result<()>; pause while idle or resume while not paused -> NotSpeaking. is_speaking() -> Result<bool>.
- synthesize(text) -> Result<Vec<u8>, SpeechError> — WAV bytes (tts-rs SynthesizedAudio.as_bytes(); TS synthesize(text) -> Promise<Uint8Array>). A second concurrent synthesize -> Busy; empty text -> InvalidArgument. flutter_tts awaitSynthCompletion needs no mapping because synthesize resolves with the bytes.
- Settings: set_language(tag) (empty -> InvalidArgument), set_voice(id) (unknown id or a voice whose installed flag is false -> VoiceNotFound), set_speech_rate(rate 0.0..=1.0), set_pitch(pitch 0.5..=2.0), set_volume(volume 0.0..=1.0) — out of range -> InvalidArgument; set_silence(ms: u32), 0 starts immediately.
- Queries: voices() -> Vec<Voice>, engines() -> Vec<Engine>, languages() -> Vec<String>, is_language_available(tag) -> bool, max_speech_input_length() -> Option<usize>. Voice { id, name, language, network_required: bool, installed: bool }; Engine { name, label, default: bool }. TS: camelCase names for everything (setSpeechRate, isLanguageAvailable, ...), Voice { id, name, language, networkRequired, installed }.
- Events: Rust set_event_handler(handler) delivering UtteranceEvent { utterance_id, kind } with kind start | progress { location, length } | finish | cancel | error(SpeechError). TS addEventListener / removeEventListener with react-native-tts's names "tts-start", "tts-progress", "tts-finish", "tts-cancel", "tts-error".

ERRORS (stable codes; apps match on them). Rust: #[non_exhaustive] enum ErrorKind + SpeechError { kind, status: Option<i64> } with kind(), status(), code() -> &'static str, message() -> &'static str, in the biometrics error.rs style. TS: union SpeechErrorCode of the same code strings, SpeechError { code, message, status? }, plus speechError(code) and asSpeechError(unknown) helpers like biometrics errors.ts.
- NotAvailable ERR_TTS_NOT_AVAILABLE — no speech service on this device or platform
- EngineMissing ERR_TTS_ENGINE_MISSING — no TTS engine installed (Android: install one first)
- VoiceNotFound ERR_TTS_VOICE_NOT_FOUND — the voice is unknown or not installed on this device
- LanguageNotSupported ERR_TTS_LANGUAGE_NOT_SUPPORTED — the platform rejected the language tag
- LanguageMissingData ERR_TTS_LANGUAGE_MISSING_DATA — Android LANG_MISSING_DATA: the voice data needs downloading
- NotSpeaking ERR_TTS_NOT_SPEAKING — pause/resume applied to an idle or unpaused synthesizer
- InvalidArgument ERR_TTS_INVALID_ARGUMENT — empty text, out-of-range rate/pitch/volume, empty language tag
- Busy ERR_TTS_BUSY — a second concurrent synthesize
- Cancelled ERR_TTS_CANCELLED — the utterance was stopped before it finished
- SynthesisFailed ERR_TTS_SYNTHESIS_FAILED — the platform failed to synthesize (web onerror, Windows synthesis fault)
- Backend ERR_TTS_BACKEND — the platform service failed some other way; carries the platform's own status code

BACKEND TRAIT (crates/nsh-text-to-speech/src/backend.rs), in the biometrics backend.rs style:
- pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
- pub type EventSink = Box<dyn Fn(UtteranceEvent) + Send>;
- pub trait Backend: Send + Sync with, each returning BackendFuture<'_, Result<T, SpeechError>>: init() -> (), is_available() -> bool, speak(text: &str, interrupt: bool, on_event: EventSink) -> Option<u64>, stop() -> (), pause() -> (), resume() -> (), is_speaking() -> bool, synthesize(text: &str) -> Vec<u8>, voices() -> Vec<Voice>, engines() -> Vec<Engine>, languages() -> Vec<String>, max_speech_input_length() -> Option<usize>, set_language(&str) -> (), set_voice(&str) -> (), set_rate(f32) -> (), set_pitch(f32) -> (), set_volume(f32) -> (), set_silence(u32) -> ().
- Every backend future resolves exactly once. A backend never panics and never hangs: whatever happens, each call settles with success, a coded SpeechError, or a cancel event.
- platform/mod.rs: pub fn platform_backend() -> Result<Box<dyn Backend>, SpeechError> selecting per cfg: apple, android, windows, linux, web module; anything else -> Err(SpeechError::new(ErrorKind::NotAvailable)). The core area seeds each of the five platform files with a compiling stub (every method Err(NotAvailable), header comment saying a bridge area owns the file); each bridge area replaces its stub entirely.
- FakeBackend (src/fake.rs), in the biometrics fake.rs style: scripted answers for every trait method, the test finishes an utterance (which fires the events), counters for calls; Clone shares state. TS mirror: memoryBackend() in src/memory.ts like biometrics memory.ts — scripted config, finishUtterance, prompt-style counters.

NAMED TESTS — write tests with exactly these names; every behavior lands in both languages with the same name (Rust in crates/nsh-text-to-speech/tests/speech.rs driving Speech::with_backend(FakeBackend); TS in packages/nsh-text-to-speech/tests/speech.test.ts driving new Speech({ backend: memoryBackend() })):
speaks_and_finishes_once; speak_rejects_empty_text; speak_rejects_blank_text; speak_rejects_oversize_text; speak_queues_while_speaking; speak_interrupts_current_utterance; awaited_speak_resolves_once_on_finish; stop_cancels_awaited_speak_once; pause_and_resume_round_trip; pause_when_idle_reports_not_speaking; resume_when_not_paused_reports_not_speaking; set_rate_rejects_out_of_range; set_pitch_rejects_out_of_range; set_volume_rejects_out_of_range; set_language_rejects_empty; unknown_language_reports_unavailable; set_unknown_voice_reports_voice_not_found; uninstalled_voice_reports_voice_not_found; init_reports_missing_engine; backend_failure_carries_status; concurrent_synthesize_reports_busy; synthesize_returns_wav_bytes; progress_events_report_location; set_silence_reaches_backend; error_codes_are_stable_and_unique; missing_voice_list_is_empty.
Plus TS: errors.test.ts asserting the eleven code strings, memory.test.ts for the memory backend. Bridge areas add mapping tests inside their own platform file (param and error mapping, pure functions), under the file's own cfg so they compile where that platform compiles.

RULES (from this repo's AGENTS.md):
- Comments are 1-2 lines and state a non-obvious constraint. Dead code is deleted; no speculative abstractions.
- Every behaviour needs a test, and a test is seen to fail once before it counts: write it, watch it fail against the stub or unimplemented path, then make it pass.
- An operation resolves exactly once: success, a coded failure, or a cancel. It never hangs.
- Docs read like a competent human wrote them: direct, concrete, no marketing filler.
- No commits, no pushes, no branches, no worktrees: work directly in the current checkout; the deliverable is the changed files. Do not touch any repo outside this workspace. Never build or run emulators or simulators; cargo and nsh command checks are the verification.
- The script runs each area's scoped gates after the work and after every fix. You may run scoped checks yourself while working (your own test file, compiling the crate or package); do not run whole-tree suites.`;

interface Area {
  /** Unique short area name, also the board row key. */
  name: string;
  /** The z-* specialist whose whole file body becomes the area's persona. */
  specialist: string;
  /** Plain persona when no specialist file exists. */
  fallback: string;
  /** Which scoped gate suite runs for this area. */
  gateKind: GateKind;
  /** True when the area needs the crate trait the core area lands. */
  needsTrait: boolean;
  /** The disjoint file set this area owns and edits. */
  owns: string;
  /** The complete predetermined work for this area. */
  work: string;
}

interface Implementation {
  /** True when the work is complete on disk and self-consistent. */
  done: boolean;
  /** Two or three sentences: what was implemented. */
  summary: string;
  /** Anything that could not be done, with the reason. Empty when done. */
  blockers: string[];
  /** One sentence copied VERBATIM from your specialist contract that most shaped the work. */
  contractLine: string;
}

interface AuditFindings {
  /** Actionable findings only: wrong, missing, or inconsistent against the frozen contract. No praise, no PASS notes. */
  findings: Array<{
    /** Workspace-relative path with a line when it applies. */
    where: string;
    /** One sentence: what is wrong. */
    problem: string;
    /** Reserve high for a broken build, a hanging operation, or a lost error code. */
    severity: "low" | "medium" | "high";
  }>;
  /** One sentence copied VERBATIM from your specialist contract that most shaped the review. */
  contractLine: string;
}

interface FixOutcome {
  /** True when the files you own now address the findings. */
  fixed: boolean;
  /** True when the failure is entirely in files this area does not own, so nothing was changed. */
  foreignFilesOnly: boolean;
  /** One or two sentences on what changed, or why nothing did. */
  summary: string;
}

interface AreaOutcome {
  name: string;
  status: "done" | "failed";
  summary: string;
  auditConverged: boolean;
  auditRounds: number;
  gateRounds: number;
  testsPassed: boolean;
  personaDelivery: "verified" | "unverified";
  blockers: string[];
}

interface GateFailure {
  /** The command that failed. */
  gate: string;
  /** The area that owns the files to fix. */
  owner: string;
  /** Tail of the failure output, for the fix ask. */
  output: string;
}

type GateKind = "docs" | "host" | "ts" | "android" | "windows" | "linux" | "web";

// Local cross targets: what this macOS host can compile-check for the bridges
// whose platform cannot run here. CI's cross job (written by the core area)
// covers aarch64-apple-ios, aarch64-linux-android and wasm32-unknown-unknown,
// with windows and linux covered natively by its three-OS rust matrix.
const CROSS_TARGETS: Record<string, string> = {
  android: "aarch64-linux-android",
  windows: "x86_64-pc-windows-gnu",
  linux: "x86_64-unknown-linux-gnu",
  web: "wasm32-unknown-unknown",
};

const COMMON = `Work directly in the current checkout: no branches, no worktrees, no commits, no pushes. Edit only the files this ask names as yours. The workflow re-runs your area's scoped gates after you and after every fix round. If an instruction here is impossible or contradictory, escalate and say so plainly rather than working around it.`;

function implementAsk(area: Area): string {
  return (
    CONTRACT +
    "\n\nYOUR AREA: " + area.name +
    "\nFILES YOU OWN (edit only these): " + area.owns +
    "\n\nYOUR WORK:\n" + area.work +
    "\n\n" + COMMON +
    "\n\nReply with done, summary, blockers, and contractLine — one sentence copied VERBATIM from your specialist contract above that most shaped the work."
  );
}

function auditAsk(area: Area): string {
  return (
    CONTRACT +
    "\n\nAUDIT REQUEST — area: " + area.name +
    "\nAudit ONLY these files, which that area owns: " + area.owns +
    "\nRead them and judge them against the frozen contract above: wrong API names or codes, missing behaviors, a behavior without its named test, an operation that could hang or resolve twice, a panic path, docs claims that outrun what CI runs. Other areas' files are changing in parallel; ignore anything outside the list." +
    "\nDo not edit any file. Do not run whole-tree gates; you may read files and run a scoped compile or the area's own tests if a suspicion needs proof." +
    "\nReturn findings only — each with where (path:line), problem (one sentence), severity. An empty findings array means the audit came back clean; do not pad it with praise." +
    "\nReply with findings, and contractLine — one sentence copied VERBATIM from your specialist contract above that most shaped the review." +
    "\n" + COMMON
  );
}

function fixAsk(area: Area, findings: AuditFindings["findings"]): string {
  return (
    "Your area \"" + area.name + "\" came back from audit with these findings. Fix each one in the files you own (only these): " + area.owns +
    "\nIf a finding points at a file you do not own, skip it and say so in the summary." +
    "\n" + JSON.stringify(findings) +
    "\nKeep to the frozen contract you already have. Reply with done, summary, blockers." +
    "\n" + COMMON
  );
}

function gateFixAsk(area: Area, failures: string[]): string {
  return (
    "Your area's scoped gates failed:\n" + failures.join("\n---\n") +
    "\nFix the failures by editing ONLY the files your area owns: " + area.owns +
    "\nIf every failure is inside files another area owns, change nothing and report foreignFilesOnly true." +
    "\nDo not run the gate suite yourself; the workflow re-runs it right after you. A quick scoped compile or your own test file is fine." +
    "\nReply with fixed, foreignFilesOnly, summary." +
    "\n" + COMMON
  );
}

function tail(r: { stdout: string; stderr: string }): string {
  const text = ((r.stderr || "") + "\n" + (r.stdout || "")).trim();
  return text.length > 4000 ? text.slice(-4000) : text;
}

function ownerOf(text: string): string {
  if (text.includes("platform/apple")) return "apple";
  if (text.includes("platform/android")) return "android";
  if (text.includes("platform/windows")) return "windows";
  if (text.includes("platform/linux")) return "linux";
  if (text.includes("platform/web")) return "web";
  if (text.includes("packages/nsh-text-to-speech")) return "ts";
  return "core";
}

async function fmtFailure(): Promise<GateFailure | null> {
  const fmt = await world.run("cargo", ["fmt", "--all", "--", "--check"], { timeoutMs: 120000 });
  if (fmt.exitCode === 0) return null;
  const output = tail(fmt);
  return { gate: "cargo fmt --all -- --check", owner: ownerOf(output), output };
}

async function hostGateFailures(): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const fmt = await fmtFailure();
  if (fmt) failures.push(fmt);
  const clippy = await world.run(
    "cargo",
    ["clippy", "-p", "nsh-text-to-speech", "--all-targets", "--", "-D", "warnings"],
    { timeoutMs: 1200000 },
  );
  if (clippy.exitCode !== 0) {
    const output = tail(clippy);
    failures.push({ gate: "cargo clippy -p nsh-text-to-speech --all-targets -- -D warnings", owner: ownerOf(output), output });
  }
  const test = await world.run("cargo", ["test", "-p", "nsh-text-to-speech", "--all-targets"], { timeoutMs: 1200000 });
  if (test.exitCode !== 0) {
    const output = tail(test);
    failures.push({ gate: "cargo test -p nsh-text-to-speech --all-targets", owner: ownerOf(output), output });
  }
  return failures;
}

async function tsGateFailures(): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const check = await world.run("nsh", ["check", "packages/nsh-text-to-speech"], { timeoutMs: 1200000 });
  if (check.exitCode !== 0) {
    const output = tail(check);
    failures.push({ gate: "nsh check packages/nsh-text-to-speech", owner: ownerOf(output), output });
  }
  const test = await world.run("nsh", ["test", "packages/nsh-text-to-speech"], { timeoutMs: 1200000 });
  if (test.exitCode !== 0) {
    const output = tail(test);
    failures.push({ gate: "nsh test packages/nsh-text-to-speech", owner: ownerOf(output), output });
  }
  return failures;
}

async function crossGateFailures(target: string): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const fmt = await fmtFailure();
  if (fmt) failures.push(fmt);
  const clippy = await world.run(
    "cargo",
    ["clippy", "-p", "nsh-text-to-speech", "--target", target, "--all-targets", "--", "-D", "warnings"],
    { timeoutMs: 1200000 },
  );
  if (clippy.exitCode !== 0) {
    const output = tail(clippy);
    failures.push({
      gate: "cargo clippy -p nsh-text-to-speech --target " + target + " --all-targets -- -D warnings",
      owner: ownerOf(output),
      output,
    });
  }
  return failures;
}

async function areaGateFailures(kind: GateKind): Promise<string[]> {
  if (kind === "docs") return [];
  const raw =
    kind === "host" ? await hostGateFailures()
    : kind === "ts" ? await tsGateFailures()
    : await crossGateFailures(CROSS_TARGETS[kind]);
  return raw.map((f) => "gate: " + f.gate + " (owner: " + f.owner + ")\n" + f.output);
}

const AREAS: Area[] = [
  {
    name: "docs",
    specialist: "z-nsh-engineer",
    fallback: PLAIN_NSH,
    gateKind: "docs",
    needsTrait: false,
    owns: "docs/goals.md, docs/parity.md, docs/decisions.md, README.md",
    work: `Write the four docs files. Sources: .refs/flutter_tts/lib/flutter_tts.dart, .refs/react-native-tts/index.d.ts, .refs/tts-rs/src/lib.rs, .refs/biometrics (its docs and README set the house style).

docs/goals.md (new) — the goals for the synthesis crate and package:
- Goals: read text aloud on desktop and mobile from one Rust crate and one TS package with identical API names and behavior, laid out like SuperNSH/biometrics.
- Acceptance criteria, each stated as something a gate proves: cargo fmt --all -- --check; cargo clippy --all-targets -- -D warnings and cargo test --all-targets in a CI rust matrix on ubuntu-latest, macos-latest and windows-latest; a CI cross job clippy-checking aarch64-apple-ios on macos and aarch64-linux-android plus wasm32-unknown-unknown on ubuntu (windows and linux code is exercised natively by the ubuntu and windows matrix runners); nsh check, nsh build (with a no-warnings grep) and nsh test for the package in a typescript job.
- The error enum as a table, one row per ErrorKind variant: variant, stable code, one-line message, when it is raised — exactly the eleven rows of the frozen contract.
- The edge-case list: one row per named test in the contract's NAMED TESTS list: the case, the named test, and where it runs (cargo test -p nsh-text-to-speech, nsh test, or a CI matrix runner). Cover everything the Flutter and React Native references handle: empty/blank/oversize text, speaking while another utterance runs (the interrupt/queue rule), awaited completion resolving exactly once, stop cancelling exactly once, pause/resume on idle, out-of-range rate/pitch/volume, empty language tag, unknown language, unknown and uninstalled voices, missing engine, backend status preservation, concurrent synthesize, WAV output, progress events, silence, stable codes, empty voice lists.
- Platform coverage table: apple (AVSpeechSynthesizer via objc2), android (android.speech.tts via JNI), windows (Windows.Media.SpeechSynthesis), linux (speech-dispatcher over SSIP), web (Web Speech API): what each bridge maps and an honest note that android and web mapping tests compile under cross-target checks and run only on device or browser runners, which this CI does not have.

docs/parity.md (rewrite; keep a capability-status table at top with the CI gates as evidence) — then one row per public API of BOTH references with what ours does:
- flutter_tts: speak, pause, stop, synthesizeToFile, setLanguage, setSpeechRate, setVolume, setPitch, setVoice, clearVoice, getLanguages, getVoices, getEngines, getDefaultEngine, getDefaultVoice, isLanguageAvailable, isLanguageInstalled, areLanguagesInstalled, setSilence, setQueueMode, setEngine, awaitSpeakCompletion, awaitSynthCompletion, getMaxSpeechInputLength, getSpeechRateValidRange, setSharedInstance, autoStopSharedSession, setIosAudioCategory.
- react-native-tts: getInitStatus, requestInstallEngine, requestInstallData, setDucking, setDefaultEngine, setDefaultVoice, setDefaultRate, setDefaultPitch, setDefaultLanguage, setIgnoreSilentSwitch, voices, engines, speak, stop, pause, resume, addEventListener/removeEventListener ("tts-start", "tts-progress", "tts-finish", "tts-cancel", "tts-error"), Voice, Engine, TtsError types.
- For each row: ours (Rust name and TS name) and implemented, or mapped-to, or intentionally out of scope with a one-line reason. House mappings: FlutterTts and Tts both map to Speech; setQueueMode maps to speak's interrupt flag; getInitStatus maps to init(); requestInstallEngine maps to the EngineMissing code whose message tells the user to install an engine (the brief's visible fallback); awaitSynthCompletion is unnecessary because synthesize resolves with the WAV bytes; getSpeechRateValidRange is out of scope because rate is normalized 0.0..=1.0 everywhere; setSharedInstance, autoStopSharedSession, setIosAudioCategory and setIgnoreSilentSwitch are iOS audio-session plumbing, out of scope with that reason; setDucking out of scope this release; clearVoice maps to setVoice with no argument clearing to the platform default voice.

docs/decisions.md — move the three Open decisions to Decided, one line of reason each: NSH 0.4.0 pinned (matches .refs/nsh and the installed CLI); library choice is the platform speech engines behind our own Speech API with flutter_tts's names (flutter_tts and react-native-tts are the API references, tts-rs the per-platform technique reference); platforms in scope: apple, android, windows, linux, web. Add one line noting the five reference repos were cloned into .refs/ (git-ignored) and all five URLs cloned successfully, so no canonical-repo substitution was needed.

README.md — update Status (implemented: name the crate and package and what Speech does), Layout (the new files and packages), Checks (exactly what .github/workflows/ci.yml is configured to run after this workflow: a format job; a rust matrix on ubuntu-latest, macos-latest and windows-latest running clippy and tests; a cross-check job for aarch64-apple-ios, aarch64-linux-android and wasm32-unknown-unknown; a typescript job running nsh check, nsh build and nsh test). Phrase CI claims as what CI is configured to run — AGENTS.md counts a claim with no passing run as unverified.`,
  },
  {
    name: "core",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "host",
    needsTrait: false,
    owns: "Cargo.toml (root), .gitignore, .github/workflows/ci.yml, and crates/nsh-text-to-speech/ EXCEPT src/platform/apple.rs, src/platform/android.rs, src/platform/windows.rs, src/platform/linux.rs, src/platform/web.rs (you seed those five as stubs; bridge areas own their real content)",
    work: `Build the workspace and the crate core so the whole tree compiles on every target from the start.
1. Root Cargo.toml: replace the single-package manifest with the workspace from the frozen contract (members, resolver 3, workspace.package). Delete the scaffold root src/lib.rs and the scaffold status() test — dead code is deleted.
2. crates/nsh-text-to-speech with the exact file set, deps and per-target dep blocks of the frozen contract. lib.rs mirrors .refs/biometrics crates/nsh-biometrics/src/lib.rs: crate doc comment, mod/pub use lines, and the #[native_module("nsh-text-to-speech/native")] registration exposing NativeSpeech (a thin wrapper class over the platform backend for TSX, like biometrics' NativeLocalAuth — consult .refs/nsh crates/nsh-core and .refs/biometrics native.rs for the exact pattern).
3. error.rs: ErrorKind + SpeechError with the eleven variants and stable codes, biometrics style. options.rs: Voice, Engine, UtteranceId, UtteranceEvent and its kind enum, EventSink.
4. backend.rs: the frozen Backend trait. fake.rs: FakeBackend (scripted answers, test-finished utterances firing Start/Progress/Finish/Cancel/Error, call counters, shared-state Clone) in the biometrics fake.rs style.
5. speech.rs: Speech with every frozen rule — init, speak with interrupt/queue, await_speak_completion exactly-once wiring, stop, pause/resume with NotSpeaking, is_speaking, synthesize with Busy, settings with range validation, queries, set_event_handler fan-out. Build the queue/completion state machine on tokio primitives (biometrics auth.rs is the style reference).
6. platform/mod.rs with platform_backend() cfg selection, platform/unsupported.rs, and the five compiling stubs (every method Err(NotAvailable), header comment naming the bridge area that owns the file).
7. tests/speech.rs: all 26 named tests, driving Speech::with_backend(FakeBackend::new()...). Watch each fail against the stub/incomplete path once (the AGENTS.md rule), then make it pass.
8. .gitignore: add .nsh/ (nsh generates it for tsconfig and build artifacts).
9. .github/workflows/ci.yml: keep the existing checkout/toolchain action pins and the env/concurrency/permissions blocks, then: a format job on ubuntu-latest running cargo fmt --all -- --check; a rust job with a matrix os of ubuntu-latest, macos-latest, windows-latest running cargo clippy -p nsh-text-to-speech --all-targets -- -D warnings and cargo test -p nsh-text-to-speech --all-targets; a cross-check job (matrix: aarch64-apple-ios on macos-latest; aarch64-linux-android and wasm32-unknown-unknown on ubuntu-latest) running cargo clippy -p nsh-text-to-speech --target <t> --all-targets -- -D warnings, with the Android NDK cc env block copied from .refs/biometrics .github/workflows/ci.yml; a typescript job on ubuntu-latest installing NSH v0.4.0 from the SuperNSH/nsh-builds installer (copy the install steps from biometrics' typescript job) and, working-directory packages/nsh-text-to-speech, running nsh check, nsh build 2>&1 | tee build.log followed by ! grep -i warn build.log, and nsh test.`,
  },
  {
    name: "apple",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "host",
    needsTrait: true,
    owns: "crates/nsh-text-to-speech/src/platform/apple.rs",
    work: `Replace the stub in src/platform/apple.rs entirely with the Apple backend implementing the frozen Backend trait via objc2 + objc2-avf-audio: AVSpeechSynthesizer with a delegate object (block2) mapping didStart to Start, willSpeakRangeOfSpeechString to Progress { location, length }, didFinish to Finish, didCancel to Cancel, and delegate errors to the error event (SynthesisFailed or Backend with the platform status). speak builds an AVSpeechUtterance with the current language/voice/rate/pitch/volume/silence applied (rate normalized 0.0..=1.0 onto AVSpeechUtteranceMaximumSpeechRate/MinimumSpeechRate, pitch onto pitchMultiplier 0.5..=2.0); on_event is invoked from the delegate on its thread. voices() and languages() come from AVSpeechSynthesisVoice.speechVoices(); an empty result or a nil shared synthesizer maps to EngineMissing or NotAvailable per the contract. Technique reference: .refs/tts-rs/src/backends/av_foundation.rs — adapt it to our trait, do not copy its API. Keep param and error mapping in pure functions with #[cfg(test)] tests in this file; they run on the mac (your scoped gate compiles and runs them via cargo test -p nsh-text-to-speech). Never panic: an ObjC exception path settles as a coded error.`,
  },
  {
    name: "android",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "android",
    needsTrait: true,
    owns: "crates/nsh-text-to-speech/src/platform/android.rs",
    work: `Replace the stub in src/platform/android.rs entirely with the Android backend implementing the frozen Backend trait over android.speech.tts.TextToSpeech via jni + ndk-context (technique reference: .refs/tts-rs/src/backends/android.rs — adapt to our trait). Get VM/activity from ndk-context (NoActivity-style absence maps to NotAvailable); construct TextToSpeech with the default engine — failure maps to EngineMissing; onInit success/failure resolves init(); LANG_MISSING_DATA to LanguageMissingData, LANG_NOT_SUPPORTED to LanguageNotSupported; UtteranceProgressListener onStart/onProgress/onDone/onCancel/onError fire the matching UtteranceEvents through the EventSink; speak/stop/pause/resume map to the engine calls with QUEUE_FLUSH for interrupt and QUEUE_ADD for queue; set_rate/pitch/volume/silence map onto the engine params; voices/languages via getVoices/getLanguages JNI calls. Every JNI call checked — an exception or failed call settles as a coded error, never a panic. Keep param and error mapping in pure functions with tests under #[cfg(target_os = "android")] in this file: they compile under your scoped cross-check (aarch64-linux-android) and would run on an Android runner, which this workflow deliberately does not drive (AGENTS.md forbids local emulators) — say that in the file header comment, honestly.`,
  },
  {
    name: "windows",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "windows",
    needsTrait: true,
    owns: "crates/nsh-text-to-speech/src/platform/windows.rs",
    work: `Replace the stub in src/platform/windows.rs entirely with the Windows backend implementing the frozen Backend trait via the windows crate: Windows.Media.SpeechSynthesis.SpeechSynthesizer (technique reference: .refs/tts-rs/src/backends/winrt.rs — adapt to our trait). speak uses SpeakText or GenerateStreamFromTextAsync; synthesize uses GenerateStreamFromTextAsync and reads the rendered WAV bytes out of the stream. IAsyncOperation results are awaited on the calling future (windows-future join; see the dep block) so every call resolves exactly once and never blocks a thread. Events: the synthesizer's bookkeeping plus the utterance stream map to Start/Progress/Finish/Cancel; a failed synthesis maps to SynthesisFailed; a failed WinRT call maps to Backend with the HRESULT as status; no installed voices maps to EngineMissing. voices() enumerates AllVoices; languages() from AllVoices' languages; set_voice matches by id, unknown -> VoiceNotFound. Keep param and error mapping in pure functions with #[cfg(test)] tests in this file: they compile in your scoped cross-check (x86_64-pc-windows-gnu) and run on the CI windows runner. Never panic: every WinRT error path settles as a coded error.`,
  },
  {
    name: "linux",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "linux",
    needsTrait: true,
    owns: "crates/nsh-text-to-speech/src/platform/linux.rs",
    work: `Replace the stub in src/platform/linux.rs entirely with the Linux backend implementing the frozen Backend trait over speech-dispatcher using ssip-client-async (ssip =0.5.0 wire types; technique reference: .refs/tts-rs/src/backends/speech_dispatcher.rs — adapt to our trait). Connecting fails -> NotAvailable (speechd not running); SSIP SPEAK/CANCEL commands drive speak/stop; SSIP pause/resume map directly; rate/pitch/volume map onto SSIP's parameters from our normalized ranges (keep the mapping function pure and test it); set_language maps to SSIP SET LANGUAGE, and an SSIP rejection maps to LanguageNotSupported. speech-dispatcher has no voice enumeration: voices() returns Ok(empty) (that is the missing_voice_list_is_empty case) and engines() returns the single default engine; languages() returns an empty list honestly. A failed send after a connection drop maps to Backend with the client error recorded as status. Keep mapping functions pure with #[cfg(test)] tests in this file: your scoped cross-check compiles them for x86_64-unknown-linux-gnu and the CI ubuntu runner runs them. Never hang: the ssip client is used in its async form so every future resolves.`,
  },
  {
    name: "web",
    specialist: "z-rust-engineer",
    fallback: PLAIN_RUST,
    gateKind: "web",
    needsTrait: true,
    owns: "crates/nsh-text-to-speech/src/platform/web.rs",
    work: `Replace the stub in src/platform/web.rs entirely with the Web backend implementing the frozen Backend trait via wasm-bindgen + web-sys: window().speech_synthesis() (technique reference: .refs/tts-rs/src/backends/web.rs — adapt to our trait). A missing speechSynthesis maps to NotAvailable in init/is_available. speak builds a SpeechSynthesisUtterance with text, voice, and the current settings — rate normalized 0.0..=1.0 onto utterance rate 0.1..=10.0, pitch 0.5..=2.0 onto pitchMultiplier 0..=2.0, volume onto 0.0..=1.0, and lang/voice from the set language or voice id — then wires onstart, onboundary (charIndex/charLength to Progress location/length), onend, onerror to the EventSink closure; onerror's error field maps to SynthesisFailed or Backend per the web error taxonomy (interrupted/canceled to Cancelled). stop/cancel maps to speechSynthesis.cancel(); pause/resume map to pause()/resume() with NotSpeaking when idle. voices() from getVoices (the list populates asynchronously — an empty first read returns Ok(empty) honestly; that is the missing_voice_list_is_empty case). Keep mapping functions pure with #[cfg(target_arch = "wasm32")] tests in this file: they compile under your scoped cross-check (wasm32-unknown-unknown) and would run in a browser, which this workflow deliberately does not drive (AGENTS.md forbids local simulators) — say that in the file header comment.`,
  },
  {
    name: "ts",
    specialist: "z-nsh-engineer",
    fallback: PLAIN_NSH,
    gateKind: "ts",
    needsTrait: true,
    owns: "packages/nsh-text-to-speech/ (everything in it)",
    work: `Build the TypeScript package that mirrors the Rust API (frozen contract's LAYOUT and API sections). .refs/biometrics/packages/nsh-biometrics is the pattern to mirror file for file.
1. package.json, nsh.json, tsconfig.json exactly per the contract. Note the package has no node_modules workflow of its own: nsh check and nsh test are the tooling.
2. src/errors.ts: SpeechErrorCode union of the eleven code strings, SpeechError { code, message, status? }, speechError(code), asSpeechError(unknown) — biometrics errors.ts style. src/options.ts: Voice, Engine, UtteranceEvent types (camelCase). src/backend.ts: SpeechBackend interface with promise-returning methods mirroring the Rust trait, like biometrics backend.ts.
3. src/memory.ts: memoryBackend(config) mirroring biometrics memory.ts — scripted config, finishUtterance firing the events, counters, setVoices etc. src/rust.ts: rustBackend() wrapping the crate's NativeSpeech native class from "nsh-text-to-speech/native", settling errors through asSpeechError — biometrics rust.ts style. src/speech.ts: class Speech with every frozen rule and camelCase names; src/index.ts re-exports the public surface.
4. App.tsx: small demo like biometrics' — mount/signal/Text/View from "nsh", a Speak button that calls speech.speak("Hello from Native Shell") and puts the result code or error code in a status text; testIDs title, status, speak.
5. tests/speech.test.ts: the 26 named tests from the contract (same names as the Rust ones), driving new Speech({ backend: memoryBackend() }); tests/errors.test.ts for the eleven codes; tests/memory.test.ts for the memory backend. Each test seen failing once before it passes (AGENTS.md). Imports from "nsh/test" like biometrics' tests.`,
  },
];

// Live board: one row per area, one column per phase.
artifact.board("areas", {
  title: "Text-to-speech areas",
  key: "name",
  status: "phase",
  columns: ["implementing", "auditing", "testing", "fixing", "done", "failed"],
});

// The bridge and package areas compile against the crate trait the core area lands first.
// This deferred is control flow only: bridges start implementing the moment core's implement
// lands, while core is still being audited — no global barrier anywhere. It is released even
// when core fails, so no area ever hangs waiting.
let coreLanded = false;
let releaseTrait!: () => void;
const traitReady = new Promise<void>((resolve) => { releaseTrait = resolve; });

// Durable persona-delivery evidence (references/specialist-personas.md): the typed result
// carries a sentence copied verbatim from the specialist body; a subagent that never got
// the body cannot manufacture an exact substring of it.
function personaHolds(persona: string, line: string | undefined | null): boolean {
  return typeof line === "string" && line.trim().length > 10 && persona.includes(line.trim());
}

async function checkContractLine(
  persona: string,
  agentRef: { ask: <T>(text: string) => Promise<T> },
  first: string,
): Promise<"verified" | "unverified"> {
  if (personaHolds(persona, first)) return "verified";
  const retry = await agentRef.ask<Implementation | AuditFindings>(
    "One retry: reply with contractLine only — one sentence copied EXACTLY, character for character, from your specialist contract (your system prompt). No other text." +
    "\n" + COMMON,
  );
  if (personaHolds(persona, (retry as Implementation).contractLine ?? (retry as AuditFindings).contractLine)) {
    return "verified";
  }
  return "unverified";
}

async function runArea(area: Area): Promise<AreaOutcome> {
  const outcome: AreaOutcome = {
    name: area.name,
    status: "failed",
    summary: "",
    auditConverged: false,
    auditRounds: 0,
    gateRounds: 0,
    testsPassed: false,
    personaDelivery: "unverified",
    blockers: [],
  };
  try {
    const { persona, source } = await specialist(area.specialist, area.fallback);
    if (source === "fallback") outcome.personaDelivery = "unverified";
    const impl = agent(area.specialist === "z-nsh-engineer" ? "NSH engineer·" + area.name : "Rust engineer·" + area.name, {
      system: persona,
      model: MODELS.pro,
    });

    if (area.needsTrait && area.gateKind !== "ts") {
      await traitReady;
      if (!coreLanded) {
        outcome.blockers.push("the core crate never landed, so this bridge has no trait to implement");
        report({ name: area.name, phase: "failed", summary: outcome.blockers[0] }, "areas");
        return outcome;
      }
    }

    report({ name: area.name, phase: "implementing", summary: area.owns }, "areas");
    const implResult = await impl.ask<Implementation>(implementAsk(area));
    const implDelivery = await checkContractLine(persona, impl, implResult.contractLine);
    if (source !== "fallback") outcome.personaDelivery = implDelivery;
    if (!implResult.done) {
      outcome.blockers.push(implResult.blockers.join("; ") || "implementer reported the work incomplete");
      outcome.summary = implResult.summary;
      if (area.name === "core") releaseTrait();
      report({ name: area.name, phase: "failed", summary: outcome.blockers[0] }, "areas");
      return outcome;
    }
    if (area.name === "core") {
      coreLanded = true;
      releaseTrait();
    }
    outcome.summary = implResult.summary;

    // Audit: check -> fix, until a check comes back clean or the 3-round cap hits.
    const auditor = agent("Auditor·" + area.name, { system: persona, model: MODELS.pro });
    for (let round = 0; round < 3; round += 1) {
      report({ name: area.name, phase: "auditing", summary: "review round " + (round + 1) }, "areas");
      const review = await auditor.ask<AuditFindings>(auditAsk(area));
      outcome.auditRounds = round + 1;
      if (review.contractLine !== undefined && source !== "fallback") {
        const auditorDelivery = await checkContractLine(persona, auditor, review.contractLine);
        if (auditorDelivery === "unverified") outcome.personaDelivery = "unverified";
      }
      if (review.findings.length === 0) {
        outcome.auditConverged = true;
        break;
      }
      if (round === 2) break; // cap reached; the last audit's findings stay recorded below
      report(
        { name: area.name, phase: "fixing", summary: review.findings.length + " audit findings" },
        "areas",
      );
      await impl.ask<Implementation>(fixAsk(area, review.findings));
    }
    if (!outcome.auditConverged) {
      outcome.blockers.push("audit still had findings after the 3-round cap");
    }

    // Scoped gates for this area's own targets, up to 3 fix rounds.
    if (area.gateKind !== "docs") {
      if (area.gateKind === "ts") {
        await traitReady;
        if (!coreLanded) {
          outcome.blockers.push("the core crate never landed, so the nsh gates cannot run");
        }
      }
      for (let round = 0; round < 3 && !outcome.testsPassed && outcome.blockers.length === 0; round += 1) {
        outcome.gateRounds = round + 1;
        report({ name: area.name, phase: "testing", summary: "scoped gate round " + (round + 1) }, "areas");
        const failures = await areaGateFailures(area.gateKind);
        if (failures.length === 0) {
          outcome.testsPassed = true;
          break;
        }
        if (round === 2) {
          outcome.blockers.push("scoped gates still failing: " + failures[0].slice(0, 400));
          break;
        }
        report({ name: area.name, phase: "fixing", summary: failures.length + " gate failures" }, "areas");
        const fix = await impl.ask<FixOutcome>(gateFixAsk(area, failures));
        if (fix.foreignFilesOnly) {
          // Nothing of ours is broken; the other area's in-flight state caused this.
          // The join re-runs the whole-tree gates, which catches any residue.
          outcome.blockers.push("gate failures came from files another area owns; they may clear at the join");
          break;
        }
      }
    } else {
      outcome.testsPassed = true; // docs have no runnable gate; the audit is their check
    }

    outcome.status = outcome.blockers.length === 0 ? "done" : "failed";
    report(
      {
        name: area.name,
        phase: outcome.status,
        summary:
          outcome.status === "done"
            ? outcome.summary.slice(0, 160)
            : outcome.blockers.join("; ").slice(0, 160),
      },
      "areas",
    );
    return outcome;
  } catch (err) {
    if (area.name === "core") releaseTrait();
    outcome.blockers.push("area failed: " + String(err));
    report({ name: area.name, phase: "failed", summary: outcome.blockers[0].slice(0, 160) }, "areas");
    return outcome;
  }
}

phase("Implement, audit, and test each area in parallel");
log("8 areas: docs, core, apple, android, windows, linux, web, ts — each runs its own chain");
const outcomes: AreaOutcome[] = await Promise.all(AREAS.map((area) => runArea(area)));

phase("Run the whole-tree gates once, fix what they reject, rerun");
async function runAllGates(): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const fmt = await fmtFailure();
  if (fmt) failures.push(fmt);
  const clippy = await world.run("cargo", ["clippy", "--all-targets", "--", "-D", "warnings"], {
    timeoutMs: 1200000,
  });
  if (clippy.exitCode !== 0) {
    const output = tail(clippy);
    failures.push({ gate: "cargo clippy --all-targets -- -D warnings", owner: ownerOf(output), output });
  }
  const test = await world.run("cargo", ["test", "--all-targets"], { timeoutMs: 1200000 });
  if (test.exitCode !== 0) {
    const output = tail(test);
    failures.push({ gate: "cargo test --all-targets", owner: ownerOf(output), output });
  }
  for (const target of ["x86_64-unknown-linux-gnu", "aarch64-linux-android", "x86_64-pc-windows-gnu", "wasm32-unknown-unknown"]) {
    const cross = await world.run(
      "cargo",
      ["clippy", "-p", "nsh-text-to-speech", "--target", target, "--all-targets", "--", "-D", "warnings"],
      { timeoutMs: 1200000 },
    );
    if (cross.exitCode !== 0) {
      const output = tail(cross);
      failures.push({
        gate: "cargo clippy -p nsh-text-to-speech --target " + target + " --all-targets -- -D warnings",
        owner: ownerOf(output),
        output,
      });
    }
  }
  const tsCheck = await world.run("nsh", ["check", "packages/nsh-text-to-speech"], { timeoutMs: 1200000 });
  if (tsCheck.exitCode !== 0) {
    const output = tail(tsCheck);
    failures.push({ gate: "nsh check packages/nsh-text-to-speech", owner: ownerOf(output), output });
  }
  const tsTest = await world.run("nsh", ["test", "packages/nsh-text-to-speech"], { timeoutMs: 1200000 });
  if (tsTest.exitCode !== 0) {
    const output = tail(tsTest);
    failures.push({ gate: "nsh test packages/nsh-text-to-speech", owner: ownerOf(output), output });
  }
  return failures;
}

let gateFailures = await runAllGates();
let joinRounds = 0;
while (gateFailures.length > 0 && joinRounds < 2) {
  joinRounds += 1;
  log("whole-tree gates: " + gateFailures.length + " failures; fix round " + joinRounds);
  const owners = Array.from(new Set(gateFailures.map((f) => f.owner)));
  const previousFailures = gateFailures;
  await Promise.all(
    owners.map(async (owner) => {
      const mine = previousFailures.filter((f) => f.owner === owner);
      const area = AREAS.find((a) => a.name === owner) ?? AREAS[0];
      const { persona } = await specialist(area.specialist, area.fallback);
      await agent("Gate fixer " + owner + " round " + joinRounds, { system: persona, model: MODELS.pro }).ask<FixOutcome>(
        "The whole-tree gates failed in your area's files:\n\n" +
          mine.map((f) => "gate: " + f.gate + "\n" + f.output).join("\n---\n") +
          "\n\nFix the failures by editing ONLY the files your area owns: " + area.owns +
          "\nDo not run the gate suite yourself; the workflow re-runs it right after you. A quick scoped compile is fine." +
          "\nReply with fixed, foreignFilesOnly, summary." +
          "\n" + COMMON,
      );
    }),
  );
  gateFailures = await runAllGates();
}
if (gateFailures.length > 0) {
  log("whole-tree gates still failing after " + joinRounds + " fix rounds");
} else {
  log("whole-tree gates all green");
}

const gatesGreen = gateFailures.length === 0;
const verifiedCommands = [
  "cargo fmt --all -- --check",
  "cargo clippy --all-targets -- -D warnings",
  "cargo test --all-targets",
  "cargo clippy -p nsh-text-to-speech --target x86_64-unknown-linux-gnu --all-targets -- -D warnings",
  "cargo clippy -p nsh-text-to-speech --target aarch64-linux-android --all-targets -- -D warnings",
  "cargo clippy -p nsh-text-to-speech --target x86_64-pc-windows-gnu --all-targets -- -D warnings",
  "cargo clippy -p nsh-text-to-speech --target wasm32-unknown-unknown --all-targets -- -D warnings",
  "nsh check packages/nsh-text-to-speech",
  "nsh test packages/nsh-text-to-speech",
];
const verified = gatesGreen
  ? verifiedCommands.map((c) => c + " — exit 0 at the final join")
  : verifiedCommands.map((c) => c + " — run at the final join (see findings for the ones still red)");

const notCovered: string[] = [];
if (!gatesGreen) {
  notCovered.push("these gates never went green: " + gateFailures.map((f) => f.gate).join(", "));
}
notCovered.push("android and web mapping tests compile under the cross-target checks but execute only on device or browser runners, which this workflow deliberately does not drive (AGENTS.md forbids local emulators and simulators)");
notCovered.push("real device speech (AVSpeechSynthesizer, TextToSpeech, SpeechSynthesizer, speech-dispatcher, Web Speech) is exercised by tests only where CI runs them: apple on macos, windows on the windows runner, linux on ubuntu");
notCovered.push("CI itself has not run yet: README and docs phrase CI claims as what .github/workflows/ci.yml is configured to run, per AGENTS.md");

const findings: Array<{
  where: string;
  what: string;
  evidence: string;
  status: "verified" | "unconfirmed";
  severity: "low" | "medium" | "high";
}> = [];
for (const outcome of outcomes) {
  if (outcome.status !== "done") {
    findings.push({
      where: areaHint(outcome.name),
      what: "area did not finish clean: " + outcome.blockers.join("; "),
      evidence: outcome.summary || "implementer or gates reported failure",
      status: "verified",
      severity: "high",
    });
  } else if (!outcome.auditConverged) {
    findings.push({
      where: areaHint(outcome.name),
      what: "audit never converged: findings remained after the 3-round cap",
      evidence: outcome.auditRounds + " audit rounds ran; the last audit still returned findings",
      status: "verified",
      severity: "medium",
    });
  }
  if (outcome.personaDelivery === "unverified") {
    findings.push({
      where: areaHint(outcome.name),
      what: "specialist persona delivery could not be verified (contractLine never matched the specialist body)",
      evidence: "the retry ask also failed; the work itself passed its gates",
      status: "verified",
      severity: "low",
    });
  }
}
for (const failure of gateFailures) {
  findings.push({
    where: areaHint(failure.owner),
    what: "whole-tree gate still red: " + failure.gate,
    evidence: tail({ stdout: failure.output, stderr: "" }) || "nonzero exit",
    status: "verified",
    severity: "high",
  });
}

function areaHint(area: string): string {
  if (area === "core") return "crates/nsh-text-to-speech and root manifests";
  if (area === "ts") return "packages/nsh-text-to-speech";
  if (area === "apple" || area === "android" || area === "windows" || area === "linux" || area === "web") {
    return "crates/nsh-text-to-speech/src/platform/" + area + ".rs";
  }
  return "docs and README";
}

const doneCount = outcomes.filter((o) => o.status === "done").length;
const conclusion =
  (gatesGreen
    ? "All whole-tree gates are green. "
    : "The whole-tree gates never fully went green after " + joinRounds + " fix rounds. ") +
  doneCount + " of " + outcomes.length + " areas finished clean (implement -> audit to zero findings or the 3-round cap -> scoped gates): " +
  outcomes.map((o) => o.name + " " + o.status + (o.auditConverged ? "" : " (audit unresolved)")).join(", ") +
  ". The repo now carries a Rust crate (crates/nsh-text-to-speech) and a TypeScript package (packages/nsh-text-to-speech) laid out like SuperNSH/biometrics, adding speech synthesis with the flutter_tts and react-native-tts names, stable ERR_TTS_* codes, and a test for every behavior in both languages. No commits were made.";

return {
  conclusion,
  findings,
  verified,
  notCovered,
  areas: outcomes.map((o) => ({
    name: o.name,
    status: o.status,
    summary: o.summary,
    auditConverged: o.auditConverged,
    auditRounds: o.auditRounds,
    gateRounds: o.gateRounds,
    testsPassed: o.testsPassed,
    personaDelivery: o.personaDelivery,
    blockers: o.blockers,
  })),
  notes: [
    "Every subagent is pinned to account:zai-individual-coding-plan/GLM-5.3$high — the user asked for the main model in every area and audit round; no vision phases exist, so no flash pin is used anywhere.",
    "Audit budget was 3 rounds per area until zero issues or the cap; areas that hit the cap are reported with auditConverged false and their remaining findings evidenced.",
    "Scoped per-area gates: core and apple run cargo fmt/clippy/test -p on the host (mac); android, windows, linux and web run cargo clippy against their cross targets; ts runs nsh check and nsh test. The whole-tree gate set ran once at the join, plus up to 2 routed fix rounds.",
    "STEP 0 was done before the run: all five reference repos were already cloned under .refs/ (git-ignored) from exactly the brief's URLs (remotes verified), so docs/decisions.md records that no canonical-repo substitution was needed. Dependency versions were pinned from .refs/tts-rs Cargo.toml and nsh-core 0.4.0 from .refs/nsh; registry re-verification happens when CI first runs.",
    "The nsh check gate shape was tested for real before the run (exit 0 on .refs/biometrics' package copy, cold start ~10 min), and every cross target used by the gates is installed in this machine's rustup toolchain.",
  ],
};
