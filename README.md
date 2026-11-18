# nsh-text-to-speech

Read text aloud on desktop and mobile.

Part of the Native Shell (NSH) capability set. Same layout as the other SuperNSH repos: a Rust crate, docs in `docs/`, CI on GitHub Actions, and a web demo on GitHub Pages at https://supernsh.github.io/text-to-speech/.

## Status

**Implemented.** `crates/nsh-text-to-speech` (Rust) and `packages/nsh-text-to-speech` (TypeScript) expose one `Speech` type with identical names and behavior: speak with queue or interrupt, pause, resume, stop, synthesize to WAV bytes, list voices, engines and languages, set language, voice, rate, pitch, volume and silence, report utterance events, and fail with stable `ERR_TTS_*` codes. The names follow Flutter's `flutter_tts`; [docs/parity.md](docs/parity.md) maps every reference API.

The bridges drive the platform engines: `AVSpeechSynthesizer` on Apple, `android.speech.tts` on Android, `Windows.Media.SpeechSynthesis` on Windows, speech-dispatcher on Linux, the Web Speech API on the web. [docs/goals.md](docs/goals.md) has the goals, the acceptance gates, the error table and the edge-case tests.

## Try the demo

The demo in `examples/demo` is the library's tour: speak with interrupt or queue, pause, resume, stop, synthesize to WAV, pick a voice, a language and the settings, and watch every event. It runs on the memory engine by default, and one press swaps in the device's own engine. Open it on the web at https://supernsh.github.io/text-to-speech/, or download a build of the latest `master`:

