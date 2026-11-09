// Plan for Text to speech in this repo. Phases run in order for each area:
// implement, audit, unit tests, device test, vision, smoke.
// Gates on every area: cargo fmt --check, cargo clippy -D warnings, cargo test.

export type Phase = "implement" | "audit" | "unit-tests" | "device-test" | "vision" | "smoke";

export interface Area {
  name: string;
  phases: Phase[];
  status: Record<Phase, "not-started" | "done" | "blocked">;
}

export const workflow = {
  repo: "SuperNSH/text-to-speech",
  gates: [
    "cargo fmt --all -- --check",
    "cargo clippy --all-targets -- -D warnings",
    "cargo test --all-targets",
  ],
  areas: [
    {
      name: "text-to-speech",
      phases: ["implement", "audit", "unit-tests", "device-test", "vision", "smoke"],
      status: {
        "implement": "not-started",
        "audit": "not-started",
        "unit-tests": "not-started",
        "device-test": "not-started",
        "vision": "not-started",
        "smoke": "not-started",
      },
    },
  ] satisfies Area[],
};
