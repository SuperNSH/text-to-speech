import { expect, test } from "nsh/test";
import { memoryBackend, Speech, speechError, type SpeechBackend } from "../src";

/** Lets a backend answer and the events it fires reach the listeners, which run in later turns. */
async function delivered(): Promise<void> {
  for (let turn = 0; turn < 4; turn += 1) await Promise.resolve();
}

function remember(speech: Speech): string[] {
  const seen: string[] = [];
  speech.addEventListener("tts-start", (event) => seen.push(`start ${event.utteranceId}`));
  speech.addEventListener("tts-finish", (event) => seen.push(`finish ${event.utteranceId}`));
  speech.addEventListener("tts-cancel", (event) => seen.push(`cancel ${event.utteranceId}`));
  return seen;
}

test("speaks_and_finishes_once", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const seen = remember(speech);
  const id = await speech.speak("Hello from Native Shell");
  expect(seen).toEqual([`start ${id}`]);
  expect(backend.finishUtterance()).toBe(true);
  await delivered();
  expect(seen).toEqual([`start ${id}`, `finish ${id}`]);
  expect(backend.finishUtterance()).toBe(false);
  await delivered();
  expect(seen).toEqual([`start ${id}`, `finish ${id}`]);
});

test("speak_rejects_empty_text", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await expect(speech.speak("")).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  expect(backend.speaks()).toBe(0);
});

test("speak_rejects_blank_text", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await expect(speech.speak("  \n\t ")).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  expect(backend.speaks()).toBe(0);
});

test("speak_rejects_oversize_text", async () => {
  const backend = memoryBackend({ maxSpeechInputLength: 10 });
  const speech = new Speech({ backend });
  const id = await speech.speak("a".repeat(10));
  expect(typeof id).toBe("number");
  await expect(speech.speak("a".repeat(11))).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  expect(backend.speaks()).toBe(1);
});

test("speak_queues_while_speaking", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const seen = remember(speech);
  const first = await speech.speak("First");
  const second = await speech.speak("Second", false);
  expect(second).not.toBe(first);
  expect(seen).toEqual([`start ${first}`]);
  expect(backend.finishUtterance()).toBe(true);
  await delivered();
  expect(seen).toEqual([`start ${first}`, `finish ${first}`, `start ${second}`]);
  expect(backend.finishUtterance()).toBe(true);
  await delivered();
  expect(seen).toEqual([`start ${first}`, `finish ${first}`, `start ${second}`, `finish ${second}`]);
});

test("speak_interrupts_current_utterance", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const seen = remember(speech);
  speech.awaitSpeakCompletion(true);
  const first = speech.speak("First");
  first.catch(() => {});
  await delivered();
  const second = speech.speak("Second");
  await expect(first).rejects.toMatchObject({ code: "ERR_TTS_CANCELLED" });
  expect(backend.interrupts()).toBe(1);
  await delivered();
  expect(seen).toEqual(["start 1", "cancel 1", "start 2"]);
  expect(backend.finishUtterance()).toBe(true);
  expect(await second).toBe(2);
});

test("awaited_speak_resolves_once_on_finish", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  let finished = -1;
  speech.addEventListener("tts-finish", (event) => {
    finished = event.utteranceId;
  });
  speech.awaitSpeakCompletion(true);
  const call = speech.speak("Hello");
  await delivered();
  backend.finishUtterance();
  expect(await call).toBe(finished);
  backend.failUtterance("ERR_TTS_SYNTHESIS_FAILED");
  expect(backend.finishUtterance()).toBe(false);
  const next = speech.speak("Again");
  await delivered();
  backend.finishUtterance();
  expect(await next).toBe(2);
});

test("stop_cancels_awaited_speak_once", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const seen = remember(speech);
  speech.awaitSpeakCompletion(true);
  const call = speech.speak("Hello");
  call.catch(() => {});
  await delivered();
  await speech.stop();
  await expect(call).rejects.toMatchObject({ code: "ERR_TTS_CANCELLED" });
  expect(seen).toEqual(["start 1", "cancel 1"]);
  expect(backend.stops()).toBe(1);
  expect(backend.finishUtterance()).toBe(false);
  await speech.stop();
  expect(backend.stops()).toBe(2);
});

test("pause_and_resume_round_trip", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await speech.speak("Hello");
  await speech.pause();
  await speech.resume();
  expect(backend.pauses()).toBe(1);
  expect(backend.resumes()).toBe(1);
  expect(await speech.isSpeaking()).toBe(true);
});

