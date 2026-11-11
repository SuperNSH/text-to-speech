# nsh-text-to-speech

Read text aloud on desktop and mobile.

Part of the Native Shell (NSH) capability set. Same layout as the other SuperNSH repos: a Rust crate, docs in `docs/`, CI on GitHub Actions, and a web status page on GitHub Pages at https://supernsh.github.io/text-to-speech/.

## Status

**Implemented.** `crates/nsh-text-to-speech` (Rust) and `packages/nsh-text-to-speech` (TypeScript) expose one `Speech` type with identical names and behavior: speak with queue or interrupt, pause, resume, stop, synthesize to WAV bytes, list voices, engines and languages, set language, voice, rate, pitch, volume and silence, report utterance events, and fail with stable `ERR_TTS_*` codes. The names follow Flutter's `flutter_tts`; [docs/parity.md](docs/parity.md) maps every reference API.

The bridges drive the platform engines: `AVSpeechSynthesizer` on Apple, `android.speech.tts` on Android, `Windows.Media.SpeechSynthesis` on Windows, speech-dispatcher on Linux, the Web Speech API on the web. [docs/goals.md](docs/goals.md) has the goals, the acceptance gates, the error table and the edge-case tests.

## Layout

- `Cargo.toml`: the workspace, one member.
- `crates/nsh-text-to-speech/`: the Rust crate. `src/lib.rs` registers the `nsh-text-to-speech/native` module; `speech.rs`, `backend.rs`, `options.rs`, `error.rs` and `fake.rs` hold the API, the backend trait and the scripted test backend; `src/platform/` holds the apple, android, windows, linux, web and unsupported bridges; `tests/speech.rs` holds the named behavior tests.
- `packages/nsh-text-to-speech/`: the TypeScript package, itself an NSH app (`nsh.json`) with a Speak demo in `App.tsx`. `src/` holds `index.ts`, `speech.ts`, `rust.ts`, `backend.ts`, `memory.ts`, `options.ts` and `errors.ts`; `tests/` holds `speech.test.ts`, `errors.test.ts` and `memory.test.ts`.
- `docs/goals.md`: goals, acceptance gates, error codes, edge cases, platform coverage.
- `docs/parity.md`: every `flutter_tts` and `react-native-tts` API and what it does here.
- `docs/decisions.md`: what was chosen and why.
- `docs/brief.md`: the research brief this started from.
- `site/`: static status page, published by `.github/workflows/pages.yml`.

## Checks

Every compile and test runs in GitHub Actions. Nothing is built on a developer machine. This workflow configures `.github/workflows/ci.yml` to run, on every push to `master` and on pull requests:

- **format**: `cargo fmt --all -- --check`.
- **rust**, a matrix on `ubuntu-latest`, `macos-latest` and `windows-latest`: `cargo clippy --all-targets -- -D warnings`, then `cargo test --all-targets`. The linux and windows bridges run natively here, and the apple bridge runs on the macOS runner.
- **cross**: clippy with `-D warnings` for `aarch64-apple-ios` on a macOS runner, and for `aarch64-linux-android` and `wasm32-unknown-unknown` on an ubuntu runner. Those targets are compile-checked only; CI has no device or browser runner, so the android and web mapping tests run nowhere in CI.
- **typescript**: `nsh check`, then `nsh build` with the job failing on any warning, then `nsh test` for the package.

The crate, the package and the four jobs above land in this same workflow. Until they are on `master` and the jobs have green runs there, every claim in this README is unverified, as `AGENTS.md` requires.

## Licence

MIT. See [LICENSE](LICENSE).

Repository: https://github.com/SuperNSH/text-to-speech
