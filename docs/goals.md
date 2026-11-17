# Goals

Read text aloud from one Rust crate and one TypeScript package, with the same API names and the same behavior in both.

## What this repo delivers

- `crates/nsh-text-to-speech`: one `Speech` type over the platform speech engines — `AVSpeechSynthesizer` on Apple, `android.speech.tts` over JNI on Android, `Windows.Media.SpeechSynthesis` on Windows, speech-dispatcher over SSIP on Linux, the Web Speech API on wasm.
- `packages/nsh-text-to-speech`: the same API as a TypeScript `Speech` class, plus a `memoryBackend` for tests. Rust keeps snake_case, TS camelCase; names and behavior are otherwise identical.
- The layout mirrors SuperNSH/biometrics: a workspace with `crates/` and `packages/`, a `FakeBackend` in Rust, a `memoryBackend` in TS, and the same test names in both languages.
- The names follow Flutter's `flutter_tts` (`FlutterTts` maps to `Speech`); react-native-tts supplies the event names. [parity.md](parity.md) records every mapping.

Every operation resolves exactly once — success, a coded failure, or a cancel — and never hangs. Errors carry stable `ERR_TTS_*` codes an app can match on.

## Acceptance criteria

Each criterion is stated as the gate that proves it. The gates are the jobs this workflow configures in `.github/workflows/ci.yml`; a criterion counts only after that gate has a green run on `master`.

- `cargo fmt --all -- --check` passes (the format job).
- `cargo clippy --all-targets -- -D warnings` and `cargo test --all-targets` pass on `ubuntu-latest`, `macos-latest` and `windows-latest` (the rust matrix job). This runs the linux bridge natively on ubuntu, the windows bridge natively on windows, and the apple bridge on macOS.
- Clippy with `-D warnings` passes for `aarch64-apple-ios` and `aarch64-apple-ios-sim` on a macOS runner, and for `aarch64-linux-android`, `wasm32-unknown-unknown` and `wasm32-wasip1` on an ubuntu runner (the cross job). Those targets are compile-checked only; no CI runner executes them. `wasm32-wasip1` is the unsupported target, and the cross job is where its not-available path and test compile.
- `nsh check`, `nsh build` and `nsh test` pass for `packages/nsh-text-to-speech` and for `examples/demo` (the typescript job), and the builds print no warning.
- Every test in the edge-case table below exists in both languages under the same name and passes in those gates.

## Errors

`ErrorKind` has exactly these eleven variants, each with a stable code. Rust carries them as `SpeechError { kind, status }`; TS as `SpeechError { code, message, status? }`. The platform's own status code rides along in `status` wherever it exists.

| Variant | Code | Message | Raised when |
|---|---|---|---|
| `NotAvailable` | `ERR_TTS_NOT_AVAILABLE` | Speech is not available on this device or platform. Offer a visible fallback. | No speech service on this device or platform |
| `EngineMissing` | `ERR_TTS_ENGINE_MISSING` | No text-to-speech engine is installed. Install one, then try again. | No TTS engine installed (Android: install one) |
| `VoiceNotFound` | `ERR_TTS_VOICE_NOT_FOUND` | That voice is unknown or not installed. Pick one from voices(). | The voice is unknown or not installed |
| `LanguageNotSupported` | `ERR_TTS_LANGUAGE_NOT_SUPPORTED` | The platform does not support that language tag. Pick one from languages(). | The platform rejected the language tag |
| `LanguageMissingData` | `ERR_TTS_LANGUAGE_MISSING_DATA` | The language's voice data needs downloading in the system settings. | Android `LANG_MISSING_DATA` |
| `NotSpeaking` | `ERR_TTS_NOT_SPEAKING` | Nothing is speaking, or nothing is paused. | Pause on an idle synthesizer, resume on an unpaused one |
| `InvalidArgument` | `ERR_TTS_INVALID_ARGUMENT` | An argument is empty or out of range, such as blank text or a rate past 1.0. | Empty or blank text, oversize text, empty language tag, rate outside 0.0..=1.0, pitch outside 0.5..=2.0, volume outside 0.0..=1.0 |
| `Busy` | `ERR_TTS_BUSY` | A synthesize is already running. Wait for its bytes. | A second concurrent `synthesize` |
| `Cancelled` | `ERR_TTS_CANCELLED` | The utterance was stopped before it finished. | The utterance was stopped before it finished |
| `SynthesisFailed` | `ERR_TTS_SYNTHESIS_FAILED` | The platform failed to synthesize the text. | Web `onerror`, a Windows synthesis fault |
| `Backend` | `ERR_TTS_BACKEND` | The platform's speech service failed. | The platform service failed any other way; carries its own status code |

## Edge cases

One row per named test in the frozen contract. *Both* means the test exists under that exact name in `cargo test -p nsh-text-to-speech` (Rust, on every rust-matrix runner) and in `nsh test` (TypeScript, in the typescript job). Rows that run somewhere else say so.

