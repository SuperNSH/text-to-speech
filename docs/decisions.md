# Decisions

## Open

None.

## Decided

1. **NSH version: 0.4.0, pinned.** It matches `.refs/nsh` and the installed `nsh` CLI, so what CI compiles is what a developer runs.
2. **Library choice: the platform speech engines behind our own `Speech` API, with flutter_tts's names.** flutter_tts and react-native-tts are the API references, tts-rs the per-platform technique reference; no third-party engine is bundled.
3. **Platforms in scope: apple, android, windows, linux, web.** The five targets the NSH capability set ships; every other target gets `ERR_TTS_NOT_AVAILABLE` through the unsupported backend.

The five reference repos were cloned into `.refs/` (git-ignored) for this workflow, and all five URLs cloned successfully, so no canonical-repo substitution was needed.
