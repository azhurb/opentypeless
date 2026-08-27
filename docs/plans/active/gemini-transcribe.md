# Gemini Transcribe

`gemini-3.5-transcribe` is registered as a batch STT provider (`gemini-transcribe`). This file
tracks what has **not** been verified against the live API and what was deliberately left out.

Landed: provider in `src-tauri/src/stt/gemini.rs`, factory arm, `SttConfig.custom_vocabulary`
fed from the user's dictionary, connection test / benchmark via a free model read, pre-warm,
frontend ID and label. See [Providers → Gemini Transcribe](../../architecture/providers.md#gemini-transcribe-batch).

## Needs confirmation — nothing here has run against a real key

The wire shape was built from the published docs, and two parts of it are inference rather
than a documented example. Both fail loudly (an API 400) rather than silently, so one live
dictation settles all of it.

- **Inline audio shape.** The transcription guide only shows the Files API form
  (`{"type": "audio", "uri": ..., "mime_type": ...}`). The inline form is documented on the
  Interactions file-input page as `{"type": "document", "data": "<base64>", "mime_type": ...}`
  for PDFs, and `gemini.rs` assumes `type: "audio"` with `data` is the audio equivalent. If it
  is not, the fallback is the Files API upload plus its extra round-trip.
- **`audio/wav`.** The docs example uses `audio/mp3` and never enumerates accepted types. We
  send the 16 kHz 16-bit mono WAV the pipeline already builds.
- **`output_text` at the REST top level.** The docs phrase it as `interaction.output_text`,
  which is the SDK object path. `parse_response` reads the top-level field and falls back to
  walking `steps[].content[].text`.
- **Region-tagged language codes.** `language_codes` is documented as BCP-47 with a region
  (`en-US`, `es-ES`), so `bcp47()` maps our ISO-639-1 codes and picks a region per language.
  Whether a bare `en` is also accepted is untested; if it is, the table can go away.
- **Smart mode against the polish step.** `smart_format` maps to `mode: {"type": "smart"}`,
  which removes fillers and false starts and formats spoken lists, dates and numbers — work the
  LLM polish step also does. Whether polish can be skipped for this provider (and what that
  saves in latency) needs a side-by-side on real dictations, not a decision on paper.

## Deferred

- **`gemini-3.5-transcribe-live`.** The streaming counterpart over the Live API, in the shape of
  `stt::deepgram`: partials during the utterance instead of one request at the end, and no
  buffering. Roughly twice the price (~$0.009/min blended against ~$0.005/min). Worth doing only
  if the batch round-trip measures badly against the streaming providers.
- **Vocabulary biasing for the other providers.** `SttConfig.custom_vocabulary` now carries the
  user's dictionary to every provider, and only this one reads it. Deepgram's keyterm prompting
  and AssemblyAI's word boost are the equivalents; wiring them is a per-provider change now that
  the field exists.
- **Regional language variants.** Settings offers bare ISO-639-1 codes only, so `en-GB` spelling
  or `pt-PT` cannot be asked for — `bcp47()` picks one region per language. Exposing variants is
  a UI change in `LANGUAGES` that affects every provider's mapping, not just this one.
- **Detected-language badge.** The API documents automatic language identification but no
  response field that reports the result, so this provider returns `None` and history rows show
  no language, the same as AssemblyAI.
