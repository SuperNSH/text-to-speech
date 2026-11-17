// The library's tour: speak (interrupting or queueing), pause, resume, stop,
// synthesize to WAV, pick a voice and the settings, and watch every event.
// The memory engine runs by default, so the demo answers the same everywhere;
// one press swaps in the device's own engine.
import { computed, mount, signal, Text, View, Scroll, type JSX } from "nsh";
import { Button, Input } from "nsh/base";
import {
  memoryBackend,
  Speech,
  SpeechError,
  type Engine,
  type MemorySpeech,
  type UtteranceEvent,
  type Voice,
} from "../../packages/nsh-text-to-speech/src";

/** The scripted answers the memory engine gives: two voices (one not installed) and two languages. */
const MEMORY_VOICES: Voice[] = [
  { id: "memory-voice", name: "Memory voice", language: "en-US", networkRequired: false, installed: true },
  { id: "memory-remote", name: "Remote voice", language: "en-US", networkRequired: true, installed: false },
];
const MEMORY_LANGUAGES = ["de-DE", "en-US"];
/** How long the memory engine takes one utterance; a real engine decides on its own. */
const MEMORY_TURN_MS = 1200;
const LOG_CAP = 24;

const usingDevice = signal(false);
const interruptMode = signal(true);
const draft = signal("Hello from Native Shell");
const status = signal("idle");
const wavBytes = signal<number | null>(null);
const log = signal<string[]>([]);
const voices = signal<Voice[]>([]);
const engines = signal<Engine[]>([]);
const languages = signal<string[]>([]);
const language = signal("en-US");
const voiceId = signal("");
const rate = signal(0.5);
const pitch = signal(1);
const volume = signal(1);
const silenceMs = signal(0);

const newestFirst = computed(() => [...log()].reverse());

function codeOf(error: unknown): string {
  return error instanceof SpeechError ? error.code : "ERR_TTS_BACKEND";
}

function fail(error: unknown): void {
  status.set(codeOf(error));
}

function describe(event: UtteranceEvent): string {
  if (event.kind.type === "progress") return `progress ${event.utteranceId} at ${event.kind.location}/${event.kind.length}`;
  if (event.kind.type === "error") return `error ${event.utteranceId} ${event.kind.error.code}`;
  return `${event.kind.type} ${event.utteranceId}`;
}

let memory: MemorySpeech | null = null;
let speech: Speech;
let unwire: () => void;
let finishTimer: number | undefined;
/** A pause holds the memory engine mid-utterance, as a real engine holds its speech. */
let held = false;

/** The memory engine speaks until finishUtterance ends it; this drives it like the engine a device runs. */
function memoryTurn(): void {
  if (memory === null) return;
  if (held) {
    finishTimer = setTimeout(memoryTurn, 150);
    return;
  }
  const length = draft().length;
  memory.emitProgress(Math.floor(length / 2), length);
  memory.finishUtterance();
}

function makeSpeech(device: boolean): void {
  memory = device ? null : memoryBackend({ voices: MEMORY_VOICES, languages: MEMORY_LANGUAGES });
  speech = new Speech(memory === null ? {} : { backend: memory });
  const listener = (event: UtteranceEvent) => {
    log.update((lines) => [...lines, describe(event)].slice(-LOG_CAP));
    if (event.kind.type === "start" && memory !== null) {
      clearTimeout(finishTimer);
      finishTimer = setTimeout(memoryTurn, MEMORY_TURN_MS);
    }
    if (event.kind.type === "finish") {
      held = false;
      status.set(`finished ${event.utteranceId}`);
    }
    if (event.kind.type === "cancel") {
      held = false;
      status.set(`cancelled ${event.utteranceId}`);
    }
    if (event.kind.type === "error") {
      held = false;
      status.set(`error ${event.kind.error.code}`);
    }
  };
  const names = ["tts-start", "tts-progress", "tts-finish", "tts-cancel", "tts-error"] as const;
  for (const name of names) speech.addEventListener(name, listener);
  unwire = () => {
    for (const name of names) speech.removeEventListener(name, listener);
  };
}

function refreshLists(): void {
  speech.voices().then(voices.set, fail);
  speech.engines().then(engines.set, fail);
  speech.languages().then(
    (list) => {
      languages.set(list);
      if (!list.includes(language())) language.set(list[0] ?? "");
    },
    fail,
  );
}

function ready(): void {
  speech.init().then(
    () => {
      status.set("engine ready");
      refreshLists();
    },
    fail,
  );
}

