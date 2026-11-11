export { type SpeechBackend } from "./backend";
export { SpeechError, asSpeechError, speechError, type SpeechErrorCode } from "./errors";
export { memoryBackend, type MemorySpeech, type MemorySpeechConfig } from "./memory";
export type { Engine, UtteranceEvent, UtteranceEventName, UtteranceId, UtteranceKind, Voice } from "./options";
export { rustBackend } from "./rust";
export { Speech, type SpeechOptions } from "./speech";
