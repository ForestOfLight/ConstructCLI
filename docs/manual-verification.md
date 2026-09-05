# Manual verification

Things automated tests cannot settle. Stage 1 and 2 items; §16 of the design
spec holds the full list.

- [x] `construct worlds` finds every world the launcher shows, with matching names.
- [x] `construct list` on a world with structures matches what Construct shows in-game.
- [x] **Locked world:** load a world in Minecraft, leave it running, then
      `construct list <that world>`. It must print the snapshot notice and still
      list structures. POSIX `fcntl` locks are per-process, so this cannot be
      tested from inside the test binary — only a second process proves it.
- [x] **World left unchanged:** run `construct list` and `construct export`
      against a world, then confirm the world's `db/` directory is unchanged
      and the world still loads in Minecraft. Byte-identity across every file
      in `db/` has already been verified programmatically; this step is about
      confirming the game itself is still happy with the world afterwards.
- [x] **An exported `.mcstructure` loads in a structure block.** Confirmed in
      Minecraft on 2026-09-02. Byte-identity between the exported file and the
      database value was already proven by tests; this closes the remaining
      gap, since only the game could confirm the file is actually usable.
      `construct export` therefore replaces the holoprint upload workflow
      end to end.
- [ ] *(Windows, needs real hardware)* GDK worlds under
      `%appdata%\Minecraft Bedrock\Users\<account>\...` are discovered, and the
      qualified reference includes the account segment.
- [ ] *(Windows)* Files written into the GDK folder by an ordinary process read
      back in-game. Expected to be a non-issue — the ACL problem was specific to
      UWP's `LocalState` inside an AppContainer, and GDK uses ordinary
      `AppData\Roaming`.
- [x] **Does Construct pick up an imported structure after a world reload?**
      `construct import <file> --world <world>`, reload the world, and look
      for it in Construct's in-game list.
- [x] **Does the `level.dat` Beta APIs flip register in-game?**
      `construct experiment <world> --beta-apis on`, then check that world's
      Experiments settings. The command already proves the file round-trips;
      only the game proves the flip is honored.
- [x] **Does a `.mcstructure` under `structures/<namespace>/` load as
      `<namespace>:<name>`, nesting included?** `docs/bedrock-mcstructure-files.md`
      — a local, untracked copy of tryashtar's third-party `.mcstructure` format
      documentation, published on GitHub (github.com/tryashtar) — documents both
      the flat and nested forms straight from the game's own loading rules, not
      inference from one shipped pack — this item confirms that documentation
      against the shipping game rather than settling an open question.
- [ ] **Does `construct install` produce a working Construct?** Install into
      a scratch world, load it, and run `/construct`.
- [ ] *(Windows)* **Do dev packs in `Users\Shared` apply to a world owned by
      a specific account?**
- [ ] **Does the in-use refusal fire on Minecraft builds other than
      mcpelauncher/macOS?** With a world loaded, run
      `construct experiment <world> --beta-apis on` and expect exit 4 and the
      "Close the world" message; then close the world and expect the flip to
      succeed. The 10-second window comes from one measured autosave cadence
      (~5s, mcpelauncher/macOS, 2026-09-04). A build that saves less often
      would slip through the window and revert the change silently again —
      which is exactly the bug this replaced, so it is worth re-measuring per
      platform rather than assuming.
- [ ] **Does a merged structure load and place correctly in-game?** Save two pieces of one
      build with structure blocks, `construct export <world> <a> <b> --merge -o merged.mcstructure`,
      import it, and place it. Check that the pieces land in their original relative positions,
      that the gaps between them leave existing terrain untouched rather than carving air, and
      that block entities (a labelled chest in each piece) kept their contents. Spec §12 asserts
      all three semantically, but only the game proves the file is one it accepts.
- [ ] **Does a merged structure beyond structure-block dimensions load?** The reference
      documentation says sizes past 64×256×64 load "just as expected"; merge only warns. Confirm
      with a merge whose union exceeds that.
