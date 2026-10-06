# Adding and removing signers

The manager embeds the public keys of its signers (`crates/w2rpm-lib/release/keys.txt` in the w2rpm
repo, mirrored by `keys/*.pub` here) and accepts an index signed by **any** of them. Users learn about
a change only by updating the manager, so every change below ends in a manager release.

## Adding a signer

1. They generate a key pair (SIGNING.md) and send you the `.pub` file.
2. Confirm it is theirs (a message from them on another channel is enough for a group this small).
3. Add the file to `keys/` here, add its base64 line to `keys.txt` in w2rpm, ship a manager release.
   Their signatures count for users who have updated; they should not publish before that.

Do this for everyone before the first release if you can, so no one is waiting on a later update.

## Removing a signer, or a lost or leaked key

1. Delete the key from `keys/` and from `keys.txt`, and ship a manager release.
2. Until users update, their managers still trust the old key. Publish nothing signed by it, and
   have a remaining signer publish a fresh index (higher serial) that drops anything you do not trust.
3. Say what happened in the release notes.

If every key is lost at once, nothing can publish until a new manager release embeds a new key. The
`next_key` field in the index announces a planned replacement ahead of time; it is optional and not
needed for the cases above.