const toggleBackend = () => {
  usingDevice.update((device) => !device);
  clearTimeout(finishTimer);
  held = false;
  const old = speech;
  unwire();
  makeSpeech(usingDevice());
  // The engine being dropped keeps its utterances; stop settles them there, out of this UI's sight.
  old.stop().catch(() => {});
  log.set([]);
  wavBytes.set(null);
  voiceId.set("");
  status.set("idle");
  ready();
};

const speak = () => {
  speech.speak(draft(), interruptMode()).then((id) => status.set(`speaking ${id}`), fail);
};
const pause = () => {
  speech.pause().then(() => {
    held = true;
    status.set("paused");
  }, fail);
};
const resume = () => {
  speech.resume().then(() => {
    held = false;
    status.set("resumed");
  }, fail);
};
const stop = () => {
  speech.stop().then(() => status.set("stopped"), fail);
};
const synthesize = () => {
  speech.synthesize(draft()).then((bytes) => {
    wavBytes.set(bytes.length);
    status.set(`synthesized ${bytes.length} bytes`);
  }, fail);
};

const applyLanguage = (tag: string) =>
  speech.setLanguage(tag).then(() => {
    language.set(tag);
    status.set(`language ${tag}`);
  }, fail);
const applyVoice = (id: string) =>
  speech.setVoice(id).then(() => {
    voiceId.set(id);
    status.set(`voice ${id}`);
  }, fail);
const applyRate = (value: number) =>
  speech.setSpeechRate(value).then(() => {
    rate.set(value);
    status.set(`rate ${value}`);
  }, fail);
const applyPitch = (value: number) =>
  speech.setPitch(value).then(() => {
    pitch.set(value);
    status.set(`pitch ${value}`);
  }, fail);
const applyVolume = (value: number) =>
  speech.setVolume(value).then(() => {
    volume.set(value);
    status.set(`volume ${value}`);
  }, fail);
const applySilence = (ms: number) =>
  speech.setSilence(ms).then(() => {
    silenceMs.set(ms);
    status.set(`silence ${ms} ms`);
  }, fail);

const ACTION = "rounded-lg border border-border bg-card px-3 py-1.5";
const PRIMARY = "rounded-lg bg-primary px-3 py-1.5";

function Section(props: { label: string; children?: JSX.Element }) {
  return (
    <View class="gap-1.5">
      <Text class="text-xs font-medium tracking-wide text-muted-foreground">{props.label}</Text>
      {props.children ?? <View />}
    </View>
  );
}

/** A row of preset chips. `active` runs inside each row, so a chip's selection stays live. */
function Chips(props: {
  of: Array<string | number>;
  id: (value: string | number) => string;
  label: (value: string | number) => string;
  active: (value: string | number) => boolean;
  apply: (value: string | number) => void;
}) {
  return (
    <View class="flex-row flex-wrap gap-1.5">
      <Each of={props.of}>
        {(value) => (
          <Button
            testID={props.id(value())}
            onPress={() => props.apply(value())}
            class={`rounded-full border px-2.5 py-1 ${props.active(value()) ? "border-primary bg-primary/10" : "border-border"}`}
          >
            <Text class={`text-xs ${props.active(value()) ? "text-primary" : "text-muted-foreground"}`}>{props.label(value())}</Text>
          </Button>
        )}
      </Each>
    </View>
  );
}

