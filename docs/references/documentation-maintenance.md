# Documentation Maintenance

Docs should change with the code or product behavior they describe.

## Update Docs When

- Architecture boundaries change.
- Pipeline states, events, or lifecycle behavior change.
- A provider is added, removed, renamed, or changes setup requirements.
- A Tauri command or event contract changes.
- Storage schema, config fields, or retention behavior changes.
- User-facing behavior changes in recording, polishing, translation, selected text, output, onboarding, auth, or history.
- Public feature descriptions change in `README.md` or on the OpenTypeless features page.
- Development commands, CI checks, or release/build steps change.
- A decision creates a new pattern future contributors should follow.

## How To Write Docs

- Prefer short focused docs.
- Link to code evidence when possible.
- Mark inferred behavior as `Inference`.
- Mark missing or uncertain sections as `Needs confirmation`.
- Avoid copying large README or code comments into docs.
- Keep placeholders honest: say what is missing and where confirmation should come from.

## Plans

- Put active multi-step plans in `docs/plans/active/`.
- Move completed plans to `docs/plans/completed/` if they are useful history.
- Delete obsolete plans if they no longer help explain current code or decisions.

## Generated Docs

Generated docs should live in `docs/generated/` and include:

- Source command or script.
- Generation date if useful.
- Whether manual edits are allowed.

## Needs confirmation

- No automated docs lint, dead-link check, or freshness check exists yet.
