# Parity with `flutter_tts` and `react-native-tts`

The `flutter_tts` names come from `.refs/flutter_tts/lib/flutter_tts.dart` and the `react-native-tts` names from `.refs/react-native-tts/index.d.ts`, both read on 2026-11-09. Both libraries map to one type here: `FlutterTts` and `Tts` are `Speech` (`Speech` in Rust, the `Speech` class in TS). Rust test names are in `crates/nsh-text-to-speech/tests/speech.rs`; TypeScript tests are in `packages/nsh-text-to-speech/tests/`.

## Capability status

Each row names the CI gate that proves it, from the jobs this workflow configures in `.github/workflows/ci.yml`. A row is unverified until that gate has a green run on `master`.

| Capability | Gate that proves it | Status |
|---|---|---|
| Rust crate: clippy clean, suite green on Linux, macOS and Windows | rust matrix: `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets` | unverified |
| Apple, Android and wasm bridges compile | cross job: clippy for `aarch64-apple-ios`, `aarch64-linux-android`, `wasm32-unknown-unknown` | unverified |
| TS package: type-checks, builds with no warning, suite green | typescript job: `nsh check`, `nsh build`, `nsh test` | unverified |
| Workspace formatting | format job: `cargo fmt --all -- --check` | unverified |
| The speech behaviors in the tables below | the named tests, in `cargo test -p nsh-text-to-speech` and `nsh test` | unverified |
| Audio out of a real speaker | none: CI has no device or browser runner | not provable in this CI |

## Status words

- **Done**: implemented in both languages, with the named test. Whether CI has verified it is the table above.
- **Mapped**: the reference call has no counterpart here, but a named API covers the use; the row says which.
- **Out of scope**: intentionally not here; the row gives the one-line reason.

## `flutter_tts` methods