test("pause_when_idle_reports_not_speaking", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await expect(speech.pause()).rejects.toMatchObject({ code: "ERR_TTS_NOT_SPEAKING" });
  expect(backend.pauses()).toBe(0);
});

test("resume_when_not_paused_reports_not_speaking", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await speech.speak("Hello");
  await expect(speech.resume()).rejects.toMatchObject({ code: "ERR_TTS_NOT_SPEAKING" });
  expect(backend.resumes()).toBe(0);
});

test("set_rate_rejects_out_of_range", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  for (const rate of [-0.1, 1.01, Number.NaN]) {
    await expect(speech.setSpeechRate(rate)).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  }
  expect(backend.lastRate()).toBeUndefined();
  await speech.setSpeechRate(0.5);
  expect(backend.lastRate()).toBe(0.5);
  await speech.setSpeechRate(0);
  await speech.setSpeechRate(1);
  expect(backend.lastRate()).toBe(1);
});

test("set_pitch_rejects_out_of_range", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  for (const pitch of [0.4, 2.1, Number.NaN]) {
    await expect(speech.setPitch(pitch)).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  }
  expect(backend.lastPitch()).toBeUndefined();
  await speech.setPitch(1);
  expect(backend.lastPitch()).toBe(1);
  await speech.setPitch(0.5);
  await speech.setPitch(2);
  expect(backend.lastPitch()).toBe(2);
});

test("set_volume_rejects_out_of_range", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  for (const volume of [-0.1, 1.1, Number.NaN]) {
    await expect(speech.setVolume(volume)).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  }
  expect(backend.lastVolume()).toBeUndefined();
  await speech.setVolume(0.75);
  expect(backend.lastVolume()).toBe(0.75);
  await speech.setVolume(0);
  await speech.setVolume(1);
  expect(backend.lastVolume()).toBe(1);
});

test("set_language_rejects_empty", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await expect(speech.setLanguage("")).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  await expect(speech.setLanguage("   ")).rejects.toMatchObject({ code: "ERR_TTS_INVALID_ARGUMENT" });
  expect(backend.lastLanguage()).toBeUndefined();
  await speech.setLanguage("en-US");
  expect(backend.lastLanguage()).toBe("en-US");
});

test("unknown_language_reports_unavailable", async () => {
  const backend = memoryBackend({ languages: ["en-US", "de-DE"] });
  const speech = new Speech({ backend });
  expect(await speech.isLanguageAvailable("en-US")).toBe(true);
  expect(await speech.isLanguageAvailable("fr-FR")).toBe(false);
});

test("set_unknown_voice_reports_voice_not_found", async () => {
  const backend = memoryBackend({ voices: [{ id: "v1", name: "Samantha", language: "en-US", networkRequired: false, installed: true }] });
  const speech = new Speech({ backend });
  await expect(speech.setVoice("nope")).rejects.toMatchObject({ code: "ERR_TTS_VOICE_NOT_FOUND" });
  expect(backend.lastVoice()).toBeUndefined();
  await speech.setVoice("v1");
  expect(backend.lastVoice()).toBe("v1");
});

test("uninstalled_voice_reports_voice_not_found", async () => {
  const backend = memoryBackend({ voices: [{ id: "v2", name: "Remote", language: "en-US", networkRequired: true, installed: false }] });
  const speech = new Speech({ backend });
  await expect(speech.setVoice("v2")).rejects.toMatchObject({ code: "ERR_TTS_VOICE_NOT_FOUND" });
  expect(backend.lastVoice()).toBeUndefined();
});

test("init_reports_missing_engine", async () => {
  const speech = new Speech({ backend: memoryBackend({ initError: "ERR_TTS_ENGINE_MISSING" }) });
  await expect(speech.init()).rejects.toMatchObject({ code: "ERR_TTS_ENGINE_MISSING" });
});

test("backend_failure_carries_status", async () => {
  const speech = new Speech({ backend: memoryBackend({ speakError: { code: "ERR_TTS_BACKEND", status: 42 } }) });
  await expect(speech.speak("Hello")).rejects.toMatchObject({ code: "ERR_TTS_BACKEND", status: 42 });
});

