# Gemini Transcribe

`gemini-3.5-transcribe` is registered as a batch STT provider (`gemini-transcribe`). This file
tracks what is left open after live verification.

Landed: provider in `src-tauri/src/stt/gemini.rs`, factory arm, `SttConfig.custom_vocabulary`
fed from the user's dictionary, connection test / benchmark via a free model read, pre-warm,
frontend ID and label. See [Providers → Gemini Transcribe](../../architecture/providers.md#gemini-transcribe-batch).

## Verified against the live API on 2026-08-27

Run with a real key; each row is a request that returned 200 and parsed. The end-to-end check
is `stt::gemini::tests::live_round_trips_against_the_real_api`, ignored by default:

```bash
GEMINI_API_KEY=... GEMINI_TEST_WAV=/path/to/16k-mono.wav \
  cargo test --manifest-path src-tauri/Cargo.toml --lib stt::gemini::tests::live -- --ignored --nocapture
```

- **Inline base64 audio works.** `{"type": "audio", "data": "<base64>", "mime_type": "audio/wav"}`
  is accepted, so the Files API and its extra round-trip are not needed.
- **The field names are right, and that is proven rather than assumed.** The API rejects an
  unknown parameter with a 400 (`custom_vocabularyy` → *"Unknown parameter … at
  generation_config.transcription_config"*), so `custom_vocabulary`, `language_codes` and `mode`
  returning 200 means they are recognized, not silently swallowed.
- **`mode.type` is enum-validated** to exactly `smart` and `verbatim`, which is the mapping
  `smart_format` uses.
- **The transcript is not where the docs say.** There is no `output_text` at the REST top level;
  the live response has `id`, `status`, `usage`, `created`, `updated`, `service_tier`, `steps`,
  `object`, `model`. `interaction.output_text` is the SDK accessor. `parse_response` reads
  `steps[].content[].text` (filtered to `type == "text"`) and keeps the `output_text` check
  first only as forward compatibility.
- **No detected-language field exists** anywhere in the response, so returning `None` is correct
  rather than a gap. History rows show no language for this provider, as with AssemblyAI.
- **`language_codes` is *not* validated.** A bare `en` and a gibberish `xx-YY` both return 200,
  so the region mapping in `bcp47()` is about keeping to the documented shape, not about
  avoiding a rejection.
- **`mime_type` is not validated either** (`audio/banana` returns 200), so the format is sniffed
  and the MIME we send is not load-bearing.

## Settled: the two differentiating features are inert

Both are accepted, both are provably real parameters (the API 400s a misspelled one), and
neither changes the output. Tested on two independent samples, synthetic and real:

| Parameter | Sample | Paired trials | Result |
| --- | --- | --- | --- |
| `custom_vocabulary` on/off | `say`-synthesized | 3 | byte-identical |
| `custom_vocabulary` on/off | real microphone | 2 | byte-identical |
| `mode: smart` vs `verbatim` | `say`-synthesized, disfluent | 2 | byte-identical, fillers retained in both |
| `mode: smart` vs `verbatim` | real microphone, spoken digits | 3 | byte-identical |

The real-microphone case is the decisive one, because it puts smart mode against its own
documented job. A dictation of spoken digits transcribes as `Testing 1 2 3 4 5` in **both**
modes: spaced digits, exactly the "format spoken numbers into clean text" that smart mode
claims and verbatim does not. If the modes did anything, this sample would separate them.

The earlier synthetic result was therefore not an artifact of TTS audio being too canonical.

What is still unknown: whether these are inactive on the free tier (responses report
`service_tier: "standard"`) or not yet implemented on this API surface. Both would look
identical from here. Re-test if Google announces a change; there is nothing to fix on our side,
since the request is correct by the API's own validation.

**Consequence for users:** the polish step stays on for this provider. It is doing the filler
removal and formatting work that smart mode was supposed to take over, and there is currently no
latency or quality argument for skipping it.

## Deferred

- **`gemini-3.5-transcribe-live`.** The streaming counterpart over the Live API, in the shape of
  `stt::deepgram`: partials during the utterance instead of one request at the end. Roughly twice
  the price (~$0.009/min blended against ~$0.005/min). Worth doing only if the batch round-trip
  measures badly against the streaming providers.
- **Vocabulary biasing for the other providers.** `SttConfig.custom_vocabulary` now reaches every
  provider and only this one reads it. Deepgram keyterms and AssemblyAI word boost are the
  equivalents, and unlike this provider's version they may actually do something — worth wiring
  on their own merits rather than waiting on the question above, which is settled.
- **Regional language variants.** Settings offers bare ISO-639-1 codes only, so `en-GB` spelling
  or `pt-PT` cannot be asked for. Exposing variants is a `LANGUAGES` change that affects every
  provider's mapping.
