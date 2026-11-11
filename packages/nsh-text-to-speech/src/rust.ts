import { NativeSpeech, type NativeUtteranceEvent } from "nsh-text-to-speech/native";
import type { SpeechBackend } from "./backend";
import { asSpeechError } from "./errors";
import type { Engine, UtteranceEvent, UtteranceId, UtteranceKind, Voice } from "./options";

const TERMINAL: ReadonlySet<string> = new Set(["finish", "cancel", "error"]);
const EARLY_CAPACITY = 32;

/**
 * Adds `raw` to the early events: ones that arrived before their `speak` call registered a sink.
 * Over capacity only a non-terminal rotates out — a dropped terminal would hang an awaited
 * `speak`, which settles on that very event.
 */
export function bufferEarly(early: NativeUtteranceEvent[], raw: NativeUtteranceEvent): void {
  early.push(raw);
  if (early.length > EARLY_CAPACITY) {
    const oldestNonTerminal = early.findIndex((event) => !TERMINAL.has(event.kind));
    if (oldestNonTerminal !== -1) early.splice(oldestNonTerminal, 1);
  }
}

/**
 * Takes the events held for `id` out of `early`, in arrival order, and reports whether they
 * ended the utterance: the sink goes then, as the live path's terminal does.
 */
export function takeHeld(
  early: NativeUtteranceEvent[],
  id: UtteranceId,
): { held: NativeUtteranceEvent[]; ended: boolean } {
  const held = early.filter((raw) => raw.utteranceId === id);
  for (let index = early.length - 1; index >= 0; index -= 1) {
    if (early[index].utteranceId === id) early.splice(index, 1);
  }
  return { held, ended: held.some((raw) => TERMINAL.has(raw.kind)) };
}

function toEvent(raw: NativeUtteranceEvent): UtteranceEvent | null {
  const kind: UtteranceKind | null =
    raw.kind === "start" || raw.kind === "finish" || raw.kind === "cancel"
      ? { type: raw.kind }
      : raw.kind === "progress"
        ? { type: "progress", location: raw.location ?? 0, length: raw.length ?? 0 }
        : raw.kind === "error"
          ? { type: "error", error: asSpeechError({ code: raw.errorCode, message: raw.errorMessage }) }
          : null;
  return kind === null ? null : { utteranceId: raw.utteranceId, kind };
}

/**
 * The Rust crate's speech (`nsh-text-to-speech/native`). The app or package must list the crate
 * under `"rust"` in `nsh.json`; importing this module without it fails the bundle.
 */
export function rustBackend(): SpeechBackend {
  return new RustBackend();
}

class RustBackend implements SpeechBackend {
  private readonly native: NativeSpeech;
  private readonly sinks = new Map<UtteranceId, (event: UtteranceEvent) => void>();
  // The platform's start can beat speak's own promise; held here until its sink registers.
  private readonly early: NativeUtteranceEvent[] = [];

  constructor() {
    try {
      this.native = new NativeSpeech();
    } catch (error) {
      throw asSpeechError(error);
    }
    this.native.setEventHandler((event) => this.receive(event));
  }

  init(): Promise<void> {
    return this.settle(() => this.native.init());
  }

  // The native class has no probe: its constructor is the check, and it threw where speech cannot run.
  isAvailable(): Promise<boolean> {
    return Promise.resolve(true);
  }

  speak(text: string, interrupt: boolean, onEvent: (event: UtteranceEvent) => void): Promise<UtteranceId | null> {
    return this.settle(async () => {
      const id = await this.native.speak(text, interrupt);
      this.sinks.set(id, onEvent);
      const { held, ended } = takeHeld(this.early, id);
      for (const raw of held) {
        const event = toEvent(raw);
        if (event !== null) onEvent(event);
      }
      // A terminal arrived early: the sink goes now, as receive()'s direct path does.
      if (ended) this.sinks.delete(id);
      return id;
    });
  }

  stop(): Promise<void> {
    return this.settle(() => this.native.stop());
  }

  pause(): Promise<void> {
    return this.settle(() => this.native.pause());
  }

  resume(): Promise<void> {
    return this.settle(() => this.native.resume());
  }

  isSpeaking(): Promise<boolean> {
    return this.settle(() => this.native.isSpeaking());
  }

  synthesize(text: string): Promise<Uint8Array> {
    return this.settle(() => this.native.synthesize(text));
  }

  voices(): Promise<Voice[]> {
    return this.settle(() => this.native.voices());
  }

  engines(): Promise<Engine[]> {
    return this.settle(() => this.native.engines());
  }

  languages(): Promise<string[]> {
    return this.settle(() => this.native.languages());
  }

  maxSpeechInputLength(): Promise<number | null> {
    return this.settle(() => this.native.maxSpeechInputLength());
  }

  setLanguage(language: string): Promise<void> {
    return this.settle(() => this.native.setLanguage(language));
  }

  setVoice(voiceId: string): Promise<void> {
    return this.settle(() => this.native.setVoice(voiceId));
  }

  setRate(rate: number): Promise<void> {
    return this.settle(() => this.native.setSpeechRate(rate));
  }

  setPitch(pitch: number): Promise<void> {
    return this.settle(() => this.native.setPitch(pitch));
  }

  setVolume(volume: number): Promise<void> {
    return this.settle(() => this.native.setVolume(volume));
  }

  setSilence(ms: number): Promise<void> {
    return this.settle(() => this.native.setSilence(ms));
  }

  private receive(raw: NativeUtteranceEvent): void {
    const sink = this.sinks.get(raw.utteranceId);
    if (sink === undefined) {
      bufferEarly(this.early, raw);
      return;
    }
    const event = toEvent(raw);
    if (event !== null) sink(event);
    if (TERMINAL.has(raw.kind)) this.sinks.delete(raw.utteranceId);
  }

  private async settle<T>(operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      throw asSpeechError(error);
    }
  }
}
