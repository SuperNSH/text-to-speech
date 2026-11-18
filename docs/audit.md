# Audit

Every claim in this repo's docs, checked against the code and the CI runs, on 2026-11-17. Method: read the sources (`crates/nsh-text-to-speech/src/`, `packages/nsh-text-to-speech/src/`, both test suites, `.github/workflows/`), cross-check the reference names in `.refs/flutter_tts/lib/flutter_tts.dart` and `.refs/react-native-tts/index.d.ts`, and verify CI claims with `gh run list` / `gh run view`, `gh release list` and `curl -sI`. Nothing was built or run locally, per AGENTS.md.

Audited state: `master` at `46b3f95`. The working tree carries **uncommitted changes from the finishing work in progress**: `.github/workflows/ci.yml` is rewritten (adds kache, `LINUX_PACKAGES`, the nsh.dev installer, `aarch64-apple-ios-sim`, and `demo`/`publish`/`pages` jobs), `.github/workflows/pages.yml` and `site/index.html` are staged for deletion, and an `examples/demo` draft appeared in the working tree while this audit ran. None of that is on `master` or covered by a green run. Rows about CI state are judged against `master`; where the working tree differs, the row says so.

Status words: **Done** (code exists and a test exercises it), **Untested** (code exists, no test proves it), **Partial**, **Missing**, **Blocked** (needs a change outside this repo), **Stale** (doc wrong, code right).

## Summary

| Status | Count |
|---|---|
| Done | 125 |
| Stale | 15 |
| Partial | 3 |
| Untested | 1 |
| Missing | 0 |
| Blocked | 0 |

## Claims