function App() {
  return (
    <Scroll class="flex-1" contentClass="gap-5 p-4">
      <View class="gap-0.5">
        <Text testID="title" class="text-lg font-semibold text-foreground">
          Text to speech
        </Text>
        <Text class="text-xs text-muted-foreground">One Speech over the platform engines: nsh-text-to-speech</Text>
      </View>

      <Section label="Engine">
        <View class="flex-row items-center gap-2">
          <Button testID="backend" onPress={toggleBackend} class={ACTION}>
            <Text class="text-sm text-foreground">{usingDevice() ? "Device" : "Memory"}</Text>
          </Button>
          <Text class="flex-1 text-xs text-muted-foreground">
            {usingDevice() ? "The device's own speech, through the Rust crate." : "The package's memory backend: deterministic, no audio."}
          </Text>
        </View>
        <Text testID="engines" class="text-xs text-muted-foreground">
          {`engines: ${engines().map((engine) => (engine.default ? `${engine.label} (default)` : engine.label)).join(", ") || "none listed"}`}
        </Text>
      </Section>

      <Section label="Utterance">
        <Input
          testID="text"
          value={draft()}
          onChange={(event) => draft.set(event.value)}
          placeholder="Text to speak"
          accessibilityLabel="Text to speak"
          class="rounded-lg border border-input bg-card px-3 py-2 text-sm text-foreground"
        />
        <View class="flex-row flex-wrap gap-2">
          <Button testID="speak" onPress={speak} class={PRIMARY}>
            <Text class="text-sm text-primary-foreground">Speak</Text>
          </Button>
          <Button testID="mode" onPress={() => interruptMode.update((mode) => !mode)} class={ACTION}>
            <Text class="text-sm text-foreground">{interruptMode() ? "Interrupts" : "Queues"}</Text>
          </Button>
          <Button testID="clear" onPress={() => draft.set("")} class={ACTION}>
            <Text class="text-sm text-foreground">Clear</Text>
          </Button>
        </View>
        <View class="flex-row flex-wrap gap-2">
          <Button testID="pause" onPress={pause} class={ACTION}>
            <Text class="text-sm text-foreground">Pause</Text>
          </Button>
          <Button testID="resume" onPress={resume} class={ACTION}>
            <Text class="text-sm text-foreground">Resume</Text>
          </Button>
          <Button testID="stop" onPress={stop} class={ACTION}>
            <Text class="text-sm text-foreground">Stop</Text>
          </Button>
          <Button testID="synthesize" onPress={synthesize} class={ACTION}>
            <Text class="text-sm text-foreground">Synthesize WAV</Text>
          </Button>
        </View>
        <Text testID="status" class="text-xs text-foreground">
          {status()}
        </Text>
        <Text testID="wav" class="text-xs text-muted-foreground">
          {wavBytes() === null ? "no audio synthesized" : `${wavBytes()} bytes of WAV`}
        </Text>
      </Section>

      <Section label="Settings">
        <Text testID="settings" class="text-xs text-muted-foreground">
          {`language ${language()} · voice ${voiceId() || "none"} · rate ${rate()} · pitch ${pitch()} · volume ${volume()} · silence ${silenceMs()} ms`}
        </Text>
        <Chips
          of={MEMORY_LANGUAGES}
          id={(value) => `language-${value}`}
          label={(value) => `language ${value}`}
          active={(value) => language() === value}
          apply={(value) => applyLanguage(String(value))}
        />
        <Chips
          of={[0.25, 0.5, 1]}
          id={(value) => `rate-${value}`}
          label={(value) => `rate ${value}`}
          active={(value) => rate() === value}
          apply={(value) => applyRate(Number(value))}
        />
        <Chips
          of={[0.5, 1, 1.5, 2]}
          id={(value) => `pitch-${value}`}
          label={(value) => `pitch ${value}`}
          active={(value) => pitch() === value}
          apply={(value) => applyPitch(Number(value))}
        />
        <Chips
          of={[0.25, 0.5, 1]}
          id={(value) => `volume-${value}`}
          label={(value) => `volume ${value}`}
          active={(value) => volume() === value}
          apply={(value) => applyVolume(Number(value))}
        />
        <Chips
          of={[0, 250, 500]}
          id={(value) => `silence-${value}`}
          label={(value) => `silence ${value} ms`}
          active={(value) => silenceMs() === value}
          apply={(value) => applySilence(Number(value))}
        />
      </Section>

      <Section label="Voices">
        {voices().length > 0 ? (
          <View class="gap-1.5">
            <Each of={voices()}>
              {(voice) => (
                <Button
                  testID={`voice-${voice().id}`}
                  onPress={() => applyVoice(voice().id)}
                  class={`rounded-lg border px-3 py-1.5 ${voiceId() === voice().id ? "border-primary bg-primary/10" : "border-border bg-card"}`}
                >
                  <Text class="text-xs text-foreground">
                    {`${voice().name} · ${voice().language}${voice().installed ? "" : " · not installed"}${voice().networkRequired ? " · network" : ""}`}
                  </Text>
                </Button>
              )}
            </Each>
          </View>
        ) : (
          <Text class="text-xs text-muted-foreground">none listed</Text>
        )}
      </Section>

      <Section label="Events (newest first)">
        <View testID="event-log" class="gap-0.5 rounded-lg border border-border bg-card p-2.5">
          {newestFirst().length > 0 ? (
            <Each of={newestFirst()}>
              {(line) => <Text class="font-mono text-xs text-foreground">{line()}</Text>}
            </Each>
          ) : (
            <Text class="text-xs text-muted-foreground">nothing yet</Text>
          )}
        </View>
      </Section>
    </Scroll>
  );
}

// The engine readies as the app loads, so the lists arrive filled.
makeSpeech(false);
ready();
mount(App);
