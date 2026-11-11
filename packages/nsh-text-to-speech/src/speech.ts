import type { SpeechBackend } from "./backend";
import { asSpeechError, speechError } from "./errors";
import type { Engine, UtteranceEvent, UtteranceEventName, UtteranceId, UtteranceKind, Voice } from "./options";
import { rustBackend } from "./rust";

/** The `Speech` constructor's options. `backend` defaults to the Rust crate's. */
export interface SpeechOptions {
  backend?: SpeechBackend;
}

function eventName(type: UtteranceKind["type"]): UtteranceEventName {
  if (type === "start") return "tts-start";
  if (type === "progress") return "tts-progress";
  if (type === "finish") return "tts-finish";
  if (type === "cancel") return "tts-cancel";
  return "tts-error";
}

/**
 * Reading text aloud, as `flutter_tts` and `react-native-tts` name it: set a language, a voice
 * and a rate, then `speak` an utterance. It runs on the Rust crate unless it is given another
 * backend. While one utterance runs, `interrupt` (the default) cancels it and speaks at once;
 * otherwise the new utterance queues behind it, first in, first out. Every call resolves
 * exactly once.
 */
export class Speech {
  private readonly backend: SpeechBackend;
  private readonly listeners = new Map<UtteranceEventName, Set<(event: UtteranceEvent) => void>>();
  private awaitCompletion = false;
  private paused = false;
  private synthesizing = false;
  private localIds = 0;

  constructor(options: SpeechOptions = {}) {
    this.backend = options.backend ?? rustBackend();
  }

  /** Adds a listener for one event name, react-native-tts's names. Returns nothing. */
  addEventListener(name: UtteranceEventName, listener: (event: UtteranceEvent) => void): void {
    let named = this.listeners.get(name);
    if (named === undefined) {
      named = new Set();
      this.listeners.set(name, named);
    }
    named.add(listener);
  }

  removeEventListener(name: UtteranceEventName, listener: (event: UtteranceEvent) => void): void {
    this.listeners.get(name)?.delete(listener);
  }

  private emit(event: UtteranceEvent): void {
    const named = this.listeners.get(eventName(event.kind.type));
    if (named === undefined) return;
    for (const listener of [...named]) listener(event);
  }

  /** Prepares the platform's speech; its error says whether an engine is missing. */
  init(): Promise<void> {
    return this.settle(() => this.backend.init());
  }

  /**
   * Speaks `text` aloud. Resolves with the utterance's id, or with nothing else: blank text and
   * text past the platform's limit reject with `ERR_TTS_INVALID_ARGUMENT`. With
   * `awaitSpeakCompletion(true)` the call settles only when the utterance ends: its id when it
   * finishes, `ERR_TTS_CANCELLED` when it was stopped, its error when it failed.
   */
  async speak(text: string, interrupt = true): Promise<UtteranceId> {
    if (text.trim() === "") throw speechError("ERR_TTS_INVALID_ARGUMENT");
    const limit = await this.settle(() => this.backend.maxSpeechInputLength());
    if (limit !== null && text.length > limit) throw speechError("ERR_TTS_INVALID_ARGUMENT");

    return new Promise<UtteranceId>((resolve, reject) => {
      let settled = false;
      const done = (id: UtteranceId) => {
        if (settled) return;
        settled = true;
        resolve(id);
      };
      const cancelled = () => {
        if (settled) return;
        settled = true;
        reject(speechError("ERR_TTS_CANCELLED"));
      };
      const failed = (error: unknown) => {
        if (settled) return;
        settled = true;
        reject(asSpeechError(error));
      };
      const sink = (event: UtteranceEvent) => {
        this.emit(event);
        if (event.kind.type === "finish") {
          this.paused = false;
          done(event.utteranceId);
        } else if (event.kind.type === "cancel") {
          this.paused = false;
          cancelled();
        } else if (event.kind.type === "error") {
          this.paused = false;
          failed(event.kind.error);
        }
      };
      let started: Promise<UtteranceId | null>;
      try {
        started = this.backend.speak(text, interrupt, sink);
      } catch (error) {
        started = Promise.reject(error);
      }
      started.then(
        (id) => {
          if (!this.awaitCompletion) done(id ?? this.nextLocalId());
        },
        (error: unknown) => failed(error),
      );
    });
  }

  /** Whether `speak` waits for its utterance to end. Off by default. */
  awaitSpeakCompletion(wait: boolean): void {
    this.awaitCompletion = wait;
  }

  /** Cancels the running utterance and the queued ones; an awaited `speak` rejects with `ERR_TTS_CANCELLED`. */
  async stop(): Promise<void> {
    this.paused = false;
    await this.settle(() => this.backend.stop());
  }

