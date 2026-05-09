# Cloud Pro Mode

OpenTypeless supports optional cloud providers for STT and LLM. BYOK mode remains available without an OpenTypeless account.

Evidence: `README.md`, `src-tauri/src/stt/cloud.rs`, `src-tauri/src/llm/cloud.rs`, `src-tauri/src/lib.rs`, `src/lib/auth-client.ts`, `src/stores/authStore.ts`, `src/lib/deep-link.ts`.

## Backend Base URL

Default cloud API base URL:

```text
https://www.opentypeless.com
```

Build-time overrides:

- Frontend: `VITE_API_BASE_URL`
- Rust backend: `API_BASE_URL`

## Token Flow

1. Frontend authenticates through Better Auth.
2. Frontend calls `set_session_token(token)`.
3. Rust stores the token in `SessionTokenStore`.
4. Cloud STT/LLM providers use the token for proxy requests.

## Subscription Checks

Connection-test commands check `/api/subscription/status` and require `plan == "pro"` before reporting cloud provider success.

## Deep Links

Auth callbacks use the `opentypeless://` scheme. `tauri-plugin-single-instance` forwards deep links to the running app instance.

## Needs confirmation

- Exact Pro quota, billing, and entitlement rules should be confirmed against the cloud backend and product policy before being treated as canonical.
- Local docs do not currently define backend API contracts beyond the client calls visible in this repo.