| `flutter_tts` | Rust | TypeScript | Status | Covered by |
|---|---|---|---|---|
| `speak(text)` | `Speech::speak(text, interrupt)` | `speech.speak(text, interrupt?)` | Done. The Android `focus` flag is not taken; NSH apps own audio focus | `speaks_and_finishes_once`; `speak_queues_while_speaking`; `speak_interrupts_current_utterance` |
| `pause()` | `Speech::pause()` | `speech.pause()` | Done | `pause_and_resume_round_trip` |
| `stop()` | `Speech::stop()` | `speech.stop()` | Done. The running utterance's awaited `speak` resolves `Cancelled` exactly once | `stop_cancels_awaited_speak_once` |
| `synthesizeToFile(text, fileName)` | `Speech::synthesize(text)` | `speech.synthesize(text)` | Mapped. Returns the WAV bytes; the app writes the file, so the library needs no path permission. Android and iOS only in flutter_tts; here it is every platform whose engine synthesizes | `synthesize_returns_wav_bytes`; `concurrent_synthesize_reports_busy` |
| `setLanguage(language)` | `set_language(tag)` | `setLanguage(tag)` | Done. Empty tag is `InvalidArgument` | `set_language_rejects_empty`; `unknown_language_reports_unavailable` |
| `setSpeechRate(rate)` | `set_speech_rate(rate)` | `setSpeechRate(rate)` | Done, 0.0..=1.0 | `set_rate_rejects_out_of_range` |
| `setVolume(volume)` | `set_volume(volume)` | `setVolume(volume)` | Done, 0.0..=1.0 | `set_volume_rejects_out_of_range` |
| `setPitch(pitch)` | `set_pitch(pitch)` | `setPitch(pitch)` | Done, 0.5..=2.0, 1.0 normal | `set_pitch_rejects_out_of_range` |
| `setVoice(voice)` | `set_voice(id)` | `setVoice(id)` | Done. Takes a voice id from `voices()`, not flutter_tts's name-plus-locale map | `set_unknown_voice_reports_voice_not_found`; `uninstalled_voice_reports_voice_not_found` |
| `clearVoice()` | none | none | Out of scope. `Speech` sets a voice by id and has no clear-to-default call; the platform's default voice is what speaks until `set_voice` picks one | none |
| `getLanguages` | `languages()` | `languages()` | Done | `unknown_language_reports_unavailable` |
| `getVoices` | `voices()` | `voices()` | Done. Empty list, not an error, when there are none | `missing_voice_list_is_empty` |
| `getEngines` | `engines()` | `engines()` | Done. Android-only in flutter_tts; other platforms answer an empty list | `missing_voice_list_is_empty` (the same empty-list rule) |
| `getDefaultEngine` | `engines()` | `engines()` | Mapped. The `Engine` whose `default` is true; one list call answers both | none beyond the list pass-through |
| `getDefaultVoice` | none | none | Out of scope. `Voice` carries no default flag, so not even `voices()` can name the default; the platform's default voice is what speaks until `set_voice` picks one | none |
| `isLanguageAvailable(language)` | `is_language_available(tag)` | `isLanguageAvailable(tag)` | Done | `unknown_language_reports_unavailable` |
| `isLanguageInstalled(language)` | `voices()` | `voices()` | Mapped. A language is installed when a voice for it has `installed: true`; `LanguageMissingData` is the code when the data needs downloading | `uninstalled_voice_reports_voice_not_found` |
| `areLanguagesInstalled(languages)` | `voices()` | `voices()` | Mapped. The app checks each tag against the same list; a batch helper would be sugar | none beyond the list pass-through |
| `setSilence(timems)` | `set_silence(ms)` | `setSilence(ms)` | Done. 0 starts the utterance immediately. Android-only in flutter_tts; here it reaches every backend that has the concept | `set_silence_reaches_backend` |
| `setQueueMode(queueMode)` | `speak`'s `interrupt` flag | `speak`'s interrupt argument | Mapped. QUEUE_FLUSH (0) is `interrupt: true`, the default; QUEUE_ADD (1) is `interrupt: false`. Per utterance, not modal state | `speak_interrupts_current_utterance`; `speak_queues_while_speaking` |
| `setEngine(engine)` | none | none | Out of scope. The platform's default engine is used; `engines()` reports what exists | `init_reports_missing_engine` |
| `awaitSpeakCompletion(awaitCompletion)` | `await_speak_completion(bool)` | `awaitSpeakCompletion(bool)` | Done. Default false; when on, `speak` resolves exactly once, on finish, cancel or error | `awaited_speak_resolves_once_on_finish` |
| `awaitSynthCompletion(awaitCompletion)` | none | none | Mapped away. `synthesize` resolves with the WAV bytes, so there is nothing to wait for separately | `synthesize_returns_wav_bytes` |
| `getMaxSpeechInputLength` | `max_speech_input_length()` | `maxSpeechInputLength()` | Done. `None` where the platform sets no limit | `speak_rejects_oversize_text` |
| `getSpeechRateValidRange` | none | none | Out of scope. Rate is normalized 0.0..=1.0 on every platform here, so there is no range to ask for | `set_rate_rejects_out_of_range` |
| `setSharedInstance(sharedSession)` | none | none | Out of scope. iOS audio-session plumbing | none |
| `autoStopSharedSession(autoStop)` | none | none | Out of scope. iOS audio-session plumbing | none |
| `setIosAudioCategory(category, options, mode)` | none | none | Out of scope. iOS audio-session plumbing | none |
| `setAudioAttributesForNavigation(isNavigation)` | none | none | Out of scope. Android audio-attributes plumbing, not speech synthesis behavior | none |
| `setStartHandler`, `setCompletionHandler`, `setPauseHandler`, `setContinueHandler`, `setCancelHandler`, `setProgressHandler`, `setErrorHandler` | `set_event_handler(handler)` | `addEventListener` / `removeEventListener` | Done. All callbacks fold into one `UtteranceEvent` stream; see the events table below | `progress_events_report_location`; `error_codes_are_stable_and_unique` |

## `react-native-tts` methods and types

