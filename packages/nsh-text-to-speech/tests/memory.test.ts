import { expect, test } from "nsh/test";
import { memoryBackend, type UtteranceEvent, type Voice } from "../src";

const VOICES: Voice[] = [
  { id: "v1", name: "Samantha", language: "en-US", networkRequired: false, installed: true },
  { id: "v2", name: "Remote", language: "en-US", networkRequired: true, installed: false },
];

/** The raw events a backend fires, before `Speech` routes them to listeners. */
function recorder(backend: ReturnType<typeof memoryBackend>): UtteranceEvent[] {
  const seen: UtteranceEvent[] = [];
  backend.speak("Hello", true, (event) => seen.push(event));
  return seen;
}

test("speak fires start and answers a fresh id", () => {
  const backend = memoryBackend();
  const seen = recorder(backend);
  expect(seen.map((event) => event.kind.type)).toEqual(["start"]);
  const next = recorder(backend);
  expect(next[0].utteranceId).toBe(seen[0].utteranceId + 1);
});

test("finishUtterance fires finish for the running utterance and starts the queued one", () => {
  const backend = memoryBackend();
  const firstSeen: UtteranceEvent[] = [];
  const secondSeen: UtteranceEvent[] = [];
  backend.speak("First", true, (event) => firstSeen.push(event));
  backend.speak("Second", false, (event) => secondSeen.push(event));
  expect(secondSeen).toEqual([]);
  expect(backend.finishUtterance()).toBe(true);
  expect(firstSeen.map((event) => event.kind.type)).toEqual(["start", "finish"]);
  expect(secondSeen.map((event) => event.kind.type)).toEqual(["start"]);
  expect(backend.finishUtterance()).toBe(true);
  expect(backend.finishUtterance()).toBe(false);
});

test("interrupt fires cancel on the running utterance and starts the new one at once", async () => {
  const backend = memoryBackend();
  const firstSeen: UtteranceEvent[] = [];
  backend.speak("First", true, (event) => firstSeen.push(event));
  const secondId = await backend.speak("Second", true, (event) => firstSeen.push(event));
  expect(backend.interrupts()).toBe(1);
  expect(firstSeen.map((event) => [event.utteranceId, event.kind.type])).toEqual([
    [1, "start"],
    [1, "cancel"],
    [2, "start"],
  ]);
  expect(secondId).toBe(2);
});

test("stop fires cancel on the running and the queued utterances", async () => {
  const backend = memoryBackend();
  const seen: UtteranceEvent[] = [];
  backend.speak("First", false, (event) => seen.push(event));
  backend.speak("Second", false, (event) => seen.push(event));
  seen.length = 0;
  backend.stop();
  expect(seen.map((event) => [event.utteranceId, event.kind.type])).toEqual([
    [1, "cancel"],
    [2, "cancel"],
  ]);
  expect(await backend.isSpeaking()).toBe(false);
});

test("emitProgress and failUtterance fire through the running utterance", async () => {
  const backend = memoryBackend();
  const seen: UtteranceEvent[] = [];
  backend.speak("Hello", true, (event) => seen.push(event));
  backend.emitProgress(5, 10);
  expect(backend.failUtterance("ERR_TTS_LANGUAGE_MISSING_DATA", 2)).toBe(true);
  expect(seen[1]).toMatchObject({ kind: { type: "progress", location: 5, length: 10 } });
  expect(seen[2].kind.type).toBe("error");
  expect(seen.length).toBe(3);
  expect(await backend.isSpeaking()).toBe(false);
});

test("the scripted answers read back as configured", async () => {
  const backend = memoryBackend({
    available: false,
    voices: VOICES,
    engines: [{ name: "com.google.android.tts", label: "Google TTS", default: true }],
    languages: ["en-US"],
    maxSpeechInputLength: 4000,
  });
  expect(await backend.isAvailable()).toBe(false);
  expect(await backend.voices()).toEqual(VOICES);
  expect(await backend.engines()).toEqual([{ name: "com.google.android.tts", label: "Google TTS", default: true }]);
  expect(await backend.languages()).toEqual(["en-US"]);
  expect(await backend.maxSpeechInputLength()).toBe(4000);
  backend.setVoices([]);
  expect(await backend.voices()).toEqual([]);
  const bare = memoryBackend();
  expect(await bare.maxSpeechInputLength()).toBe(null);
});

test("init and synthesize answer the scripts", async () => {
  const failing = memoryBackend({ initError: "ERR_TTS_NOT_AVAILABLE", synthesizeError: "ERR_TTS_SYNTHESIS_FAILED" });
  await expect(failing.init()).rejects.toMatchObject({ code: "ERR_TTS_NOT_AVAILABLE" });
  await expect(failing.synthesize("Hello")).rejects.toMatchObject({ code: "ERR_TTS_SYNTHESIS_FAILED" });
  const working = memoryBackend();
  await working.init();
  expect((await working.synthesize("Hello")).length).toBeGreaterThan(0);
});

test("the counters and last values record what reached the backend", async () => {
  const backend = memoryBackend();
  backend.speak("Hello", true, () => {});
  await backend.pause();
  await backend.resume();
  await backend.setLanguage("en-US");
  await backend.setVoice("v9");
  await backend.setRate(0.5);
  await backend.setPitch(1.25);
  await backend.setVolume(0.75);
  await backend.setSilence(300);
  expect(backend.speaks()).toBe(1);
  expect(backend.pauses()).toBe(1);
  expect(backend.resumes()).toBe(1);
  expect(backend.lastLanguage()).toBe("en-US");
  expect(backend.lastVoice()).toBe("v9");
  expect(backend.lastRate()).toBe(0.5);
  expect(backend.lastPitch()).toBe(1.25);
  expect(backend.lastVolume()).toBe(0.75);
  expect(backend.lastSilence()).toBe(300);
});
