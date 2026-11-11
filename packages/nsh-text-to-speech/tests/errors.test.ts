import { expect, test } from "nsh/test";
import { asSpeechError, speechError, SpeechError, type SpeechErrorCode } from "../src";

const CODES: SpeechErrorCode[] = [
  "ERR_TTS_NOT_AVAILABLE",
  "ERR_TTS_ENGINE_MISSING",
  "ERR_TTS_VOICE_NOT_FOUND",
  "ERR_TTS_LANGUAGE_NOT_SUPPORTED",
  "ERR_TTS_LANGUAGE_MISSING_DATA",
  "ERR_TTS_NOT_SPEAKING",
  "ERR_TTS_INVALID_ARGUMENT",
  "ERR_TTS_BUSY",
  "ERR_TTS_CANCELLED",
  "ERR_TTS_SYNTHESIS_FAILED",
  "ERR_TTS_BACKEND",
];

test("the eleven code strings are the stable surface", () => {
  expect(CODES.length).toBe(11);
  for (const code of CODES) {
    const error = speechError(code);
    expect(error instanceof SpeechError).toBe(true);
    expect(error.code).toBe(code);
    expect(error.message).not.toBe("");
    expect(error.name).toBe("SpeechError");
  }
});

test("each code has its own message", () => {
  const messages = CODES.map((code) => speechError(code).message);
  expect(new Set(messages).size).toBe(11);
});

test("asSpeechError keeps a code and a status, and wraps anything else as a backend failure", () => {
  expect(asSpeechError(speechError("ERR_TTS_BUSY")).code).toBe("ERR_TTS_BUSY");
  expect(asSpeechError({ code: "ERR_TTS_VOICE_NOT_FOUND", message: "x" }).code).toBe("ERR_TTS_VOICE_NOT_FOUND");
  expect(asSpeechError({ code: "ERR_TTS_BACKEND", message: "x", status: 7 }).status).toBe(7);
  expect(asSpeechError({ code: "ERR_NSH_HOST_ERROR", message: "ERR_TTS_CANCELLED: stopped" }).code).toBe(
    "ERR_TTS_CANCELLED",
  );
  expect(asSpeechError(new Error("boom")).code).toBe("ERR_TTS_BACKEND");
  expect(asSpeechError(undefined).code).toBe("ERR_TTS_BACKEND");
});
