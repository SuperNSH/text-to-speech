import type { SpeechBackend } from "./backend";
import { SpeechError, type SpeechErrorCode } from "./errors";
import type { Engine, UtteranceEvent, UtteranceId, Voice } from "./options";

/** The answers a memory backend gives. Each call answers what the config says. */
export interface MemorySpeechConfig {
  available?: boolean;
  /** `init` rejects with this code. */
  initError?: SpeechErrorCode;
  voices?: Voice[];
  engines?: Engine[];
  languages?: string[];
  /** `speak` rejects with this failure instead of speaking. */
  speakError?: { code: SpeechErrorCode; status?: number };
  /** `synthesize` rejects with this code instead of answering bytes. */
  synthesizeError?: SpeechErrorCode;
  /** What `synthesize` resolves with; a WAV header when left out. */
  wav?: Uint8Array;
  maxSpeechInputLength?: number;
}

/** A backend the test drives: it finishes utterances and counts what was asked. */
export interface MemorySpeech extends SpeechBackend {
  /** Ends the running utterance with a finish event and starts the queued one. */
  finishUtterance(): boolean;
  /** Ends the running utterance with an error event and starts the queued one. */
  failUtterance(code: SpeechErrorCode, status?: number): boolean;
  /** Fires a progress event on the running utterance. */
  emitProgress(location: number, length: number): boolean;
  speaks(): number;
  interrupts(): number;
  stops(): number;
  pauses(): number;
  resumes(): number;
  lastLanguage(): string | undefined;
  lastVoice(): string | undefined;
  lastRate(): number | undefined;
  lastPitch(): number | undefined;
  lastVolume(): number | undefined;
  lastSilence(): number | undefined;
  setVoices(voices: Voice[]): void;
}

interface Queued {
  id: UtteranceId;
  text: string;
  onEvent: (event: UtteranceEvent) => void;
}

/** A minimal WAV header: "RIFF", a size, "WAVE" and an empty "data" chunk. */
function wavHeader(): Uint8Array {
  const bytes = new Uint8Array(44);
  const put = (offset: number, text: string) => {
    for (let index = 0; index < text.length; index += 1) bytes[offset + index] = text.charCodeAt(index);
  };
  put(0, "RIFF");
  put(8, "WAVE");
  put(12, "fmt ");
  put(36, "data");
  return bytes;
}

/** A memory backend, for tests and for apps that run without an engine. */
export function memoryBackend(config: MemorySpeechConfig = {}): MemorySpeech {
  let available = config.available ?? true;
  let voices = [...(config.voices ?? [])];
  let engines = [...(config.engines ?? [{ name: "com.google.android.tts", label: "Google TTS", default: true }])];
  let languages = [...(config.languages ?? ["en-US"])];
  let nextId = 0;
  let current: Queued | null = null;
  const queue: Queued[] = [];
  let speaks = 0;
  let interrupts = 0;
  let stops = 0;
  let pauses = 0;
  let resumes = 0;
  let lastLanguage: string | undefined;
  let lastVoice: string | undefined;
  let lastRate: number | undefined;
  let lastPitch: number | undefined;
  let lastVolume: number | undefined;
  let lastSilence: number | undefined;

  const fire = (entry: Queued, kind: UtteranceEvent["kind"]) => entry.onEvent({ utteranceId: entry.id, kind });
  const start = (entry: Queued) => fire(entry, { type: "start" });
  // One utterance owns the channel; what queued behind it starts when it ends.
  const advance = () => {
    current = queue.shift() ?? null;
    if (current !== null) start(current);
  };

  return {
    init: () => (config.initError !== undefined ? Promise.reject(new SpeechError(config.initError)) : Promise.resolve()),
    isAvailable: async () => available,
    speak: (text, interrupt, onEvent) => {
      speaks += 1;
      if (config.speakError !== undefined) {
        return Promise.reject(new SpeechError(config.speakError.code, config.speakError.status));
      }
      nextId += 1;
      const entry: Queued = { id: nextId, text, onEvent };
      if (current !== null && interrupt) {
        interrupts += 1;
        fire(current, { type: "cancel" });
        current = entry;
        start(entry);
      } else if (current !== null) {
        queue.push(entry);
      } else {
        current = entry;
        start(entry);
      }
      return Promise.resolve(entry.id);
    },
    stop: () => {
      stops += 1;
      if (current !== null) fire(current, { type: "cancel" });
      for (const entry of queue) fire(entry, { type: "cancel" });
      current = null;
      queue.length = 0;
      return Promise.resolve();
    },
    pause: () => {
      pauses += 1;
      return Promise.resolve();
    },
    resume: () => {
      resumes += 1;
      return Promise.resolve();
    },
    isSpeaking: () => Promise.resolve(current !== null),
    synthesize: (_text) => {
      if (config.synthesizeError !== undefined) return Promise.reject(new SpeechError(config.synthesizeError));
      return Promise.resolve(config.wav !== undefined ? new Uint8Array(config.wav) : wavHeader());
    },
    voices: async () => [...voices],
    engines: async () => [...engines],
    languages: async () => [...languages],
    maxSpeechInputLength: () => Promise.resolve(config.maxSpeechInputLength ?? null),
    setLanguage: (language) => {
      lastLanguage = language;
      return Promise.resolve();
    },
    setVoice: (voiceId) => {
      lastVoice = voiceId;
      return Promise.resolve();
    },
    setRate: (rate) => {
      lastRate = rate;
      return Promise.resolve();
    },
    setPitch: (pitch) => {
      lastPitch = pitch;
      return Promise.resolve();
    },
    setVolume: (volume) => {
      lastVolume = volume;
      return Promise.resolve();
    },
    setSilence: (ms) => {
      lastSilence = ms;
      return Promise.resolve();
    },
    finishUtterance: () => {
      if (current === null) return false;
      fire(current, { type: "finish" });
      advance();
      return true;
    },
    failUtterance: (code, status) => {
      if (current === null) return false;
      fire(current, { type: "error", error: new SpeechError(code, status) });
      advance();
      return true;
    },
    emitProgress: (location, length) => {
      if (current === null) return false;
      fire(current, { type: "progress", location, length });
      return true;
    },
    speaks: () => speaks,
    interrupts: () => interrupts,
    stops: () => stops,
    pauses: () => pauses,
    resumes: () => resumes,
    lastLanguage: () => lastLanguage,
    lastVoice: () => lastVoice,
    lastRate: () => lastRate,
    lastPitch: () => lastPitch,
    lastVolume: () => lastVolume,
    lastSilence: () => lastSilence,
    setVoices: (voices_) => {
      voices = [...voices_];
    },
  };
}
