import type { Engine, UtteranceEvent, UtteranceId, Voice } from "./options";

/** What a `Speech` runs on: the Rust crate, or a memory backend in tests. */
export interface SpeechBackend {
  /** Prepares the platform's speech. Rejects when no engine exists or speech cannot run. */
  init(): Promise<void>;
  isAvailable(): Promise<boolean>;
  /**
   * Speaks `text`: `interrupt` cancels the running utterance first, and otherwise the utterance
   * queues behind it. The utterance's events reach `onEvent`, the earliest ones included, and
   * the call resolves with its id, or null when the platform reports none.
   */
  speak(text: string, interrupt: boolean, onEvent: (event: UtteranceEvent) => void): Promise<UtteranceId | null>;
  /** Cancels the running utterance and the queued ones; each fires a cancel event. */
  stop(): Promise<void>;
  pause(): Promise<void>;
  resume(): Promise<void>;
  isSpeaking(): Promise<boolean>;
  synthesize(text: string): Promise<Uint8Array>;
  voices(): Promise<Voice[]>;
  engines(): Promise<Engine[]>;
  languages(): Promise<string[]>;
  maxSpeechInputLength(): Promise<number | null>;
  setLanguage(language: string): Promise<void>;
  setVoice(voiceId: string): Promise<void>;
  setRate(rate: number): Promise<void>;
  setPitch(pitch: number): Promise<void>;
  setVolume(volume: number): Promise<void>;
  setSilence(ms: number): Promise<void>;
}
