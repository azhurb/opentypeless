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

## Open: the two differentiating features had no measurable effect

Both are accepted and both are real parameters, but neither changed the output on synthetic
speech. This is the main thing left to settle, and it decides how the feature should be
described to users.

- **`custom_vocabulary`**: three paired trials, everything else held fixed, produced
  byte-identical transcripts with and without the terms. "Akeneo" came back as "Akenia" and
  "OpenTypeless" as "open typeless" in every run, including the ones that listed both terms.
- **`mode: smart`**: two paired trials against `verbatim` on deliberately disfluent speech
  ("So um, I think we should uh…") produced byte-identical transcripts. Fillers were retained
  in both; the spoken "twenty three point five percent" became "23.5%" in both, so that
  formatting is not attributable to smart mode.

Candidate explanations, none tested: the audio was `say`-synthesized and may be too canonical
for biasing to have anything to correct; the features may be inactive on the free tier
(responses report `service_tier: "standard"`); or they may be accepted-but-not-yet-implemented
on this surface. **The next step is a real microphone recording**, not more synthetic samples.

Until that is settled, the vocabulary and smart-mode benefits should be described as sent and
accepted, not as working.

## Deferred

- **`gemini-3.5-transcribe-live`.** The streaming counterpart over the Live API, in the shape of
  `stt::deepgram`: partials during the utterance instead of one request at the end. Roughly twice
  the price (~$0.009/min blended against ~$0.005/min). Worth doing only if the batch round-trip
  measures badly against the streaming providers.
- **Vocabulary biasing for the other providers.** `SttConfig.custom_vocabulary` now reaches every
  provider and only this one reads it. Deepgram keyterms and AssemblyAI word boost are the
  equivalents. Worth wiring only once the effect above is understood.
- **Regional language variants.** Settings offers bare ISO-639-1 codes only, so `en-GB` spelling
  or `pt-PT` cannot be asked for. Exposing variants is a `LANGUAGES` change that affects every
  provider's mapping.
