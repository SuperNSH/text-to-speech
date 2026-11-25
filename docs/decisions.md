# Decisions

## Open

None.

## Decided

1. **NSH version: 0.7.1, pinned.** `nsh-core = "0.7.1"` matches the CLI CI installs (`NSH_VERSION: v0.7.1`), so what CI compiles is what a developer runs. The pin moved from 0.4.0 to 0.5.0 on 2026-11-12, to 0.5.1 on 2026-11-16, to 0.6.0 on 2026-11-18, and to 0.7.1 on 2026-11-25.
2. **Library choice: the platform speech engines behind our own `Speech` API, with flutter_tts's names.** flutter_tts and react-native-tts are the API references, tts-rs the per-platform technique reference; no third-party engine is bundled.
3. **Platforms in scope: apple, android, windows, linux, web.** The five targets the NSH capability set ships; every other target gets `ERR_TTS_NOT_AVAILABLE` through the unsupported backend.

The five reference repos were cloned into `.refs/` (git-ignored) for this workflow, and all five URLs cloned successfully, so no canonical-repo substitution was needed.

4. **`stop` clears the queue.** flutter_tts and react-native-tts cancel the queued utterances on stop, and Android's engine has no stop that keeps them, so `stop` cancels the running utterance and every queued one; each awaited `speak` resolves `Cancelled` exactly once (`stop_clears_the_queue`).
5. **`voices()`, `engines()` and `languages()` answer an empty list when the backend fails.** A failed list call reads as "none", not an error, in both languages; `missing_voice_list_is_empty` covers both the empty and the failed list.
6. **`set_voice` propagates a failed voice list.** The backend error answers, not `VoiceNotFound`, so an app can tell a broken list call from a wrong id (`set_voice_propagates_a_failed_voice_list`).
7. **Language tags match case-insensitively, per BCP 47.** BCP 47 tags are case-insensitive, so `en-us` and `en-US` are one language, in Rust and in TS (`language_matching_ignores_case`).
8. **The demo app ships from `examples/demo`.** CI packages it for nine platforms on every push to `master`, replaces the unsigned `latest` release with those builds, and serves its web build on Pages at https://supernsh.github.io/text-to-speech/, replacing the static status page (`site/` and `pages.yml` are gone).
9. **`wasm32-wasip1` joins the cross job.** It is an unsupported target, so the cross job is where the not-available path and its test compile; nothing runs there.