  /** Pauses the running utterance. Rejects with `ERR_TTS_NOT_SPEAKING` when nothing speaks. */
  async pause(): Promise<void> {
    if (!(await this.settle(() => this.backend.isSpeaking()))) throw speechError("ERR_TTS_NOT_SPEAKING");
    await this.settle(() => this.backend.pause());
    this.paused = true;
  }

  /** Resumes a paused utterance. Rejects with `ERR_TTS_NOT_SPEAKING` when none is paused. */
  async resume(): Promise<void> {
    if (!this.paused) throw speechError("ERR_TTS_NOT_SPEAKING");
    await this.settle(() => this.backend.resume());
    this.paused = false;
  }

  isSpeaking(): Promise<boolean> {
    return this.settle(() => this.backend.isSpeaking());
  }

  /**
   * Synthesizes `text` to WAV bytes, without speaking it. A second call while one runs rejects
   * with `ERR_TTS_BUSY`; blank text rejects with `ERR_TTS_INVALID_ARGUMENT`.
   */
  async synthesize(text: string): Promise<Uint8Array> {
    if (text.trim() === "") throw speechError("ERR_TTS_INVALID_ARGUMENT");
    if (this.synthesizing) throw speechError("ERR_TTS_BUSY");
    this.synthesizing = true;
    try {
      return await this.settle(() => this.backend.synthesize(text));
    } finally {
      this.synthesizing = false;
    }
  }

  /** Sets the language of later utterances. Blank rejects with `ERR_TTS_INVALID_ARGUMENT`. */
  async setLanguage(language: string): Promise<void> {
    if (language.trim() === "") throw speechError("ERR_TTS_INVALID_ARGUMENT");
    await this.settle(() => this.backend.setLanguage(language));
  }

  /** Sets the voice of later utterances. An unknown or uninstalled voice rejects with `ERR_TTS_VOICE_NOT_FOUND`. */
  async setVoice(voiceId: string): Promise<void> {
    const voices = await this.settle(() => this.backend.voices());
    const voice = voices.find((candidate) => candidate.id === voiceId);
    if (voice === undefined || !voice.installed) throw speechError("ERR_TTS_VOICE_NOT_FOUND");
    await this.settle(() => this.backend.setVoice(voiceId));
  }

  /** Sets the rate of later utterances (0.0 to 1.0). */
  async setSpeechRate(rate: number): Promise<void> {
    if (!(rate >= 0.0 && rate <= 1.0)) throw speechError("ERR_TTS_INVALID_ARGUMENT");
    await this.settle(() => this.backend.setRate(rate));
  }

  /** Sets the pitch of later utterances (0.5 to 2.0). */
  async setPitch(pitch: number): Promise<void> {
    if (!(pitch >= 0.5 && pitch <= 2.0)) throw speechError("ERR_TTS_INVALID_ARGUMENT");
    await this.settle(() => this.backend.setPitch(pitch));
  }

  /** Sets the volume of later utterances (0.0 to 1.0). */
  async setVolume(volume: number): Promise<void> {
    if (!(volume >= 0.0 && volume <= 1.0)) throw speechError("ERR_TTS_INVALID_ARGUMENT");
    await this.settle(() => this.backend.setVolume(volume));
  }

  /** Sets the silence before each utterance, in milliseconds; 0 starts immediately. */
  async setSilence(ms: number): Promise<void> {
    await this.settle(() => this.backend.setSilence(ms));
  }

  /** The voices the device offers; an empty list when the platform gives none. */
  voices(): Promise<Voice[]> {
    return this.settle(() => this.backend.voices());
  }

  /** The installed engines; desktops and Apple report one. */
  engines(): Promise<Engine[]> {
    return this.settle(() => this.backend.engines());
  }

  /** The languages a voice exists for, as BCP 47 tags. */
  languages(): Promise<string[]> {
    return this.settle(() => this.backend.languages());
  }

  /** Whether a voice exists for `language`. */
  async isLanguageAvailable(language: string): Promise<boolean> {
    const languages = await this.languages();
    return languages.includes(language);
  }

  /** The longest text the platform accepts; `null` when it reports no limit. */
  maxSpeechInputLength(): Promise<number | null> {
    return this.settle(() => this.backend.maxSpeechInputLength());
  }

  /** Runs one backend call, so every rejection the app sees carries a stable code. */
  private async settle<T>(call: () => Promise<T>): Promise<T> {
    try {
      return await call();
    } catch (error) {
      throw asSpeechError(error);
    }
  }

  private nextLocalId(): UtteranceId {
    this.localIds += 1;
    return this.localIds;
  }
}