| # | Claim (doc and line) | Where it should live in code | Evidence it works | Status |
|---|---|---|---|---|
| 1 | goals.md:7 — one `Speech` type over `AVSpeechSynthesizer`, `android.speech.tts` over JNI, `Windows.Media.SpeechSynthesis`, speech-dispatcher over SSIP, Web Speech API on wasm | `crates/nsh-text-to-speech/src/platform/{apple,android,windows,linux,web}.rs` + target dep blocks in the crate `Cargo.toml` | Each bridge file uses the stated engine (file headers, imports); mapping tests inside each file; run 44043490843 green | Done |
| 2 | goals.md:8 — "Rust keeps snake_case, TS camelCase; names and behavior are otherwise identical" | `crates/.../src/speech.rs` vs `packages/.../src/speech.ts` | Names match case-for-case across all 20 methods; but three behaviors differ (spelled out under "Row 2's gap" below the table) | Partial |
| 3 | goals.md:9 — layout mirrors biometrics: workspace with `crates/` and `packages/`, `FakeBackend`, `memoryBackend`, same test names in both | root `Cargo.toml`, `crates/.../src/fake.rs`, `packages/.../src/memory.ts` | All present; 26/26 test names identical in `tests/speech.rs` and `tests/speech.test.ts` (checked by name, both files) | Done |
| 4 | goals.md:10 — names follow `flutter_tts`; react-native-tts supplies the event names; parity.md records every mapping | `docs/parity.md` vs `.refs/flutter_tts/lib/flutter_tts.dart`, `.refs/react-native-tts/index.d.ts` | Every reference name in parity.md found in the reference sources (rows 57–128) | Done |
| 5 | goals.md:12 — "Every operation resolves exactly once — success, a coded failure, or a cancel — and never hangs" | `speech.rs` (oneshot per speak, `SynthGate` drop guard), `speech.ts` (`settled` flags), `rust.ts` (early-event tripwires) | `awaited_speak_resolves_once_on_finish`; `stop_cancels_awaited_speak_once`; `an_awaited_speak_settles_when_the_backend_loses_the_utterance`; `a_second_terminal_event_settles_nothing_but_still_reaches_the_handler`; `cancelled_synthesize_frees_the_gate`; `waiting_sink_expires_on_silence`; `rust.test.ts` | Done |
| 6 | goals.md:18 — `cargo fmt --all -- --check` passes (the format job) | `.github/workflows/ci.yml` format job | Job exists; run 44043490843 green (`gh run view 44043490843`) | Done |
| 7 | goals.md:19 — clippy `-D warnings` and `cargo test --all-targets` pass on ubuntu/macos/windows (the rust matrix job) | ci.yml rust job | Job matrix is those three OSes; green in 44043490843. Commands carry a `-p nsh-text-to-speech` prefix — same package set, this workspace has one member | Done |
| 8 | goals.md:20 — clippy for `aarch64-apple-ios` (macOS), `aarch64-linux-android` and `wasm32-unknown-unknown` (ubuntu), compile-checked only | ci.yml cross-check job at `master` | Exactly those three targets; green in 44043490843 | Done |
| 9 | goals.md:21 — `nsh check`, `nsh build` (no warning) and `nsh test` pass (the typescript job) | ci.yml typescript job | `nsh build 2>&1 | tee build.log; ! grep -i warn build.log`; green in 44043490843 | Done |
| 10 | goals.md:22 — every edge-case test exists in both languages under the same name and passes in those gates | `crates/.../tests/speech.rs`, `packages/.../tests/speech.test.ts` | All 26 names present exactly once in each file (rows 23–48); suites green in 44043490843 | Done |
| 11 | goals.md:26 — eleven `ErrorKind` variants; Rust `SpeechError { kind, status }`, TS `SpeechError { code, message, status? }` | `crates/.../src/error.rs`, `packages/.../src/errors.ts` | Eleven variants in both; `error_codes_are_stable_and_unique` (both) + `errors.test.ts` | Done |
| 12 | goals.md:30 — `NotAvailable` / `ERR_TTS_NOT_AVAILABLE` / "Speech synthesis is not available on this device" | error.rs:8,79; errors.ts:3,17 | Code and raised-when correct (unsupported target, speechd absent, web without speechSynthesis). Message differs: the code says "Speech is not available on this device or platform. Offer a visible fallback." | Stale |
| 13 | goals.md:31 — `EngineMissing` / `ERR_TTS_ENGINE_MISSING` / "No speech engine is installed; install one first" | error.rs:10,80; errors.ts:4,18 | Code and raised-when correct (Android init failure, voiceless Apple system, Windows REGDB_E_CLASSNOTREG). Message differs: "No text-to-speech engine is installed. Install one, then try again." | Stale |
| 14 | goals.md:32 — `VoiceNotFound` / "The voice is not available on this device" | error.rs:12,81; errors.ts:5,19 | Code and raised-when correct. Message differs: "That voice is unknown or not installed. Pick one from voices()." | Stale |
| 15 | goals.md:33 — `LanguageNotSupported` / "The language is not supported" | error.rs:14,82; errors.ts:6,20 | Code and raised-when correct (fake's list check, SSIP language refusal). Message differs: "The platform does not support that language tag. Pick one from languages()." | Stale |
| 16 | goals.md:34 — `LanguageMissingData` / "The voice data for this language needs downloading" | error.rs:16,83; errors.ts:7,21 | Code and raised-when correct (Android `LANG_MISSING_DATA` = −1, android.rs:120,144). Message differs: "...needs downloading in the system settings." | Stale |
| 17 | goals.md:35 — `NotSpeaking` / "Nothing is speaking" | error.rs:18,84; errors.ts:8,22 | Code and raised-when correct (pause idle, resume unpaused). Message differs: "Nothing is speaking, or nothing is paused." | Stale |
| 18 | goals.md:36 — `InvalidArgument` / "An argument was empty or out of range" | error.rs:20,85; errors.ts:9,23 | Code and raised-when correct (blank/oversize text, empty tag, rate/pitch/volume ranges — all in speech.rs/speech.ts). Message differs: "...such as blank text or a rate past 1.0." | Stale |
| 19 | goals.md:37 — `Busy` / "A synthesis is already running" | error.rs:22,86; errors.ts:10,24 | Code and raised-when correct. Message differs: "A synthesize is already running. Wait for its bytes." | Stale |
| 20 | goals.md:38 — `Cancelled` / "The utterance was cancelled" | error.rs:24,87; errors.ts:11,25 | Code and raised-when correct. Message differs: "The utterance was stopped before it finished." | Stale |
| 21 | goals.md:39 — `SynthesisFailed` / "The platform failed to synthesize the text" | error.rs:26,88; errors.ts:12,26 | Code, message and raised-when all match (web `onerror`, Windows refused render) | Done |
| 22 | goals.md:40 — `Backend` / "The platform speech service failed" | error.rs:28,89; errors.ts:13,27 | Code, message and raised-when all match; carries `status` (`backend_failure_carries_status`, both languages) | Done |
| 23 | goals.md:48 — `speaks_and_finishes_once` (both) | tests/speech.rs:64; tests/speech.test.ts:17 | Rust asserts id, events `[start, finish]`, exactly one finish; TS asserts the same through listeners | Done |
| 24 | goals.md:49 — `speak_rejects_empty_text` (both) | speech.rs:85; speech.test.ts:31 | Both assert `InvalidArgument`, backend never asked (`speaks() == 0`) | Done |
| 25 | goals.md:50 — `speak_rejects_blank_text` (both) | speech.rs:95; speech.test.ts:38 | Whitespace-only text refused in both | Done |
| 26 | goals.md:51 — `speak_rejects_oversize_text`, Android's 4000-char limit the reference case | speech.rs:105; speech.test.ts:45 | Both script a limit of 10 and refuse 11 while 10 speaks; the fake's default limit is 4000 (`the_fake_defaults_to_the_same_device_as_new`) | Done |
| 27 | goals.md:52 — `speak_queues_while_speaking` (QUEUE_ADD) | speech.rs:121; speech.test.ts:54 | Both assert FIFO order and the event sequence per utterance | Done |
| 28 | goals.md:53 — `speak_interrupts_current_utterance` (QUEUE_FLUSH) | speech.rs:147; speech.test.ts:70 | Both assert cancel of the running one, new one starts, `interrupts` counted (TS) / queue empty (Rust) | Done |
| 29 | goals.md:54 — `awaited_speak_resolves_once_on_finish` | speech.rs:170; speech.test.ts:87 | Both assert the call waits and settles exactly once | Done |
| 30 | goals.md:55 — `stop_cancels_awaited_speak_once` | speech.rs:188; speech.test.ts:107 | Both assert `Cancelled` exactly once, nothing left to settle, `[start, cancel]` | Done |
| 31 | goals.md:56 — `pause_and_resume_round_trip` | speech.rs:212; speech.test.ts:124 | Both assert one pause and one resume reached the backend | Done |
| 32 | goals.md:57 — `pause_when_idle_reports_not_speaking` | speech.rs:226; speech.test.ts:135 | Both assert `NotSpeaking`, backend never asked | Done |
| 33 | goals.md:58 — `resume_when_not_paused_reports_not_speaking` | speech.rs:237; speech.test.ts:142 | Both assert `NotSpeaking`, backend never asked | Done |
| 34 | goals.md:59 — `set_rate_rejects_out_of_range` (0.0..=1.0) | speech.rs:247; speech.test.ts:150 | Both refuse above/below/NaN and accept the endpoints | Done |
| 35 | goals.md:60 — `set_pitch_rejects_out_of_range` (0.5..=2.0) | speech.rs:262; speech.test.ts:164 | Same shape | Done |
| 36 | goals.md:61 — `set_volume_rejects_out_of_range` (0.0..=1.0) | speech.rs:277; speech.test.ts:178 | Same shape | Done |
| 37 | goals.md:62 — `set_language_rejects_empty` | speech.rs:292; speech.test.ts:192 | Both refuse `""` and whitespace | Done |
| 38 | goals.md:63 — `unknown_language_reports_unavailable` | speech.rs:304; speech.test.ts:202 | Rust asserts the `LanguageNotSupported` error and `is_language_available` false; TS asserts `isLanguageAvailable` false for an unknown tag (the availability half only) | Done |
| 39 | goals.md:64 — `set_unknown_voice_reports_voice_not_found` | speech.rs:323; speech.test.ts:209 | Both assert `VoiceNotFound`, backend never asked | Done |
| 40 | goals.md:65 — `uninstalled_voice_reports_voice_not_found` | speech.rs:334; speech.test.ts:218 | Both refuse a voice whose `installed` is false | Done |
| 41 | goals.md:66 — `init_reports_missing_engine` | speech.rs:348; speech.test.ts:225 | Both script `EngineMissing` and see it pass through | Done |
| 42 | goals.md:67 — `backend_failure_carries_status` | speech.rs:359; speech.test.ts:230 | Rust: status 500 on init; TS: status 42 on speak; both assert code and status | Done |
| 43 | goals.md:68 — `concurrent_synthesize_reports_busy` | speech.rs:370; speech.test.ts:235 | Both hold one synthesis, refuse the second with `Busy`, gate frees after | Done |
| 44 | goals.md:69 — `synthesize_returns_wav_bytes` | speech.rs:393; speech.test.ts:247 | Both assert `RIFF`/`WAVE` header | Done |
| 45 | goals.md:70 — `progress_events_report_location` | speech.rs:407; speech.test.ts:254 | Both assert `location` and `length` ride the progress event | Done |
| 46 | goals.md:71 — `set_silence_reaches_backend`; 0 starts immediately | speech.rs:432; speech.test.ts:265 | Both assert 250 and 0 reach the backend | Done |
| 47 | goals.md:72 — `error_codes_are_stable_and_unique`, plus errors.test.ts asserting the eleven strings | speech.rs:444; speech.test.ts:274; errors.test.ts:18 | Rust asserts all eleven exact code strings and pairwise distinctness; TS collects eleven through real paths; errors.test.ts walks the eleven strings | Done |
| 48 | goals.md:73 — `missing_voice_list_is_empty` | speech.rs:478; speech.test.ts:317 | Rust: a failing `voices()` reads as an empty list; TS: an empty configured list reads as empty, not an error | Done |
| 49 | goals.md:75 — "The TS memory backend has its own suite in `tests/memory.test.ts`" | packages/.../tests/memory.test.ts | Nine tests: queue, interrupt, stop, progress/error, scripted answers, counters | Done |
| 50 | goals.md:81 — Apple row: delegate callbacks, voice list, rate/pitch/volume, silence as pre-utterance delay, synthesis to a WAV buffer; tests run on `macos-latest` | platform/apple.rs | `delegate_messages_carry_one_utterance_from_start_to_finish`; `silence_becomes_pre_utterance_delay_seconds`; `wav_bytes_carry_a_16_bit_pcm_header`; `rate_spans_the_avfoundation_bounds`; ran green in the macos rust job | Done |
| 51 | goals.md:82 — Android row: QUEUE_FLUSH/ADD, engine list and default, `LANG_MISSING_DATA`/`LANG_NOT_SUPPORTED`, `isLanguageAvailable`, voices with installed flag, `getMaxSpeechInputLength`; compile in cross only | platform/android.rs | `queue_mode_flushes_on_interrupt_and_adds_on_queue`; `language_result_maps_engine_codes`; `max_length_reports_none_for_a_negative_answer`; JNI calls `getEngines`/`getDefaultEngine`/`getVoices`/`getMaxSpeechInputLength` verified (android.rs:1175–1256). But the bridge never calls `isLanguageAvailable`: availability comes from `setLanguage`'s int result and the voices' locales | Partial |
| 52 | goals.md:83 — Windows row: voice list, rate/pitch/volume, stream to WAV, synthesis fault to `SynthesisFailed`; tests on `windows-latest` | platform/windows.rs | `rate_maps_to_winrt_speaking_rate`; `refused_renders_report_synthesis_failed`; `backend_errors_carry_the_hresult_status`; `class_not_registered_reports_engine_missing`; green on the windows runner | Done |
| 53 | goals.md:84 — Linux row: speak/stop/pause/resume, rate/pitch/volume, the server's language list, **word-index progress**; tests on `ubuntu-latest` | platform/linux.rs | `normalized_settings_map_onto_ssip_hundred_steps`; `set_language_refusal_maps_to_language_not_supported`; `listed_voices_become_our_voices`; `languages_are_distinct_in_listed_order`. But there is no word-index progress: index marks are never requested and `EVENT_INDEX_MARK` is excluded from events (linux.rs:981, test `notification_status_codes_are_the_five_lifecycle_events`) | Stale |
| 54 | goals.md:85 — Web row: speak with queue or cancel, voice list, `onerror` to `SynthesisFailed`, `onboundary` progress; compile in cross only | platform/web.rs | `canceled_and_interrupted_are_cancels`; `synthesis_faults_are_synthesis_failed`; `onboundary` wired to Progress with `char_index`/`char_length` (web.rs:325–342) | Done |
| 55 | goals.md:87 — "Any other target gets the unsupported backend, and every call answers `ERR_TTS_NOT_AVAILABLE`" | platform/mod.rs:38, platform/unsupported.rs | Code right: `Speech::new()` itself fails with `NotAvailable` (no backend is constructed). The test `a_target_without_speech_reports_not_available` is cfg'd to targets no CI job builds, so no green run proves it | Untested |
| 56 | goals.md:89 — honest limits: android/web tests compile only; no gate plays audio | ci.yml, bridge file headers | True of `master`: cross-check compiles them, no device or browser runner exists | Done |
| 57 | parity.md:3 — names read from the two `.refs` files | `.refs/flutter_tts/lib/flutter_tts.dart`, `.refs/react-native-tts/index.d.ts` | Both files present; every name in the tables below verified against them | Done |
| 58 | parity.md:11 — rust matrix green (run 44043490843) | ci.yml rust job | `gh run view 44043490843`: rust (ubuntu/macos/windows) ✓ | Done |
| 59 | parity.md:12 — apple, android, wasm bridges compile (run 44043490843) | ci.yml cross-check | Same run: check (aarch64-apple-ios / aarch64-linux-android / wasm32-unknown-unknown) ✓ | Done |
| 60 | parity.md:13 — TS package gates green (run 44043490843) | ci.yml typescript | Same run: typescript ✓ | Done |
| 61 | parity.md:14 — format green (run 44043490843) | ci.yml format | Same run: format ✓ | Done |
| 62 | parity.md:15 — the speech behaviors verified by the named tests | both test suites | Rows 23–48; suites green in 44043490843 (and 44044962684 on `46b3f95`) | Done |
| 63 | parity.md:16 — "Audio out of a real speaker — not provable in this CI" | — | True: no runner plays audio | Done |
| 64 | parity.md:28 — `speak(text)` → `Speech::speak(text, interrupt)`; Android `focus` not taken | speech.rs:127; speech.ts:69 | `speaks_and_finishes_once`; `speak_queues_while_speaking`; `speak_interrupts_current_utterance`; reference's `focus` flag has no counterpart here | Done |
| 65 | parity.md:29 — `pause()` | speech.rs:173; speech.ts:131 | `pause_and_resume_round_trip` | Done |
| 66 | parity.md:30 — `stop()`; awaited speak resolves `Cancelled` once | speech.rs:167; speech.ts:125 | `stop_cancels_awaited_speak_once` | Done |
| 67 | parity.md:31 — `synthesizeToFile` → `synthesize(text)` returning WAV bytes; "every platform whose engine synthesizes" | speech.rs:199; speech.ts:152 | `synthesize_returns_wav_bytes`; `concurrent_synthesize_reports_busy`. Linux and web honestly answer `NotAvailable` (linux.rs:116, web.rs:535), consistent with the phrasing | Done |
| 68 | parity.md:32 — `setLanguage`; empty tag is `InvalidArgument` | speech.rs:218; speech.ts:164 | `set_language_rejects_empty`; `unknown_language_reports_unavailable` | Done |
| 69 | parity.md:33 — `setSpeechRate`, 0.0..=1.0 | speech.rs:243; speech.ts:178 | `set_rate_rejects_out_of_range` | Done |
| 70 | parity.md:34 — `setVolume`, 0.0..=1.0 | speech.rs:259; speech.ts:190 | `set_volume_rejects_out_of_range` | Done |
| 71 | parity.md:35 — `setPitch`, 0.5..=2.0, 1.0 normal | speech.rs:251; speech.ts:184 | `set_pitch_rejects_out_of_range` | Done |
| 72 | parity.md:36 — `setVoice` by id from `voices()`, not the name-plus-locale map | speech.rs:227; speech.ts:170 | `set_unknown_voice_reports_voice_not_found`; `uninstalled_voice_reports_voice_not_found`; reference takes `Map<String, String>` (flutter_tts.dart:477) | Done |
| 73 | parity.md:37 — `clearVoice` out of scope; no clear-to-default call | — | No such API exists, as the row says | Done |
| 74 | parity.md:38 — `getLanguages` → `languages()` | speech.rs:282; speech.ts:211 | `unknown_language_reports_unavailable`; `languages_list_what_the_device_offers` | Done |
| 75 | parity.md:39 — `getVoices` → `voices()`; empty list when none | speech.rs:272; speech.ts:201 | `missing_voice_list_is_empty` | Done |
| 76 | parity.md:40 — `getEngines` → `engines()`; "**other platforms answer an empty list**" | speech.rs:277; speech.ts:206; bridges' `engines()` | `engines_report_what_the_device_has` (pass-through). But apple, windows, linux each report one engine with `default: true` (apple.rs:364, windows.rs:715, linux.rs:125) and speech.rs documents "desktops and Apple report one" | Stale |
| 77 | parity.md:41 — `getDefaultEngine` mapped to the `Engine` whose `default` is true | android.rs:1175–1213 marks it; other bridges hardcode `default: true` | `engines()` exists on every path; no test asserts the default flag specifically (doc-level mapping, status word "Mapped") | Done |
| 78 | parity.md:42 — `getDefaultVoice` out of scope; `Voice` carries no default flag | options.rs:10; options.ts:7 | True: no default field | Done |
| 79 | parity.md:43 — `isLanguageAvailable` → `is_language_available(tag)` | speech.rs:287; speech.ts:216 | `unknown_language_reports_unavailable` | Done |
| 80 | parity.md:44 — `isLanguageInstalled` mapped to `voices()` + `LanguageMissingData` | speech.rs:272; error.rs:16 | `uninstalled_voice_reports_voice_not_found`; the code exists with its message | Done |
| 81 | parity.md:45 — `areLanguagesInstalled` mapped to the same list | speech.rs:282 | Doc-level mapping; list API exists and is tested | Done |
| 82 | parity.md:46 — `setSilence(timems)`; 0 starts immediately | speech.rs:267; speech.ts:196 | `set_silence_reaches_backend` | Done |
| 83 | parity.md:47 — `setQueueMode`: QUEUE_FLUSH (0) = `interrupt: true` (default), QUEUE_ADD (1) = `interrupt: false` | android.rs:116–133; speech.ts:69 | `speak_interrupts_current_utterance`; `speak_queues_while_speaking`; constants verified in flutter_tts.dart:565–567 | Done |
| 84 | parity.md:48 — `setEngine` out of scope; `engines()` reports what exists | — | No engine-setting API exists, as the row says | Done |
| 85 | parity.md:49 — `awaitSpeakCompletion(bool)`, default false, resolves exactly once | speech.rs:161; speech.ts:120 | `awaited_speak_resolves_once_on_finish`; `stop_cancels_awaited_speak_once` | Done |
| 86 | parity.md:50 — `awaitSynthCompletion` mapped away | speech.rs:199 | `synthesize_returns_wav_bytes` | Done |
| 87 | parity.md:51 — `getMaxSpeechInputLength`; `None` where no limit | speech.rs:301; speech.ts:222 | `speak_rejects_oversize_text`; `max_speech_input_length_reports_the_limit_or_none` | Done |
| 88 | parity.md:52 — `getSpeechRateValidRange` out of scope (rate normalized) | — | No such API, as the row says | Done |
| 89 | parity.md:53–56 — `setSharedInstance`, `autoStopSharedSession`, `setIosAudioCategory`, `setAudioAttributesForNavigation` out of scope | — | None exist here, as the rows say | Done |
| 90 | parity.md:57 — the seven handlers fold into one `UtteranceEvent` stream | speech.rs:77; speech.ts:39 | `progress_events_report_location`; `error_codes_are_stable_and_unique` | Done |
| 91 | parity.md:63 — `getInitStatus()` → `init()` | speech.rs:117; speech.ts:59 | `init_reports_missing_engine` | Done |
| 92 | parity.md:64 — `requestInstallEngine` mapped to the `EngineMissing` message | error.rs:100 | "Install one, then try again" is in the message | Done |
| 93 | parity.md:65 — `requestInstallData` mapped to `LanguageMissingData` | error.rs:108 | "needs downloading in the system settings" | Done |
| 94 | parity.md:66 — `setDucking` out of scope this release | — | No ducking code exists | Done |
| 95 | parity.md:67 — `setDefaultEngine` out of scope | — | Same | Done |
| 96 | parity.md:68 — `setDefaultVoice(voiceId)` → `set_voice(id)` | speech.rs:227; speech.ts:170 | `set_unknown_voice_reports_voice_not_found`; `uninstalled_voice_reports_voice_not_found` | Done |
| 97 | parity.md:69 — `setDefaultRate` mapped, normalized 0.0..=1.0, no `skipTransform` | speech.rs:243; speech.ts:178 | `set_rate_rejects_out_of_range` | Done |
| 98 | parity.md:70 — `setDefaultPitch`, same 0.5..=2.0 scale | speech.rs:251; speech.ts:184 | `set_pitch_rejects_out_of_range` | Done |
| 99 | parity.md:71 — `setDefaultLanguage` | speech.rs:218; speech.ts:164 | `set_language_rejects_empty` | Done |
| 100 | parity.md:72 — `setIgnoreSilentSwitch` out of scope (iOS audio session) | — | None exists | Done |
| 101 | parity.md:73 — `voices()` | speech.rs:272; speech.ts:201 | `missing_voice_list_is_empty` | Done |
| 102 | parity.md:74 — `engines()` | speech.rs:277; speech.ts:206 | `engines_report_what_the_device_has`; `init_reports_missing_engine` | Done |
| 103 | parity.md:75 — `speak(utterance, options?)` returns an id; options payload not taken | speech.rs:127; speech.ts:69 | `speaks_and_finishes_once` | Done |
| 104 | parity.md:76 — `stop(onWordBoundary?)`; flag not taken | speech.rs:167; speech.ts:125 | `stop_cancels_awaited_speak_once` | Done |
| 105 | parity.md:77 — `pause(onWordBoundary?)`; idle pause is `NotSpeaking` | speech.rs:173; speech.ts:131 | `pause_when_idle_reports_not_speaking` | Done |
| 106 | parity.md:78 — `resume()`; unpaused resume is `NotSpeaking` | speech.rs:183; speech.ts:138 | `resume_when_not_paused_reports_not_speaking` | Done |
| 107 | parity.md:79 — `addEventListener`/`removeEventListener` with the five names | speech.ts:39,48; options.ts:45 | `progress_events_report_location`; names `tts-start`…`tts-error` match index.d.ts:3–8 | Done |
| 108 | parity.md:80 — `Voice` type; `quality`/`latency` not kept, `notInstalled` inverted to `installed` | options.rs:10; options.ts:7 | Fields match; memory.test.ts reads voices back field for field; reference fields verified in index.d.ts:47–55 | Done |
| 109 | parity.md:81 — `Engine` type; RN's `icon` not kept | options.rs:25; options.ts:21 | Fields match; memory.test.ts | Done |
| 110 | parity.md:82 — `TtsError` → `SpeechError` as stable codes | errors.ts; error.rs | `error_codes_are_stable_and_unique`; `errors.test.ts` | Done |
| 111 | parity.md:90 — events row `start` | options.rs:37; speech.ts:12 | `speaks_and_finishes_once` | Done |
| 112 | parity.md:91 — events row `progress { location, length }` | options.rs:41 | `progress_events_report_location` | Done |
| 113 | parity.md:92 — events row `finish` | options.rs:43 | `speaks_and_finishes_once` | Done |
| 114 | parity.md:93 — events row `cancel` | options.rs:45 | `stop_cancels_awaited_speak_once` | Done |
| 115 | parity.md:94 — events row `error(SpeechError)` | options.rs:47 | `backend_failure_carries_status` | Done |
| 116 | parity.md:95 — `onPause`/`onContinue` map to none; the pause/resume results answer | — | No pause/continue event kinds exist, as the row says | Done |
| 117 | parity.md:103 — `no_engine` → `ERR_TTS_ENGINE_MISSING` | error.rs:10 | Code exists and tested | Done |
| 118 | parity.md:104 — `not_available` → `ERR_TTS_NOT_AVAILABLE` | error.rs:8 | Same | Done |
| 119 | parity.md:105 — `lang_missing_data` → `ERR_TTS_LANGUAGE_MISSING_DATA` | error.rs:16 | Same | Done |
| 120 | parity.md:106 — `lang_not_supported` → `ERR_TTS_LANGUAGE_NOT_SUPPORTED` | error.rs:14 | Same | Done |
| 121 | parity.md:107 — `invalid_request` → `ERR_TTS_INVALID_ARGUMENT` | error.rs:20 | Same | Done |
| 122 | parity.md:108 — `bad_rate` → `ERR_TTS_INVALID_ARGUMENT` | error.rs:20 | Same | Done |
| 123 | parity.md:109 — `synthesis_error` → `ERR_TTS_SYNTHESIS_FAILED` | error.rs:26 | Same | Done |
| 124 | parity.md:110 — everything else → `ERR_TTS_BACKEND`, status kept | error.rs:28,49 | The nine leftover names match the reference union exactly (index.d.ts:23–39); the `Backend` kind carries `status` | Done |
| 125 | parity.md:114 — additions: WAV-byte `synthesize`, `ERR_TTS_BUSY`, `ERR_TTS_NOT_SPEAKING`, the `status` field | speech.rs, error.rs, errors.ts | `synthesize_returns_wav_bytes`; `concurrent_synthesize_reports_busy`; `pause_when_idle_reports_not_speaking`; `backend_failure_carries_status` | Done |
| 126 | decisions.md:9 — NSH 0.5.1 pinned; CI installs `NSH_VERSION: v0.5.1`; history 0.4.0→0.5.0 (2026-11-12)→0.5.1 (2026-11-16) | crates/.../Cargo.toml:15; ci.yml:109 | `nsh-core = "0.5.1"` and `NSH_VERSION: v0.5.1` at `master`; commits 2205839 (2026-11-12) and 1b030ea (pushed 2026-11-16T23:12Z per its CI run) | Done |
| 127 | decisions.md:11 — five platforms in scope; every other target gets `ERR_TTS_NOT_AVAILABLE` through the unsupported backend | platform/mod.rs, platform/unsupported.rs | Five bridges cfg-selected; `no_backend()` answers `NotAvailable` (see row 55 on test coverage) | Done |
| 128 | decisions.md:13 — five reference repos cloned into `.refs/`, all URLs cloned successfully | `.refs/` | Five dirs (biometrics, flutter_tts, nsh, react-native-tts, tts-rs), remotes verified; `flutter_tts`'s remote `dlutton/flutter_tts` is that plugin's canonical repo | Done |
| 129 | brief.md:17 — "Voice not installed: show a visible fallback" | errors (messages), parity rows 92–93 | `NotAvailable` message says "Offer a visible fallback."; `uninstalled_voice_reports_voice_not_found`; `EngineMissing`/`LanguageMissingData` messages say what to do | Done |
| 130 | brief.md:18 — "Voice availability differs per device and language" | `voices()`/`languages()`/`is_language_available` per backend | `missing_voice_list_is_empty`; `unknown_language_reports_unavailable`; `languages_list_what_the_device_offers` | Done |
| 131 | brief.md:19 — "Speaking while another utterance runs: define the rule" | `speak(text, interrupt)` | `speak_queues_while_speaking`; `speak_interrupts_current_utterance` | Done |
| 132 | README.md:9 — Status "Implemented"; the listed feature set (queue/interrupt, pause, resume, stop, WAV synthesize, voices/engines/languages, setters, events, stable codes) | both APIs | Every listed API exists with a named test (rows 23–48, 64–110) | Done |
| 133 | README.md:11 — the bridges drive the platform engines | platform/*.rs | Row 1 | Done |
| 134 | README.md:16 — crate layout: lib.rs registers `nsh-text-to-speech/native`; speech/backend/options/error/fake; six bridges; `tests/speech.rs` | crates/... | All files present; `#[native_module("nsh-text-to-speech/native")]` at lib.rs:300; `the_native_module_registers_the_speech_class` | Done |
| 135 | README.md:17 — package layout: `nsh.json`, Speak demo in `App.tsx`, the seven `src/` files, tests `speech.test.ts`, `errors.test.ts`, `memory.test.ts` | packages/... | All listed files exist (App.tsx has the Speak button and the three testIDs). **Unlisted:** `tests/rust.test.ts` exists and is covered by no README mention | Partial |
| 136 | README.md:22 — `site/`: static status page published by `.github/workflows/pages.yml` | site/index.html, .github/workflows/pages.yml | Both exist at `master`; Pages runs green (44044962623). The working tree stages their deletion (in-flight switch to the web demo) | Done |
| 137 | README.md:26–31 — Checks: format; rust matrix; cross (three targets); typescript; "on every push to `master` and on pull requests" | ci.yml at `master` | Matches job for job; run 44043490843 green | Done |
| 138 | README.md:33 — "All four jobs are green on `master` (run 44043490843, commit `7bcdef8`)" | GitHub Actions | `gh run view 44043490843`: 8 jobs, all ✓, head 7bcdef8; the newer run on `46b3f95` (44044962684) is also green | Done |
| 139 | README.md:5 — "a web status page on GitHub Pages at https://supernsh.github.io/text-to-speech/" | Pages deployment | `curl -sI` → HTTP 200 | Done |
| 140 | The deployed page's own status line | site/index.html:23 | The live page says "**Status: scaffold.** No implementation yet." — the repo is implemented; the page contradicts README.md:9 | Stale |
| 141 | README.md:37 — MIT licence, `LICENSE` | LICENSE | MIT (file and GitHub both); repo URL matches `gh api` | Done |
| 142 | .zcode/workflow.ts:25–30 — all six phases "not-started" | the repo | Implementation exists and CI is green; every status is wrong | Stale |
| 143 | .zcode/workflow.ts:5 — phases include device-test, vision, smoke | CI | No such gates exist anywhere; the plan's phase list was never realized | Stale |
| 144 | .zcode/workflow-drafts/nsh-text-to-speech.workflow.ts — the frozen contract (crate, package, 26 tests, bridges, docs, CI) | the repo | Everything it promised exists (rows above). Its NSH statements are pinned to 0.4.0 (lines 41, 340, 360, 773) — stale as of 0.5.1 | Stale |

Row 2's gap, spelled out (the "identical behavior" half):

- **`stop` and the queue.** Rust keeps queued utterances (backend.rs:43 "Queued utterances are kept"; the fake's `stop` cancels the pending one and starts the next; apple "stop keeps what is queued"; windows test `stop_cancels_the_running_utterance_and_keeps_the_queue`; linux uses SSIP `STOP`). TS cancels the queue (speech.ts:124 "Cancels the running utterance and the queued ones"; memory.ts `stop` fires cancel on every queued entry). No named test covers stop-versus-queue, so both suites stay green while disagreeing.
- **Language-tag case.** Rust matches ASCII-case-insensitively (`eq_ignore_ascii_case`, asserted in `unknown_language_reports_unavailable`); TS uses `languages.includes(language)` — exact match only.
- **`setVoice` when `voices()` fails.** Rust reads a failed list as empty and answers `VoiceNotFound` (asserted in `missing_voice_list_is_empty`); TS passes the backend rejection through as `ERR_TTS_BACKEND`.

## NSH 0.4.0/0.5.0 statements

Every place in docs/ or code comments that states an 0.4.0/0.5.0/0.5.1 behavior (grepped):

- docs/decisions.md:9 — the pin history ("0.4.0 to 0.5.0 on 2026-11-12, and to 0.5.1 on 2026-11-16"). Accurate as history; the 0.5.1 pin is live. As of 0.5.1: true.
- crates/nsh-text-to-speech/Cargo.toml:59 — "The backend relies on 0.5.0's Priority wire mapping" with `ssip = "=0.5.0"`. This is the ssip crate, not NSH; the pin holds, so still true.
- crates/.../src/platform/linux.rs:375 — "ssip 0.5.0's Progress serializes as 'important'". Also ssip, not NSH; still true under the pin.
- .zcode/workflow-drafts/nsh-text-to-speech.workflow.ts:41, 340, 360, 773 — "NSH 0.4.0, matching the installed nsh CLI", the decision text, the CI install step, and the closing note. Stale as of 0.5.1; it is a planning artifact frozen before the bumps, so treat it as history, not spec.
- No NSH version statements exist in goals.md, parity.md, brief.md, README.md, AGENTS.md or any other code comment (the working-tree ci.yml mentions v0.5.1 only).
- Context: the `.refs/biometrics` clone pins `nsh-core = "0.4.0"` — a planning-time snapshot; the live `~/fo/nsh-biometrics` is on 0.5.1. `.refs/nsh` likewise predates 0.5.1; use `~/.cargo/registry/src/*/nsh-*-0.5.1/` for API lookups instead.

## Gaps against the reference standard

Not doc claims — the distance to `~/fo/nsh-biometrics` and `~/fo/nsh-icons` (READMEs and `.github/workflows/ci.yml` read there). Each verified today:

1. **No examples/ app on `master`.** biometrics ships `examples/demo`, icons `examples/gallery`; `master` has none. An untracked `examples/demo` draft appeared in the working tree during this audit — unfinished and unverified by any run. The uncommitted ci.yml references it in three jobs, so the two must land together.
2. **No demo matrix, no `publish` (latest release), no `pages` (web demo) jobs** in `master`'s ci.yml. The references package nine builds and replace a `latest` release on every push to `master`. Present only in the uncommitted ci.yml.
3. **Pages deploys the static status page**, not the demo's web build (`.github/workflows/pages.yml` at `master` uploads `site/`). The references deploy the web demo artifact.
4. **typescript job installs NSH from `raw.githubusercontent.com/SuperNSH/nsh-builds`** (master's ci.yml); the references install from `https://nsh.dev/install.sh` (`install.ps1` on Windows). Switched in the uncommitted tree.
5. **cross-check lacks `aarch64-apple-ios-sim`** at `master` (three targets; the references check four). Added in the uncommitted tree.
6. **No kache-action** at `master`; the references cache every job.
7. **No `LINUX_PACKAGES`** at `master`; the references install the GTK/WebKit/GStreamer dev packages on Linux runners.
8. **README has no "Try the demo" section** with the nine download links from the `latest` release; both references have one.
9. **No `latest` release exists** — `gh release list` is empty.
10. README has no "Install and use it in Rust / TSX" usage sections; both references show the `Cargo.toml`/`nsh.json` wiring and a snippet.
11. README's Layout omits `packages/nsh-text-to-speech/tests/rust.test.ts` (row 135).
12. When the ci.yml `pages` job lands, `.github/workflows/pages.yml` must be gone so two workflows never race on the same Pages deployment — the deletion is already staged in the working tree; keep it in the commit.

## Notes for implementers

Every row that is not Done, plus the gap list, as one-line actions:

- Rows 12–20 (goals.md error table): replace the nine message cells with the actual strings from error.rs/errors.ts (they are quoted in the rows), or decide the doc keeps paraphrases and say so — today the column reads as the literal messages and is wrong for nine of eleven.
- Row 2 / the three behavior divergences: pick one behavior for `stop`-versus-queue (the Rust "keep the queue" reading matches apple, windows and linux), make TS match it, and add a named test; decide whether TS `isLanguageAvailable` should fold case like Rust; make TS `setVoice` on a failed `voices()` answer `ERR_TTS_VOICE_NOT_FOUND` like Rust (or document the difference).
- Row 51 (goals.md android row): drop "`isLanguageAvailable`" from the cell or implement the call; today availability comes from `setLanguage`'s result and the voices' locales.
- Row 53 (goals.md linux row): drop "word-index progress" — the bridge deliberately never requests index marks (linux.rs:981) — or implement index-mark notifications.
- Row 55 (unsupported target): nothing to code; if you want it proven, one CI entry compiling any sixth target (e.g. `x86_64-unknown-none`) would run the cfg'd test.
- Row 76 (parity.md getEngines): fix the cell — non-Android platforms report one engine each, not an empty list.
- Row 135 (README Layout): add `tests/rust.test.ts` to the package's test list.
- Row 140: replace or delete the deployed status page — it still says "scaffold"; it goes away naturally when the web demo deploy (gap 3) lands.
- Rows 142–144 (.zcode): update `.zcode/workflow.ts` statuses to done (or delete the file); treat the draft as history.
- Gaps 1–12: land `examples/demo` first, then commit the ci.yml rewrite together with the pages.yml/site deletion and a README that describes the demo matrix, the `latest` release, the Pages web demo, and a "Try the demo" table — AGENTS.md counts a claim with no passing run as unverified, so the README must not describe the new jobs before a green run exists.

## Re-audit, 2026-11-18

Re-walked at `ff67c3c`, after the finishing work, by an independent verifier whose gates and GitHub checks were re-run (not trusted from this file). Every row is now Done, Stale-fixed or honestly limited:

- Rows 12–20 (error messages): Stale-fixed — goals.md's Message column is byte-identical to error.rs, and errors.ts carries the same eleven strings.
- Row 2 (identical behavior): fixed — `stop` cancels the running and every queued utterance in both languages (`stop_clears_the_queue`); language tags match case-insensitively in both (`language_matching_ignores_case`); a failed voice list propagates its error from `set_voice`/`setVoice` in both (`set_voice_propagates_a_failed_voice_list`); and the three list queries answer `[]` on failure in both, folded into `missing_voice_list_is_empty`.
- Row 3 re-checked: all 29 named edge-case tests exist under identical names in both suites, script-checked.
- Rows 51, 53, 76: Stale-fixed — goals.md's platform rows and parity.md now state what the bridges do.
- Row 55: fixed to the extent CI can — the test now compiles (it never had: `unwrap_err` on a non-`Debug` box) and `wasm32-wasip1` joins the cross job, so the unsupported path is compile-checked; it still runs nowhere, as goals.md says.
- Rows 135, 140, 142–144: Stale-fixed — the README lists what exists, the status page is gone from the repo and from Pages, and `.zcode/` states the truth.
- Gaps 1–12: closed. CI run 44100866879 (`da4fef0`) is green end to end — 21 jobs, publish and Pages included — and the docs-only `44110470920` (`ff67c3c`) is green the same way after one infrastructure-cancelled job was re-run. The `latest` release carries the nine demo builds, the nine stable download URLs answer 200, and Pages serves the web build (its hashed JS and wasm names match the released web tarball of the same run).

Still true and still a limit, by design: no gate plays audio, the android and web mapping tests compile but run nowhere, and the demo builds are unsigned.

## Re-check after the NSH 0.6.0 move, 2026-11-18

Run https://github.com/SuperNSH/text-to-speech/actions/runs/44376397808 (`61afb39`, "build: move to NSH 0.6.0") concluded success: format, rust on ubuntu, macos and windows, all five cross-check targets, typescript, the nine demo builds, publish and pages. Row 126 is now NSH 0.6.0 (`decisions.md` item 1). The 29 edge-case test names were re-checked as present in both suites, and the Rust and TS method lists still match the parity tables. No gaps found; the limits above (no audio, android and web tests compile only, unsigned demo builds) stand.
