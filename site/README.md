# T4E static site

The T4E website is intentionally framework-free. GitHub Pages serves the files
in this directory directly; there is no Node dependency or build output to
commit.

- `index.html` contains the semantic page structure and static fallback content.
- `styles.css` provides the responsive terminal-native visual system.
- `app.js` drives the deterministic demo player and install-command copy action.
- `demos.json` contains the replayable, non-executing T4E scenarios.

Preview it locally from the repository root:

```bash
python3 -m http.server 4173 --directory site
```

Then open <http://127.0.0.1:4173/>. Run `python3 tests/site_static.py` and
`node --check site/app.js` before publishing. The Pages workflow deploys this
directory on pushes to `main` that change the site or its workflow.
