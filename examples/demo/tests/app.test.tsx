// The demo's screen on the memory engine: speaking settles through its
// events, blank text refuses with its code, utterances interrupt, queue and
// cancel, pause holds one, synthesize answers WAV bytes, and the voice list
// picks a voice or refuses one not installed. No real engine, no audio.
import { expect, open, test } from "nsh/test";

/** The demo opened with its whole column on screen, ready on the memory engine. */
async function demo() {
  const app = await open("../App.tsx", { width: 420, height: 1800 });
  await app.waitFor("text:engine ready");
  return app;
}

test("speaking fires start and finish, and the status settles", async () => {
  const app = await demo();
  await app.press("id:speak");
  await app.waitFor("text:start 1");
  await app.waitFor("text:finish 1");
  await app.waitFor("text:finished 1");
});

test("blank text refuses with ERR_TTS_INVALID_ARGUMENT and fires nothing", async () => {
  const app = await demo();
  await app.press("id:clear");
  await app.press("id:speak");
  await app.waitFor("text:ERR_TTS_INVALID_ARGUMENT");
  await app.waitFor("text:nothing yet");
});

test("a second speak in interrupt mode cancels the running utterance", async () => {
  const app = await demo();
  await app.press("id:speak");
  await app.waitFor("text:start 1");
  await app.press("id:speak");
  await app.waitFor("text:cancel 1");
  await app.waitFor("text:start 2");
  await app.waitFor("text:finish 2");
});

test("queue mode speaks one utterance after the other", async () => {
  const app = await demo();
  await app.press("id:mode");
  await app.waitFor("text:Queues");
  await app.press("id:speak");
  await app.press("id:speak");
  await app.waitFor("text:start 1");
  await app.waitFor("text:finish 1");
  await app.waitFor("text:start 2");
  await app.waitFor("text:finish 2");
});

test("stop cancels the running utterance", async () => {
  const app = await demo();
  await app.press("id:speak");
  await app.waitFor("text:start 1");
  await app.press("id:stop");
  await app.waitFor("text:cancel 1");
  await app.waitFor("text:stopped");
});

test("pause holds the utterance and resume lets it finish", async () => {
  const app = await demo();
  await app.press("id:speak");
  await app.waitFor("text:start 1");
  await app.press("id:pause");
  await app.waitFor("text:paused");
  await app.press("id:resume");
  await app.waitFor("text:finish 1");
});

test("synthesize answers WAV bytes without speaking", async () => {
  const app = await demo();
  await app.press("id:synthesize");
  await app.waitFor("text:synthesized 44 bytes");
  await app.waitFor("text:44 bytes of WAV");
  expect(app.texts().includes("start 1")).toBe(false);
});

test("the voice list sets a voice and refuses one not installed", async () => {
  const app = await demo();
  await app.waitFor("id:voice-memory-voice");
  await app.press("id:voice-memory-voice");
  await app.waitFor("text:voice memory-voice");
  await app.press("id:voice-memory-remote");
  await app.waitFor("text:ERR_TTS_VOICE_NOT_FOUND");
});