| Case | Named test | Runs |
|---|---|---|
| An utterance speaks, fires start and finish, and finishes exactly once | `speaks_and_finishes_once` | both |
| Empty text is refused | `speak_rejects_empty_text` | both |
| Whitespace-only text is refused | `speak_rejects_blank_text` | both |
| Text past the backend's `max_speech_input_length` is refused (Android's 4000-char limit is the reference case) | `speak_rejects_oversize_text` | both |
| `speak` with `interrupt: false` queues FIFO behind the running utterance (flutter_tts `setQueueMode(1)`, QUEUE_ADD) | `speak_queues_while_speaking` | both |
| `speak` with `interrupt: true` cancels the running utterance (QUEUE_FLUSH) and the new one starts | `speak_interrupts_current_utterance` | both |
| With `await_speak_completion` on, `speak` resolves exactly once, on finish | `awaited_speak_resolves_once_on_finish` | both |
| `stop` cancels the running utterance; its awaited `speak` resolves `Cancelled` exactly once | `stop_cancels_awaited_speak_once` | both |
| `stop` cancels the queued utterances too; a queued speak never starts and its awaited form resolves `Cancelled` | `stop_clears_the_queue` | both |
| Pause then resume round-trips | `pause_and_resume_round_trip` | both |
| Pause while idle reports `NotSpeaking` | `pause_when_idle_reports_not_speaking` | both |
| Resume while not paused reports `NotSpeaking` | `resume_when_not_paused_reports_not_speaking` | both |
| Rate outside 0.0..=1.0 is refused | `set_rate_rejects_out_of_range` | both |
| Pitch outside 0.5..=2.0 is refused | `set_pitch_rejects_out_of_range` | both |
| Volume outside 0.0..=1.0 is refused | `set_volume_rejects_out_of_range` | both |
| An empty language tag is refused | `set_language_rejects_empty` | both |
| A language the platform does not have reports unavailable | `unknown_language_reports_unavailable` | both |
| Language tags match case-insensitively, as BCP 47 has it | `language_matching_ignores_case` | both |
| `set_voice` with an id not in `voices()` reports `VoiceNotFound` | `set_unknown_voice_reports_voice_not_found` | both |
| `set_voice` with a voice whose `installed` is false reports `VoiceNotFound` (react-native-tts `notInstalled`) | `uninstalled_voice_reports_voice_not_found` | both |
| `set_voice` with a failed voice list reports the backend failure, not `VoiceNotFound` | `set_voice_propagates_a_failed_voice_list` | both |
| `init` on a device with no engine reports `EngineMissing` (react-native-tts `getInitStatus` failure) | `init_reports_missing_engine` | both |
| A backend failure keeps the platform's own status code | `backend_failure_carries_status` | both |
| A second concurrent `synthesize` reports `Busy` | `concurrent_synthesize_reports_busy` | both |
| `synthesize` returns WAV bytes, header included | `synthesize_returns_wav_bytes` | both |
| Progress events carry `location` and `length` (flutter_tts `onProgress`, react-native-tts `tts-progress`) | `progress_events_report_location` | both |
| `set_silence` reaches the backend; 0 starts the utterance immediately | `set_silence_reaches_backend` | both |
| The eleven codes are stable and pairwise distinct | `error_codes_are_stable_and_unique` | both, plus `errors.test.ts` (TS) asserting the eleven strings |
| `voices()` answers an empty list both when the platform has none and when the list call fails | `missing_voice_list_is_empty` | both |

The TS memory backend has its own suite in `packages/nsh-text-to-speech/tests/memory.test.ts`.

## Platforms

| Platform | Technique | The bridge maps | Where its tests run |
|---|---|---|---|
| Apple (iOS, macOS) | `AVSpeechSynthesizer` via objc2 | Utterance start/progress/finish/cancel delegate callbacks; `AVSpeechSynthesisVoice` list; rate, pitch, volume; silence as the pre-utterance delay; synthesis to a WAV buffer | Mapping tests run in the rust matrix on `macos-latest` |
| Android | `android.speech.tts.TextToSpeech` over JNI (`jni`, `ndk-context`) | QUEUE_FLUSH/QUEUE_ADD onto `speak`'s interrupt flag; engine list and default; `LANG_MISSING_DATA` and `LANG_NOT_SUPPORTED`; language availability from `setLanguage`'s result codes and the voices' locales; voices with their installed flag; `getMaxSpeechInputLength` | Mapping tests compile in the cross job (`aarch64-linux-android`) and run only on a device runner, which this CI does not have |
| Windows | `Windows.Media.SpeechSynthesis` (`SpeechSynthesizer`) | Voice list; rate, pitch, volume; the synthesis stream to WAV bytes; a synthesis fault to `SynthesisFailed` | Mapping tests run in the rust matrix on `windows-latest` |
| Linux | speech-dispatcher over SSIP (`ssip-client-async`) | Speak, stop, pause, resume; rate, pitch, volume; the server's language list; progress is start/finish/cancel only, because the bridge never requests SSIP index marks (word indices are not character locations, so they cannot fill `location`/`length` honestly) | Mapping tests run in the rust matrix on `ubuntu-latest` |
| Web (wasm) | Web Speech API (`speechSynthesis`) | Speak with queue or cancel; the voice list; `onerror` to `SynthesisFailed`; `onboundary` progress | Mapping tests compile in the cross job (`wasm32-unknown-unknown`) and run only in a browser runner, which this CI does not have |

Any other target gets the unsupported backend, and every call answers `ERR_TTS_NOT_AVAILABLE`.

Honest limits: the android and web mapping tests compile under the cross job and run only on device or browser runners, which this CI does not have. The unsupported-target check compiles for `wasm32-wasip1` and runs nowhere. No gate anywhere plays audio; what a speaker does is the platform's behavior and is not seen here. The demo app's builds are unsigned.
