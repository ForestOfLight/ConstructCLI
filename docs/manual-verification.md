# Manual verification

Things automated tests cannot settle. Stage 1 items only; §16 of the design
spec holds the full list.

- [ ] `construct worlds` finds every world the launcher shows, with matching names.
- [ ] `construct list` on a world with structures matches what Construct shows in-game.
- [ ] **Locked world:** load a world in Minecraft, leave it running, then
      `construct list <that world>`. It must print the snapshot notice and still
      list structures. POSIX `fcntl` locks are per-process, so this cannot be
      tested from inside the test binary — only a second process proves it.
- [ ] **World left unchanged:** run `construct list` and `construct export`
      against a world, then confirm the world's `db/` directory is unchanged
      and the world still loads in Minecraft. Byte-identity across every file
      in `db/` has already been verified programmatically; this step is about
      confirming the game itself is still happy with the world afterwards.
- [ ] An exported `.mcstructure` loads in a structure block, or in a
      third-party `.mcstructure` viewer. Byte-identity between the exported
      file and the database value is already proven by tests; only the game
      confirms the file is actually usable in a structure block.
- [ ] *(Windows, needs real hardware)* GDK worlds under
      `%appdata%\Minecraft Bedrock\Users\<account>\...` are discovered, and the
      qualified reference includes the account segment.
- [ ] *(Windows)* Files written into the GDK folder by an ordinary process read
      back in-game. Expected to be a non-issue — the ACL problem was specific to
      UWP's `LocalState` inside an AppContainer, and GDK uses ordinary
      `AppData\Roaming`.
