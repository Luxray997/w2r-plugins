# Pipeline: from pull request to signed index

```
PR ──review──► main ──CI──► packages + unsigned index ──signer──► index.json + .sig ──► Pages
```

1. **Pull request.** CI (`ci.yml`) runs `w2r-publish check` (manifests valid, id equals folder name,
   `Cargo.toml` version equals `plugin.json` version), builds every plugin for `i686-pc-windows-msvc`
   and runs the unit tests. A plugin is listed only after a maintainer approves the PR
   (CODEOWNERS; extra rules for risky plugins in CONTRIBUTING.md).
2. **Build.** On a push to `main` that changes a plugin, `release.yml` builds each plugin in release
   mode, runs `w2r-publish package` (deterministic zips holding only `plugin.json`, the DLL,
   `README.md`, `LICENSE`) and uploads the zips as assets of a GitHub release named
   `pkg-<id>-<version>`. Package URLs are therefore permanent, and a release is never edited.
3. **Assemble.** The same workflow downloads the currently published, signed `index.json` and its
   signature, runs `w2r-publish verify` on them against `keys/*.pub`, then runs
   `w2r-publish index --previous` to produce an **unsigned** `index.json`: the old entries
   untouched, new versions appended, `serial` plus one, `expires` 90 days out. The tool runs the
   manager's own parser over the result before it is accepted. The workflow stores it as the
   `unsigned-index` artifact and stops. **CI holds no signing key.**
4. **Sign.** A signer follows [SIGNING.md](SIGNING.md): download the artifact, compare it with what
   the release shows, sign with the real `minisign`, run `w2r-publish verify`, commit the pair to the
   `gh-pages` branch.
5. **Serve.** GitHub Pages serves `index.json` and `index.json.sig` from the `gh-pages` branch. The
   manager's default index URL is that `index.json` (the `.sig` sits next to it).

Re-signing without any plugin change (to extend `expires` before it lapses) is the same flow with the
manual `workflow_dispatch` run of the release workflow: no new packages, `serial` plus one. Re-sign at least every 2 months (`expires` is 90 days).

## Rules the tooling enforces

- A published `(id, version)` is immutable. A rebuild with a different hash keeps the published
  entry; changes ship under a new version.
- The index never loses an entry. Removing a plugin (security) is a manual, reviewed edit of the
  assembled index, announced in the release notes.
- Everything in an index must pass `w2rpm-lib`'s parser; nothing is signed that the manager would refuse.

## Repository access for CI

`w2r-publish` and the plugins depend on `w2r-abi`, `w2r-formats` and `w2rpm-lib` from the w2rpm
repository, pinned by commit in the root `Cargo.toml`. While w2rpm is private, both workflows need a
token to fetch it:

1. Create a **fine-grained personal access token** (GitHub, Settings, Developer settings) owned by the
   account that owns w2rpm, with **only** the repository `hastinbe/w2rpm` and the single permission
   **Contents: Read-only**. Give it an expiry (for example 90 days) and note the date.
2. Add it to this repository as an Actions secret named `W2RPM_READ` (Settings, Secrets and variables,
   Actions).
3. Both workflows start with a step that points git at the token for that one repository and fail with a
   clear message if the secret is missing. Renew the token before it expires.

Cargo fetches through the git CLI (`CARGO_NET_GIT_FETCH_WITH_CLI`) so it honours that setting. Pull
requests from forks receive no secrets, so CI cannot build them; a maintainer re-runs the checks from a
branch in this repository. Delete the step and the secret when w2rpm becomes public.
