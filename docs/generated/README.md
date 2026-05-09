# Generated References

Folder for generated references that derive from code rather than human judgement. None checked in yet.

Each generated file must declare:

- Source command or script.
- Whether manual edits are allowed.
- How to refresh.

## Good Candidates (currently maintained by hand)

These are documented manually today; they are the cleanest candidates for code generation because the human-judgement layer is thin:

- **Tauri command list + signatures** — derive from `tauri::generate_handler!` in `src-tauri/src/lib.rs` and `#[tauri::command]` definitions. Source-of-truth duplication today: [`architecture/frontend-backend.md`](../architecture/frontend-backend.md#tauri-commands).
- **TypeScript wrapper ↔ command map** — would catch wrappers without a registered command and vice versa.
- **Emitted event list** — grep `emit("` in `src-tauri/src/`. Source-of-truth duplication today: [`architecture/pipeline.md`](../architecture/pipeline.md#events) and [`architecture/frontend-backend.md`](../architecture/frontend-backend.md#events).
- **`AppConfig` defaults table** — derive from `Default::default` in `src-tauri/src/storage/mod.rs`. Source-of-truth duplication today: [`architecture/storage.md`](../architecture/storage.md#appconfig-defaults).
- **STT / LLM provider IDs** — derive from `create_provider` match arms and the frontend `appStore.ts` union; would have caught the `deepgram` mismatch documented in [Providers](../architecture/providers.md#mismatches-with-the-frontend-list).
- **SQLite schema** — derive from runtime `CREATE TABLE` calls (and surface drift from `migrations/001_init.sql`).

## Should Stay Human-Maintained

- All of `docs/architecture/`, `docs/domain/`, `docs/decisions/` — these capture intent, invariants, and trade-offs that are not derivable from code.
- `docs/references/conventions.md`, `docs/references/documentation-maintenance.md` — these are policies.

## Needs confirmation

- No generation scripts exist yet. The pieces above are listed in priority order; the first one to build is the command + event reference because it most often goes stale silently.
