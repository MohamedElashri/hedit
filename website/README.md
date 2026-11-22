# Documentation website

The public docs are built with [Nida](https://github.com/MohamedElashri/nida) and
published to [GitHub Pages](https://melashri.net/hedit/). The site uses a small
local theme with no third-party browser dependencies, remote fonts, analytics,
or Node build pipeline.

## Build and preview

Use Python 3.11+ and the Nida release pinned in [nida.lock.json](nida.lock.json).
If that Nida version is installed:

```sh
make test-docs-site docs-site
nida serve --site target/docs-site
```

Open `http://127.0.0.1:1702/hedit/`. To fetch the checksum-pinned generator into
the ignored build directory instead of installing it globally:

```sh
python3 scripts/setup-nida.py
make docs-site NIDA=target/docs-tools/nida
target/docs-tools/nida serve --site target/docs-site
```

On Windows the executable is `target/docs-tools/nida.exe`. Rebuild after editing
the source guides; `nida serve` watches the assembled source, not the original
Markdown. `make docs-site-prepare` refreshes that source without a full build.
Builds replace only `target/docs-site/`; generated output is never committed.

For another deployment path, use:

```sh
python3 scripts/build-docs.py --base-url https://example.com/manual/
```

The build runs `nida check`, `nida build`, and an HTML link-graph check. The final
check includes template navigation, fragment anchors, images, scripts,
stylesheets, downloads, and search routes under the configured path prefix.

## Where to edit

| Content | Source |
| --- | --- |
| User reference pages | [USER_GUIDE.md](../USER_GUIDE.md) |
| API contracts and threat model | Markdown files in [docs](../docs/) |
| Contributor introduction | [content/project/contributing.md](content/project/contributing.md) |
| Detailed maintainer reference | [DEVELOPER_GUIDE.md](../DEVELOPER_GUIDE.md), linked from the contributor introduction |
| Release history | [CHANGELOG.md](../CHANGELOG.md) |
| Web introductions and plugin tutorials | [content](content/) |
| Page boundaries, titles, summaries, and order | [imports.toml](imports.toml) and authored front matter |
| Layout and styling | [templates](templates/) and [static](static/) |

The importer splits sections outside code fences, maps source links to web
routes, and preserves links back to the canonical Markdown for editing. A missing
or ambiguous heading fails the build. User-guide/API prose is not copied into
another maintained document. The application continues shipping its offline help.

Tutorial code is rendered from the downloadable files in
[static/examples/first-plugin](static/examples/first-plugin/). When changing the
example, run it with an installed package's confined author CLI:

```sh
hedit plugin check website/static/examples/first-plugin/extension.json
hedit plugin test website/static/examples/first-plugin/extension.json website/static/examples/first-plugin/fixtures.json
```

## Deployment

The [Pages workflow](../.github/workflows/pages.yml) checks and builds relevant
pull requests without deployment authority. Only `main` publishes to the
`github-pages` environment. Actions are pinned to full commits; Nida archives are
pinned by version and SHA-256. To update Nida, review its release and update the
archive hashes in [nida.lock.json](nida.lock.json).
Site-only changes run this workflow instead of the application matrix. Changes
to application code or the shared offline guides still run application CI;
manual and release validation remain available independently. A workflow-lock
update alone does not rebuild the application; dispatch application CI when
deliberately updating its build actions.
After changing workflow `uses:` entries, run the official
`gh actions-lock .github/workflows/pages.yml` generator and commit its lockfile
diff. Repository policy requires coverage even for full-commit action references.

Pages uses the repository owner's inherited custom domain. The configured
`/hedit/` prefix is used in local preview, navigation, search, and canonical URLs.
The workflow takes the actual base URL from GitHub's Pages configuration.
The assembler writes that URL into its generated config before both Nida
commands; it does not depend on a command-line override supported by only some
Nida commands. Pull requests exercise this same explicit-URL build path.

Keep browser scripts in `website/static/js/`. The native-app policy permits
JavaScript only there and continues to reject frontend code in the application
and Node package/build files throughout the repository.

## Search metadata

The shared head provides page descriptions, canonical URLs, Open Graph/Twitter
previews, and JSON-LD website/page/breadcrumb data. Search and error pages use
`noindex`; the build removes internal search from Nida's sitemap. Site checks
validate the metadata, structured JSON, social-image paths and exact sitemap set.
The copyright footer uses the build year and links to the full MIT notice.

The docs sitemap is `https://melashri.net/hedit/sitemap.xml`, ready for submission
in Google Search Console or Bing Webmaster Tools. Effective crawler rules live at
the domain's `/robots.txt`; a project's `/hedit/robots.txt` does not override that
policy. The main domain currently allows ordinary search engines. Its crawler
policy is maintained separately from this site.
