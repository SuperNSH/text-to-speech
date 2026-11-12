# Decisions

## Open

None.

## Decided

1. **NSH version: 0.5.0, pinned.** `nsh-core = "0.5.0"` matches the CLI CI installs (`NSH_VERSION: v0.5.0`), so what CI compiles is what a developer runs. The pin moved from 0.4.0 on 2026-11-12.
2. **Library choice: the platform speech engines behind our own `Speech` API, with flutter_tts's names.** flutter_tts and react-native-tts are the API references, tts-rs the per-platform technique reference; no third-party engine is bundled.
3. **Platforms in scope: apple, android, windows, linux, web.** The five targets the NSH capability set ships; every other target gets `ERR_TTS_NOT_AVAILABLE` through the unsupported backend.

The five reference repos were cloned into `.refs/` (git-ignored) for this workflow, and all five URLs cloned successfully, so no canonical-repo substitution was needed.