test("concurrent_synthesize_reports_busy", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const first = speech.synthesize("One");
  const second = speech.synthesize("Two");
  second.catch(() => {});
  await expect(second).rejects.toMatchObject({ code: "ERR_TTS_BUSY" });
  await first;
  const again = await speech.synthesize("Three");
  expect(again.length).toBeGreaterThan(0);
});

test("synthesize_returns_wav_bytes", async () => {
  const speech = new Speech({ backend: memoryBackend() });
  const bytes = await speech.synthesize("Hello");
  expect(Array.from(bytes.slice(0, 4))).toEqual([82, 73, 70, 70]);
  expect(Array.from(bytes.slice(8, 12))).toEqual([87, 65, 86, 69]);
});

test("progress_events_report_location", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  const seen: unknown[] = [];
  speech.addEventListener("tts-progress", (event) => seen.push(event));
  const id = await speech.speak("Hello");
  backend.emitProgress(120, 480);
  await delivered();
  expect(seen).toEqual([{ utteranceId: id, kind: { type: "progress", location: 120, length: 480 } }]);
});

test("set_silence_reaches_backend", async () => {
  const backend = memoryBackend();
  const speech = new Speech({ backend });
  await speech.setSilence(250);
  expect(backend.lastSilence()).toBe(250);
  await speech.setSilence(0);
  expect(backend.lastSilence()).toBe(0);
});

test("error_codes_are_stable_and_unique", async () => {
  const speech = new Speech({ backend: memoryBackend() });
  const failing = new Speech({ backend: memoryBackend({ speakError: { code: "ERR_TTS_SYNTHESIS_FAILED", status: 3 } }) });
  const stopping = new Speech({ backend: memoryBackend() });
  stopping.awaitSpeakCompletion(true);
  const stopped = stopping.speak("Hello");
  stopped.catch(() => {});
  await delivered();
  await stopping.stop();
  const first = speech.synthesize("One");
  const busy = speech.synthesize("Two");
  busy.catch(() => {});
  await first;

  const seen: string[] = [];
  const failures: Array<[string, Promise<unknown>]> = [
    ["ERR_TTS_INVALID_ARGUMENT", speech.speak("")],
    ["ERR_TTS_NOT_SPEAKING", speech.pause()],
    ["ERR_TTS_VOICE_NOT_FOUND", speech.setVoice("none")],
    ["ERR_TTS_ENGINE_MISSING", new Speech({ backend: memoryBackend({ initError: "ERR_TTS_ENGINE_MISSING" }) }).init()],
    ["ERR_TTS_SYNTHESIS_FAILED", failing.speak("Hello")],
    ["ERR_TTS_BUSY", busy],
    ["ERR_TTS_CANCELLED", stopped],
  ];
  // Each rejects now, before its turn below: marked, so none is an unhandled rejection.
  for (const [, failure] of failures) failure.catch(() => {});
  for (const [code, failure] of failures) {
    await expect(failure).rejects.toMatchObject({ code });
    seen.push(code);
  }
  for (const code of [
    "ERR_TTS_NOT_AVAILABLE",
    "ERR_TTS_LANGUAGE_NOT_SUPPORTED",
    "ERR_TTS_LANGUAGE_MISSING_DATA",
    "ERR_TTS_BACKEND",
  ] as const) {
    expect(speechError(code).code).toBe(code);
    seen.push(code);
  }
  expect(seen.length).toBe(11);
  expect(new Set(seen).size).toBe(11);
});

test("missing_voice_list_is_empty", async () => {
  const speech = new Speech({ backend: memoryBackend({ voices: [] }) });
  expect(await speech.voices()).toEqual([]);
});

test("backend_rejections_carry_codes", async () => {
  const memory = memoryBackend();
  const backend: SpeechBackend = {
    ...memory,
    setLanguage: () => Promise.reject(new Error("wedge")),
    setRate: () => Promise.reject(new Error("wedge")),
    voices: () => Promise.reject(new Error("wedge")),
    isSpeaking: () => Promise.reject(new Error("wedge")),
  };
  const speech = new Speech({ backend });
  await expect(speech.setLanguage("en-US")).rejects.toMatchObject({ code: "ERR_TTS_BACKEND" });
  await expect(speech.setSpeechRate(0.5)).rejects.toMatchObject({ code: "ERR_TTS_BACKEND" });
  await expect(speech.voices()).rejects.toMatchObject({ code: "ERR_TTS_BACKEND" });
  await expect(speech.isSpeaking()).rejects.toMatchObject({ code: "ERR_TTS_BACKEND" });
});
