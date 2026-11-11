import { expect, test } from "nsh/test";
import type { NativeUtteranceEvent } from "nsh-text-to-speech/native";
import { bufferEarly, takeHeld } from "../src/rust";

/** A synthetic native event: the crate's handler delivers exactly this shape. */
function event(utteranceId: number, kind: string, extra: Partial<NativeUtteranceEvent> = {}): NativeUtteranceEvent {
  return { utteranceId, kind, ...extra };
}

// The two rules below are the tripwires for the adapter's one hang path: a terminal that
// arrived before speak registered its sink must survive the buffer and end the utterance
// on replay, or an awaited speak never settles.
test("overfull_early_events_rotate_out_only_non_terminals", () => {
  const early: NativeUtteranceEvent[] = [];
  const finish = event(1, "finish");
  bufferEarly(early, finish);
  for (let id = 2; id < 44; id += 1) bufferEarly(early, event(id, "progress", { location: 1, length: 2 }));
  expect(early.length).toBe(32);
  expect(early[0]).toBe(finish);
  expect(early.filter((raw) => raw.kind === "finish")).toEqual([finish]);
});

test("replayed_events_report_the_utterance_ended", () => {
  const early: NativeUtteranceEvent[] = [];
  bufferEarly(early, event(7, "start"));
  bufferEarly(early, event(8, "start"));
  bufferEarly(early, event(7, "progress", { location: 3, length: 9 }));
  const open = takeHeld(early, 7);
  expect(open.held.map((raw) => raw.kind)).toEqual(["start", "progress"]);
  expect(open.ended).toBe(false);
  expect(early.map((raw) => raw.utteranceId)).toEqual([8]);

  // A fast utterance can finish before speak's promise resolves: its terminal is buffered
  // with the rest, and the replay must report the utterance ended so the sink goes.
  const fast: NativeUtteranceEvent[] = [];
  bufferEarly(fast, event(9, "start"));
  bufferEarly(fast, event(9, "finish"));
  const ended = takeHeld(fast, 9);
  expect(ended.held.map((raw) => raw.kind)).toEqual(["start", "finish"]);
  expect(ended.ended).toBe(true);
  expect(fast).toEqual([]);
});
