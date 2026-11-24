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

## Releasing

Add a `## [X.Y.Z] - YYYY-MM-DD` entry at the top of `CHANGELOG.md` and push to `master`. The Changelog Release workflow bumps the versions, tags `vX.Y.Z`, creates the GitHub release with that entry as its notes and publishes to crates.io. Do not bump versions or push tags by hand. A crates.io version cannot be replaced, so CI must be green before the entry lands. To finish a release that stopped halfway: `gh workflow run changelog-release.yml`, or `gh workflow run publish.yml -f tag=vX.Y.Z` for an older tag.
