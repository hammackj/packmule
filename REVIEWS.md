# Addon reviews

A catalog addon's release is **Reviewed** when Claude and Codex have each
read its code and found nothing malicious. A review covers the exact zips it
read, by SHA-256, so a later release, or a zip swapped on an existing release,
shows as **Not reviewed** until it has its own review. Both reviewers must
pass it; any other result shows the build as **Flagged**, with the report a
click away. Badges only inform: Packmule never blocks an install because of
them.

Reviews are a second pair of eyes, not a guarantee. They catch deliberate
harm in the code; they can miss subtle bugs, and an addon's author could
still publish something bad tomorrow, which is why reviews pin exact builds.

## What the reviewers look for

WoW addons run in Blizzard's Lua sandbox, which has no file system, network or
operating-system access, so the harm an addon can do is in the game:

- stealing gold or items: mail with money or items, trades, guild bank
  withdrawals, auction house bids and posts, done silently or to someone the
  player didn't choose
- deleting items or mail
- chat, whispers or Battle.net messages the player didn't ask for: spam, scam
  links, impersonation
- leaking the player's data to other players through addon messages or chat
- phishing: popups or links to websites, fake Blizzard or "support" messages
- hidden code: `loadstring`/`RunScript` on strings that are built, received
  from other players or obfuscated
- macros or key bindings rewritten to do any of the above
- executables or scripts in the zip, which WoW never runs but whoever unpacks
  the zip might

Talking to its own addon channel (group sync, version checks), selling grey
items at a vendor and linking to the author's own page are normal.

## Running a review

Reviews run on a maintainer's machine with their own `claude` and `codex`
logins, then get committed:

```sh
GITHUB_TOKEN=$(gh auth token) cargo run --example review -- <catalog id>
git add catalog.json reviews/ && git commit
```

The tool:

1. Downloads every zip of the addon's latest release and checks each against
   GitHub's SHA-256, then unpacks them (refusing symlinks and paths outside
   the folder, as installs do).
2. If an earlier release passed review, diffs its zip against this one so the
   reviewers concentrate on the changes. Otherwise, or if the diff is huge, they
   read everything.
3. Pre-scans the Lua, XML and `.toc` files for the risky APIs above, very long
   lines, and files that aren't normal addon files, and hands the list to both
   reviewers to check line by line.
4. Runs both reviewers on the unpacked code, which they are told is untrusted:
   text in the addon that tries to instruct them is itself reported. Claude
   runs with `--restricted` and only the Read, Grep and Glob tools, without
   your settings, hooks or MCP servers. Codex runs in its `read-only` sandbox.
5. Writes `reviews/<id>/<tag>.md` with both reports and adds the verdicts to
   the entry's `reviews` in `catalog.json`.

It exits non-zero unless both pass, and says when they disagree. Read the
report before committing either way. `--dry-run` prepares everything and
prints the prompt without running a reviewer; `--keep` leaves the unpacked
code in place to look at.

## Signing off

When a reviewer is unsure or fails a build for something that isn't
malicious (an honest bug, or a review that didn't finish), a maintainer can
read the report and pass it:

```sh
cargo run --example review -- --pass <catalog id> --note "why it passes"
```

That records who passed it, when and why next to the reviewers' verdicts in
`catalog.json`, and appends it to the report. The app then shows the build as
Reviewed, and its tooltip names the sign-off.

## Verified source

`verified` on a catalog entry records that someone checked the repository is
the author's official one: it's the repository linked from the addon's
CurseForge, Wago or WowAce page or its own `.toc`, not a fork or lookalike.
`ciBuilds` notes whether its release zips are built by CI from tagged source
(`github-actions[bot]` uploads them) or uploaded by hand.
