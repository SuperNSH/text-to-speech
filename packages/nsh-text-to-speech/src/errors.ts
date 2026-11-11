/** The codes a speech rejection carries. Apps match on `code`. */
export type SpeechErrorCode =
  | "ERR_TTS_NOT_AVAILABLE"
  | "ERR_TTS_ENGINE_MISSING"
  | "ERR_TTS_VOICE_NOT_FOUND"
  | "ERR_TTS_LANGUAGE_NOT_SUPPORTED"
  | "ERR_TTS_LANGUAGE_MISSING_DATA"
  | "ERR_TTS_NOT_SPEAKING"
  | "ERR_TTS_INVALID_ARGUMENT"
  | "ERR_TTS_BUSY"
  | "ERR_TTS_CANCELLED"
  | "ERR_TTS_SYNTHESIS_FAILED"
  | "ERR_TTS_BACKEND";

/** Same wording as the Rust crate's messages. None holds the reason text. */
const MESSAGES: Record<SpeechErrorCode, string> = {
  ERR_TTS_NOT_AVAILABLE: "Speech is not available on this device or platform. Offer a visible fallback.",
  ERR_TTS_ENGINE_MISSING: "No text-to-speech engine is installed. Install one, then try again.",
  ERR_TTS_VOICE_NOT_FOUND: "That voice is unknown or not installed. Pick one from voices().",
  ERR_TTS_LANGUAGE_NOT_SUPPORTED: "The platform does not support that language tag. Pick one from languages().",
  ERR_TTS_LANGUAGE_MISSING_DATA: "The language's voice data needs downloading in the system settings.",
  ERR_TTS_NOT_SPEAKING: "Nothing is speaking, or nothing is paused.",
  ERR_TTS_INVALID_ARGUMENT: "An argument is empty or out of range, such as blank text or a rate past 1.0.",
  ERR_TTS_BUSY: "A synthesize is already running. Wait for its bytes.",
  ERR_TTS_CANCELLED: "The utterance was stopped before it finished.",
  ERR_TTS_SYNTHESIS_FAILED: "The platform failed to synthesize the text.",
  ERR_TTS_BACKEND: "The platform's speech service failed.",
};

const KNOWN = new Set<string>(Object.keys(MESSAGES));

/** A rejection of the speech API. `code` is the stable name; `status` is the platform's own. */
export class SpeechError extends Error {
  readonly code: SpeechErrorCode;
  readonly status?: number;

  constructor(code: SpeechErrorCode, status?: number) {
    super(MESSAGES[code]);
    this.name = "SpeechError";
    this.code = code;
    if (status !== undefined) this.status = status;
  }
}

/** The error for a known code. */
export function speechError(code: SpeechErrorCode): SpeechError {
  return new SpeechError(code);
}

/**
 * Turns whatever the native side or a backend threw into a coded `SpeechError`. A native
 * rejection carries its `code`, and a host's rejection carries `ERR_NSH_HOST_ERROR` with the
 * code first in its message, as `ERR_TTS_CANCELLED: text`. Anything unknown is a backend
 * failure.
 */
export function asSpeechError(error: unknown): SpeechError {
  if (error instanceof SpeechError) return error;
  const raw = (error ?? {}) as { code?: unknown; message?: unknown; status?: unknown };
  const code = typeof raw.code === "string" ? raw.code : "";
  const message = typeof raw.message === "string" ? raw.message : "";
  const status = typeof raw.status === "number" ? raw.status : undefined;
  const known = (candidate: string): SpeechError | null =>
    KNOWN.has(candidate) ? new SpeechError(candidate as SpeechErrorCode, status) : null;
  if (code === "ERR_NSH_HOST_ERROR") {
    const named = /^(ERR_TTS_[A-Z_]+): /.exec(message)?.[1] ?? "";
    return known(named) ?? speechError("ERR_TTS_BACKEND");
  }
  return known(code) ?? speechError("ERR_TTS_BACKEND");
}