| `react-native-tts` | Rust | TypeScript | Status | Covered by |
|---|---|---|---|---|
| `getInitStatus()` | `Speech::init()` | `speech.init()` | Done. `Ok(())` where flutter_tts's `getInitStatus` says `success`; `EngineMissing` or `NotAvailable` say why not | `init_reports_missing_engine` |
| `requestInstallEngine()` | none | none | Mapped. The `EngineMissing` code's message tells the user to install an engine; the app shows that fallback. The library cannot open the store | `init_reports_missing_engine` |
| `requestInstallData()` | none | none | Mapped. `LanguageMissingData` says the voice data needs downloading; the app acts on the code | `error_codes_are_stable_and_unique` |
| `setDucking(enabled)` | none | none | Out of scope this release | none |
| `setDefaultEngine(engineName)` | none | none | Out of scope. Same reason as flutter_tts `setEngine` above | none |
| `setDefaultVoice(voiceId)` | `set_voice(id)` | `setVoice(id)` | Done | `set_unknown_voice_reports_voice_not_found`; `uninstalled_voice_reports_voice_not_found` |
| `setDefaultRate(rate, skipTransform?)` | `set_speech_rate(rate)` | `setSpeechRate(rate)` | Mapped. Normalized 0.0..=1.0 like flutter_tts, not RN's platform scale; `skipTransform` has no counterpart because nothing is transformed | `set_rate_rejects_out_of_range` |
| `setDefaultPitch(pitch)` | `set_pitch(pitch)` | `setPitch(pitch)` | Done. Same 0.5..=2.0 scale, 1.0 normal | `set_pitch_rejects_out_of_range` |
| `setDefaultLanguage(language)` | `set_language(tag)` | `setLanguage(tag)` | Done | `set_language_rejects_empty` |
| `setIgnoreSilentSwitch(ignoreSilentSwitch)` | none | none | Out of scope. iOS audio-session plumbing | none |
| `voices()` | `voices()` | `voices()` | Done | `missing_voice_list_is_empty` |
| `engines()` | `engines()` | `engines()` | Done | `init_reports_missing_engine` (the same backend) |
| `speak(utterance, options?)` | `Speech::speak(text, interrupt)` | `speech.speak(text, interrupt?)` | Done. Returns an utterance id. The `options` payload (iOS voice id, rate, Android stream, pan) is not taken: voice, rate and volume go through their setters | `speaks_and_finishes_once` |
| `stop(onWordBoundary?)` | `Speech::stop()` | `speech.stop()` | Done. The flag is not taken; the platform stops where it stops | `stop_cancels_awaited_speak_once` |
| `pause(onWordBoundary?)` | `Speech::pause()` | `speech.pause()` | Done. Pause while idle is `NotSpeaking` | `pause_when_idle_reports_not_speaking` |
| `resume()` | `Speech::resume()` | `speech.resume()` | Done. Resume while not paused is `NotSpeaking` | `resume_when_not_paused_reports_not_speaking` |
| `addEventListener` / `removeEventListener` | `set_event_handler(handler)` | `addEventListener` / `removeEventListener` | Done. The same five event names; see the table below | `progress_events_report_location` |
| `Voice` type | `Voice { id, name, language, network_required, installed }` | `Voice { id, name, language, networkRequired, installed }` | Done. RN's `quality` and `latency` are not kept; `notInstalled` is kept inverted as `installed` | `memory.test.ts` |
| `Engine` type | `Engine { name, label, default }` | `Engine { name, label, default }` | Done. RN's `icon` is not kept | `memory.test.ts` |
| `TtsError` type | `SpeechError` | `SpeechError` | Done, as stable codes instead of strings; see the mapping below | `error_codes_are_stable_and_unique`; `errors.test.ts` |

## Events

One `UtteranceEvent { utterance_id, kind }` stream in Rust, the five react-native-tts event names in TS.

| `flutter_tts` callback | `react-native-tts` event | Ours |
|---|---|---|
| `speak.onStart` / `synth.onStart` | `tts-start` | `start` |
| `speak.onProgress` | `tts-progress` | `progress { location, length }` |
| `speak.onComplete` / `synth.onComplete` | `tts-finish` | `finish` |
| `speak.onCancel` | `tts-cancel` | `cancel` |
| `speak.onError` / `synth.onError` | `tts-error` | `error(SpeechError)` |
| `speak.onPause`, `speak.onContinue` | none | none. The `pause()` and `resume()` results answer it |

## `TtsError` codes

The direct ones carry their own code here; every other `TtsError` code (`error`, `not_ready`, `network_error`, `network_timeout`, `not_installed_yet`, `output_error`, `service_error`, `not_found`, `Android AudioManager error`) is `ERR_TTS_BACKEND`, with the platform's own status kept in `status`.

| `TtsError` code | Code here |
|---|---|
| `no_engine` | `ERR_TTS_ENGINE_MISSING` |
| `not_available` | `ERR_TTS_NOT_AVAILABLE` |
| `lang_missing_data` | `ERR_TTS_LANGUAGE_MISSING_DATA` |
| `lang_not_supported` | `ERR_TTS_LANGUAGE_NOT_SUPPORTED` |
| `invalid_request` | `ERR_TTS_INVALID_ARGUMENT` |
| `bad_rate` | `ERR_TTS_INVALID_ARGUMENT` |
| `synthesis_error` | `ERR_TTS_SYNTHESIS_FAILED` |
| everything else | `ERR_TTS_BACKEND`, status kept |

## Additions the references do not have

`synthesize` returning WAV bytes rather than writing a file, `ERR_TTS_BUSY` for a second concurrent synthesis, `ERR_TTS_NOT_SPEAKING` for pause and resume in the wrong state, and the `status` field that keeps a platform's own error code. [goals.md](goals.md) lists all eleven codes and every named test.
