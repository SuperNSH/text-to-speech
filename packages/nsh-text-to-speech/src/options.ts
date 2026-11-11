import type { SpeechError } from "./errors";

/** Identifies one utterance: what `speak` resolved with, and what its events carry. */
export type UtteranceId = number;

/** A voice the device can speak with, as `voices` reports it. */
export interface Voice {
  /** The id `setVoice` takes. */
  id: string;
  /** Its name, as the system settings show it. */
  name: string;
  /** Its language, a BCP 47 tag (`en-US`). */
  language: string;
  /** Whether it needs the network; such a voice fails offline. */
  networkRequired: boolean;
  /** Whether it is installed. An uninstalled voice cannot be set. */
  installed: boolean;
}

/** A speech engine, as `engines` reports it. Android has several; other platforms report one. */
export interface Engine {
  /** Its package name (`com.google.android.tts`). */
  name: string;
  /** Its display name. */
  label: string;
  /** Whether the system picks it by default. */
  default: boolean;
}

/** What happened to one utterance. */
export type UtteranceKind =
  | { type: "start" }
  | { type: "progress"; location: number; length: number }
  | { type: "finish" }
  | { type: "cancel" }
  | { type: "error"; error: SpeechError };

/** One utterance event: which utterance, and what happened to it. */
export interface UtteranceEvent {
  utteranceId: UtteranceId;
  kind: UtteranceKind;
}

/** react-native-tts's event names, which `addEventListener` takes. */
export type UtteranceEventName = "tts-start" | "tts-progress" | "tts-finish" | "tts-cancel" | "tts-error";
