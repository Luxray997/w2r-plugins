# Pipeline: from pull request to published index

```
PR ──review──► main ──CI──► packages + index.json ──► gh-pages ──► GitHub Pages
```

The index is not signed. Users are protected by HTTPS and by the SHA-256 and size of every package
in the index; whoever controls this repository can publish any plugin, so keep two-factor
authentication on the account and review pull requests (see the w2rpm repository's `docs/index.md`,
"Trust").

1. **Pull request.** CI (`ci.yml`) runs `w2r-publish check` (manifests valid, id equals folder name,
   `Cargo.toml` version equals `plugin.json` version), builds every plugin for `i686-pc-windows-msvc`
   and runs the unit tests. A plugin is listed only after a maintainer approves the PR
   (CODEOWNERS; extra rules for risky plugins in CONTRIBUTING.md).
2. **Build.** On a push to `main` that changes a plugin, `release.yml` builds each plugin in release
   mode, runs `w2r-publish package` (deterministic zips holding only `plugin.json`, the DLL,
   `README.md`, `LICENSE`) and uploads the zips as assets of a GitHub release named
   `pkg-<id>-<version>`. Package URLs are therefore permanent, and a release is never edited.
3. **Assemble.** The same workflow reads the currently published `index.json` from the `gh-pages`
   branch, checks it with `w2r-publish verify`, then runs `w2r-publish index --previous` to produce
   the new one: the old entries untouched, new versions appended, `serial` plus one. The tool runs
   the manager's own parser over the result before it is accepted.
4. **Publish.** A second job commits `index.json` to the `gh-pages` branch (creating it the first
   time). GitHub Pages serves it; the manager's default index URL is
   `https://hastinbe.github.io/w2r-plugins/index.json`.

One-time setup: make the repository public (Pages on a private repository needs a paid plan, and
users must be able to download the packages), then Settings, Pages, "Deploy from a branch",
`gh-pages` / root.

## Rules the tooling enforces

- A published `(id, version)` is immutable. A rebuild with a different hash keeps the published
  entry; changes ship under a new version.
- The index never loses an entry. Removing a plugin (security) is a manual, reviewed edit of
  `index.json` on the `gh-pages` branch, announced in the release notes. Raise `serial` when you do.
- Everything in an index must pass `w2r-loader-lib`'s parser; nothing is published that the manager would
  refuse.

## Repository access for CI

`w2r-publish` and the plugins depend on `w2r-abi`, `w2r-formats` and `w2r-loader-lib` from the w2rpm
repository, pinned by commit in the root `Cargo.toml`. While w2rpm is private, both workflows need a
token to fetch it:

1. Create a **fine-grained personal access token** (GitHub, Settings, Developer settings) owned by the
   account that owns w2rpm, with **only** the repository `hastinbe/w2r-mod-loader` and the single permission
   **Contents: Read-only**. Give it an expiry (for example 90 days) and note the date.
2. Add it to this repository as an Actions secret named `W2R_LOADER_READ` (Settings, Secrets and variables,
   Actions).
3. Both workflows start with a step that points git at the token for that one repository and fail with a
   clear message if the secret is missing. Renew the token before it expires.

Cargo fetches through the git CLI (`CARGO_NET_GIT_FETCH_WITH_CLI`) so it honours that setting. Pull
requests from forks receive no secrets, so CI cannot build them; a maintainer re-runs the checks from a
branch in this repository. Delete the step and the secret when w2rpm becomes public.
