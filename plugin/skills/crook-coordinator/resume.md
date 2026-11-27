# After a resume, `/clear` or compaction

Your SessionStart hook has already run `crook resume` for every child that lost its terminal. It put a report in
your context: which children it resumed, and which results are undelivered.

## Do

1. Do not run `crook resume` again for all children. Never spawn a child that is already in the roster.
2. Rebuild your picture from three sources:
   - the flock's `README.md`;
   - `crook status <slug> --json`;
   - each active child's result checklist.
3. Tell the maintainer, briefly: which children were resumed, which are done, which are waiting.
4. Warn the maintainer of two things:
   - A child that was blocked on a question will ask it again.
   - Commands that were running at the moment of the stop were cut off part-way, so a resumed child checks its
     last step on disk before it continues.
5. If one child failed to resume, run `crook resume <child>` for that child alone.

## After `/clear` or compaction

Your skill text may be gone from the conversation. Invoke `crook-coordinator` again, then follow the steps above.
The files are the memory, so the README and `crook status` carry the state.
