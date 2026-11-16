---
maturity: growing
type: lessons
updated: 2026-11-16
---

# Lessons Learned

Insights, failed approaches, rework, and blockers, captured blamelessly
so the team replicates what worked and avoids what did not.

## Active index

| ID | Status | Title | Date |
|----|--------|-------|------|
| LL-001 | actioned | Worktrees must not share one CARGO_TARGET_DIR | 2026-11-16 |
| LL-002 | actioned | Branches that merge cleanly as text can fail as code | 2026-11-16 |
| LL-003 | actioned | Thread-scoped tracing subscribers race in parallel unit tests | 2026-11-16 |
| LL-004 | actioned | Review summaries need evidence for every clean claim | 2026-11-16 |

## Lessons

## LL-001: Worktrees must not share one CARGO_TARGET_DIR

- Status: actioned
- Date: 2026-11-16
- Phase: review
- Category: tooling
- Owner: -
- Links: PR #33
<!-- key: ac61949dc36a -->

### What happened

Eight parallel fix agents for PR #33 needed cargo builds with 17G of disk free; one shared target dir looked like the way to fit them.

### What did not work

A pilot built q2-shared in worktree 1, then ran the same command in worktree 2: worktree 2 compiled nothing and reused worktree 1's build. With differing sources, a test binary could come from the other tree.

### Root cause

Not traced inside cargo; observed that the second worktree's unchanged member counted as fresh against the first worktree's artifacts.

### Recommendation / action item

Each worktree gets its own target with CARGO_INCREMENTAL=0 and CARGO_PROFILE_DEV_DEBUG=0 (about 400M per full workspace test build). Pilot any shared-cache idea with one cheap build before fanning out.

## LL-002: Branches that merge cleanly as text can fail as code

- Status: actioned
- Date: 2026-11-16
- Phase: review
- Category: process
- Owner: -
- Links: PR #33, 9cdb5e7
<!-- key: 9cccd7e8ccaa -->

### What happened

The render and core fix groups both edited crates/q2-shared/tests/workspace_invariants.rs. git merged them with no conflict.

### What did not work

Core had replaced the crates_dir() helper with member_dirs(); render's new test still called crates_dir(), so the merged tree did not compile.

### Root cause

A textual merge checks overlapping lines, not the names one side removed and the other side still uses.

### Recommendation / action item

Run at least the touched crates' tests after every merge of parallel branches, and make ci before pushing; the break was caught that way and fixed in 9cdb5e7.

## LL-003: Thread-scoped tracing subscribers race in parallel unit tests

- Status: actioned
- Date: 2026-11-16
- Phase: review
- Category: testing
- Owner: -
- Links: PR #33, 9c687a0
<!-- key: 208a417be711 -->

### What happened

A test capturing gi.dprintf and cprintf logs with tracing::subscriber::with_default failed about 1 run in 8 (4/30), always missing the cprintf line.

### What did not work

Keeping the test among the other q2-server unit tests: those call cprintf on threads with no subscriber at the same time.

### Root cause

tracing caches each log site's enabled state process-wide; a site first hit on a subscriber-less thread while this test's scoped subscriber registered could be cached as disabled.

### Recommendation / action item

Tests that capture logs from sites other tests also hit run in their own integration-test binary (crates/q2-server/tests/game_print_log.rs): 0/50 after the move. Measure the failure rate before and after a flake fix.

## LL-004: Review summaries need evidence for every clean claim

- Status: actioned
- Date: 2026-11-16
- Phase: review
- Category: review
- Owner: -
- Links: PR #33 round-3 and round-4 summaries
<!-- key: 6fc8feafd7ad -->

### What happened

The round-3 summary first stated a prior-fix tally and a clean duplication result, then said the duplicated flash case labels mirror C.

### What did not work

The tally counted bullet lines, not finding ids; the duplication claim had no scan behind it; C has one CL_AddMuzzleFlash2 switch, so the split was the port's own (round 4, r4-simplify-1).

### Root cause

Summary lines were written from agent reports without re-running the check they describe.

### Recommendation / action item

Every clean or count claim in a posted review cites the command that produced it; the round-3 summary was corrected in place and in round 4.

## Archive

Superseded or deprecated entries sink here; nothing is deleted (git keeps history).

<!-- ENTRY TEMPLATE: copy a block into the Lessons section above the Archive
heading, assign the next LL-NNN id, and fill it in. The journal_append helper
does this automatically; this block is the fallback for hand-editing.

## LL-NNN: <short lesson title>

- Status: open
- Date: YYYY-MM-DD
- Phase: execute | review
- Category: process | technology | requirements | testing | communication
- Owner: <who carries the follow-up>
- Links: <PR/commit/issue>, <related TR-NNN>

### What happened

<blameless, factual: the situation/activity>

### What went well / where we got lucky

<successes worth replicating>

### What did not work

<the gap or failure>

### Root cause

<5 Whys / contributing factors>

### Recommendation / action item

- Action: <specific change>. Owner: <name>. Due: <date>. Status: <...>
-->
