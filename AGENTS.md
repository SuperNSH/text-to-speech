# AGENTS.md

Hard rules for humans and coding agents in this repo.

- Never build, test or run emulators or simulators on a developer's machine. GitHub Actions compiles and tests everything. Commit, push to `master`, and read the results with `gh run`.
- The main branch is `master`.
- Commit small changes with a short conventional message. No attribution lines.
- Comments are 1-2 lines and state a non-obvious constraint.
- Docs read like a competent human wrote them: direct, concrete, no marketing filler.
- Dead code is deleted. No speculative abstractions.
- Every behaviour change needs a test. A test is seen to fail once before it counts.
- An operation resolves exactly once: success, a coded failure, or a cancel. It never hangs.
- Status claims in README and docs must match what CI runs. A claim with no passing run is unverified.
