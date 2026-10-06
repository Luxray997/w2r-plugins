# Signing the index (several signers, one offline key each)

## Model

- Every signer holds **their own** minisign key pair. The secret key never leaves that person's
  machine (a hardware-backed or encrypted drive is better; a password is mandatory).
- Every signer's **public** key is committed in `keys/` and embedded in the manager. The manager
  already accepts a signature from *any* trusted key (`verify_index_any`), so adding or removing a
  signer is a manager release, not a protocol change.
- CI never signs. It builds, assembles and verifies; a human signs after looking.
- What a signature means: "I compared this index with the reviewed merge commit and its release
  assets, and I approve it." Reviewing is the control; the key is only the proof.

**Honest limits.** Any single signer can publish an index (there is no 2-of-N rule), so the
protections are: PR review decides what gets built, the signer checks the artifact against the
release, and old index entries cannot change (`serial` rises, entries are immutable, the manager
refuses rollbacks). A stolen signer key is fixed by removing its public key in a new manager
release (ROTATION.md). If a threshold is wanted later, the index format would need a multi-signature
field; that is a spec change and is not planned.

## One-time setup per signer

Install minisign (<https://jedisct1.github.io/minisign/>), then:

```bash
minisign -G -p w2r-<yourname>.pub -s ~/.minisign/w2r-<yourname>.key
```

Choose a strong password. Send the `.pub` file (public only) to the other maintainers; ROTATION.md
says where it goes. Generate all keys before the first manager release if you can.

## Each release

1. Open the release workflow run for the merge you intend to publish and download the
   `unsigned-index` artifact (`index.json`).
2. Check it: diff it against the currently published file
   (`git diff --no-index published/index.json index.json`); the diff must contain only the new versions, the serial bump and the new dates. Each new `url` must be a
   `pkg-<id>-<version>` release asset of this repo, and its `sha256` must match the release asset
   (`sha256sum` of the downloaded zip).
3. Sign with a trusted comment naming the serial and commit:

   ```bash
   minisign -S -H -s ~/.minisign/w2r-<yourname>.key -m index.json \
     -t "w2r-plugins serial <N> commit <short sha>"
   ```

   `-H` is required: the manager only accepts the prehashed `ED` form.
4. Verify with the exact code the manager uses:

   ```bash
   cargo run -p w2r-publish -- verify --index index.json --sig index.json.minisig \
     --key keys/w2r-<yourname>.pub
   ```

5. Rename `index.json.minisig` to `index.json.sig`, commit both to the `gh-pages` branch, push.
   Wait for Pages to update, then run `w2rpm refresh` against the live URL once.

## When something goes wrong

- A signature you made was wrong: publish a corrected index with a higher serial. Never reuse a serial.
- You lost or leaked your key: tell the other signers at once and follow ROTATION.md.