| Platform | Download |
|---|---|
| macOS (Apple silicon) | [text-to-speech-demo-macos-arm64.zip](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-macos-arm64.zip) |
| macOS (Intel) | [text-to-speech-demo-macos-x86_64.zip](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-macos-x86_64.zip) |
| Linux x86_64 | [text-to-speech-demo-linux-x86_64.tar.gz](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-linux-x86_64.tar.gz) |
| Linux arm64 | [text-to-speech-demo-linux-arm64.tar.gz](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-linux-arm64.tar.gz) |
| Windows x86_64 | [text-to-speech-demo-windows-x86_64.zip](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-windows-x86_64.zip) |
| Windows arm64 | [text-to-speech-demo-windows-arm64.zip](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-windows-arm64.zip) |
| iOS simulator | [text-to-speech-demo-ios-simulator.zip](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-ios-simulator.zip) |
| Android (arm64) | [text-to-speech-demo-android.apk](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-android.apk) |
| Web (static site) | [text-to-speech-demo-web.tar.gz](https://github.com/SuperNSH/text-to-speech/releases/latest/download/text-to-speech-demo-web.tar.gz) |

- **macOS:** the app is signed ad hoc, not notarized, and needs macOS 14 or later. macOS quarantines what a browser downloads, so clear that before opening it: `xattr -dr com.apple.quarantine <the unzipped .app>`.
- **iOS simulator:** an arm64 simulator build, so it needs Xcode with an iOS simulator runtime on an Apple silicon Mac. Unzip it and install it on a booted simulator with `xcrun simctl install booted <the unzipped .app>`, or drag the `.app` onto the simulator window.

## Install and use it in Rust

Add the crate to `Cargo.toml`:

```toml
[dependencies]
nsh-text-to-speech = { git = "https://github.com/SuperNSH/text-to-speech" }
```

```rust
use nsh_text_to_speech::{Speech, SpeechError};

async fn greet(speech: &Speech) -> Result<(), SpeechError> {
    speech.set_language("en-US").await?;
    speech.set_speech_rate(0.5).await?;
    speech.speak("Hello from Native Shell", true).await?;
    Ok(())
}
```

Match on `error.code()` for the cases an app handles, such as `ERR_TTS_ENGINE_MISSING` or `ERR_TTS_CANCELLED`. Every utterance event reaches one handler: `speech.set_event_handler(|event| ...)`.

## Install and use it in TSX

List the crate under `"rust"` in the app's `nsh.json`, and add the TypeScript package from `packages/nsh-text-to-speech` to the app:

```json
{
  "name": "My app",
  "id": "com.example.app",
  "version": "0.1.0",
  "build": 1,
  "rust": "path/to/nsh-text-to-speech"
}
```

```tsx
import { mount, Text, View } from "nsh";
import { Speech, SpeechError } from "nsh-text-to-speech";

const speech = new Speech();

function App() {
  const say = () => {
    speech.speak("Hello from Native Shell").then(
      (id) => console.log("speaking", id),
      (error: unknown) => console.log(error instanceof SpeechError ? error.code : "unknown error"),
    );
  };
  return (
    <View class="flex-1 p-4">
      <Text role="button" onPress={say} class="rounded-(--radius) bg-muted px-3 py-2">Speak</Text>
    </View>
  );
}

mount(App);
```

`error.code` carries the same stable `ERR_TTS_*` codes as Rust. Events arrive through `addEventListener` under react-native-tts's five names, `tts-start` through `tts-error`.

`examples/demo` is a complete app built on the package. [Try the demo](#try-the-demo) links its builds.

## Layout

- `Cargo.toml`: the workspace, one member.
- `crates/nsh-text-to-speech/`: the Rust crate. `src/lib.rs` registers the `nsh-text-to-speech/native` module; `speech.rs`, `backend.rs`, `options.rs`, `error.rs` and `fake.rs` hold the API, the backend trait and the scripted test backend; `src/platform/` holds the apple, android, windows, linux, web and unsupported bridges; `tests/speech.rs` holds the named behavior tests.
- `packages/nsh-text-to-speech/`: the TypeScript package, itself an NSH app (`nsh.json`) with a Speak demo in `App.tsx`. `src/` holds `index.ts`, `speech.ts`, `rust.ts`, `backend.ts`, `memory.ts`, `options.ts` and `errors.ts`; `tests/` holds `speech.test.ts`, `errors.test.ts`, `memory.test.ts` and `rust.test.ts`.
- `examples/demo/`: the demo app — `App.tsx`, `nsh.json` and `tests/app.test.tsx`.
- `docs/goals.md`: goals, acceptance gates, error codes, edge cases, platform coverage.
- `docs/parity.md`: every `flutter_tts` and `react-native-tts` API and what it does here.
- `docs/decisions.md`: what was chosen and why.
- `docs/audit.md`: every doc claim checked against the code and the CI runs.
- `docs/brief.md`: the research brief this started from.

## Checks

Every compile and test runs in GitHub Actions. Nothing is built on a developer machine. `.github/workflows/ci.yml` runs, on every push to `master` and on pull requests:

- **format**: `cargo fmt --all -- --check`.
- **rust**, a matrix on `ubuntu-latest`, `macos-latest` and `windows-latest`: `cargo clippy -p nsh-text-to-speech --all-targets -- -D warnings`, then `cargo test -p nsh-text-to-speech --all-targets`. The linux and windows bridges run natively here, and the apple bridge runs on the macOS runner.
- **cross**: clippy with `-D warnings` for `aarch64-apple-ios` and `aarch64-apple-ios-sim` on a macOS runner, and for `aarch64-linux-android`, `wasm32-unknown-unknown` and `wasm32-wasip1` on an ubuntu runner. Those targets are compile-checked only; CI has no device or browser runner, so the android and web mapping tests run nowhere in CI. `wasm32-wasip1` is the unsupported target, and the cross job is where its not-available path and test compile.
- **typescript**: `nsh check`, then `nsh build` with the job failing on any warning, then `nsh test` — once for the package (`packages/nsh-text-to-speech`), once for the demo (`examples/demo`).

On pushes to `master` only, after those jobs:

- **demo**: a nine-platform matrix packages `examples/demo` — macOS on arm64 and x86_64, Linux on x86_64 and arm64, Windows on x86_64 and arm64, the iOS simulator, Android (arm64-v8a) and the web.
- **publish**: replaces the `latest` release with those nine builds — the downloads in [Try the demo](#try-the-demo). The builds are unsigned: macOS and Windows warn before opening them, and the iOS build runs only in the simulator.
- **pages**: deploys the demo's web build to GitHub Pages at https://supernsh.github.io/text-to-speech/.

Every job was green end to end — publish and Pages included — on `master` (run 44376397808, commit `61afb39`, NSH 0.6.0), so the claims above are verified.

## Licence

MIT. See [LICENSE](LICENSE).

Repository: https://github.com/SuperNSH/text-to-speech
