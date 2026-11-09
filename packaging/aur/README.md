# AUR package: packmule-bin

`publish.sh` writes the PKGBUILD and .SRCINFO for a released version from its
`.tar.gz`, and with `--push` publishes them to the AUR. The release workflow
runs it after each release once the `AUR_SSH_KEY` repository secret is set.

One-time setup:

1. Create an account at https://aur.archlinux.org and add an SSH public key
   to it (My Account → SSH Public Key).
2. Publish the current release from a machine with that key:
   `packaging/aur/publish.sh <version> --push` (the first push creates the
   package).
3. For automatic updates, add the matching private key as the `AUR_SSH_KEY`
   secret: `gh secret set AUR_SSH_KEY < ~/.ssh/<key>`. A key just for this,
   with no other access, is best.

`packaging/aur/publish.sh <version>` without `--push` writes into
`packaging/aur/out/` to inspect or test with `makepkg -si`.
