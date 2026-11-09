# Text to speech: brief

Read text aloud on desktop and mobile.

## Options

- tts: Rust, desktop platform speech backends
- AVSpeechSynthesizer: Apple, native
- TextToSpeech: Android, native

## Recommended path

tts on desktop. Native speech on mobile.

## Edge cases to design for

- Voice not installed: show a visible fallback
- Voice availability differs per device and language
- Speaking while another utterance runs: define the rule

## Research status

Research done 2026-11-09 by web search. Versions and licences are unverified until checked on the registry before pinning. Full catalogue: the research notes for the SuperNSH library set.
