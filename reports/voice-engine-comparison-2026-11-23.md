# Voice engine comparison (voice upgrade, 2026-11-23)

Question: is there a small speech engine that is faster than Kokoro without breaking the design
(C5 thinks, C6 approves, nothing leaves the laptop)? Tested on this laptop (Ryzen 7 5800H, CPU only,
nothing else heavy running). Same three sentences (31, 43 and 83 characters), 2 runs each, warm engine.
Raw output: `reports/bench/voice-engine-comparison-2026-11-23.txt`. Listening samples: `~/voice-compare/`.

| Engine | Size | Time for a 31-char piece | 83-char piece | Memory (peak) | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Kokoro (now, in C2) | 82M | about 1.1-1.7 s (service trace, warm) | about 4 s (0.05 s per char) | 465-625 MiB for short pieces (ADR-0015) | about 1.3x real time |
| Pocket TTS 100M | 100M | 0.41 s | 0.9 s | 668 MB | 4.7-5x real time; int8: 0.25 s, 451 MB |
| Pocket TTS 18M | 18M | 0.11 s | 0.28-0.41 s | 235 MB | 11-18x real time |
| Pocket TTS 7M | 7M | 0.07-0.09 s | 0.29-0.36 s | 150 MB | 13-27x real time |
| Piper lessac-medium | 63 MB file | 0.06-0.07 s | 0.16 s | 175-240 MB | 24-30x real time |
| Piper ryan-high | 121 MB file | 0.29 s | 0.9-1.0 s | 430-490 MB | about 5-6x real time |

Not tested: Moshi, Haru-4B (speech-to-speech models, GPU or Mac, about 12 GB+, answer on their own,
Haru's card says "not a reliable source of facts" and "does not follow instructions the way an assistant
does"), Chatterbox, CosyVoice2 (GPU oriented).

Not measured: voice quality (nobody listened yet), intelligibility score (the eval tool needs a wake
phrase in each recording), time to the first audio chunk of a streaming engine (the numbers above are the
whole piece; for a 28-character first piece this is the same thing to within a few ms).
Kokoro's number is from the live service trace, not the same harness; treat the comparison as order of
magnitude, not exact.

Integration notes (from the engines' own pages, not tried):
- Pocket TTS: official runtime is PyTorch or a Rust binary (CPU, no Python). No official ONNX; community
  ONNX and C++ ports exist. Weights CC-BY-4.0, code MIT; the Hugging Face page is gated.
- Piper: ONNX voices run directly on ONNX Runtime, which C2 already uses; needs a phonemiser (espeak-ng).

## Rerun, same footing (2026-11-23, after the owner's listening test)

Both engines warm, model loaded once, median of the last 4 of 5 runs, nothing else heavy running.
Kokoro through `neuroos-voice-say` with the new `NEUROOS_SAY_REPEAT` option; Pocket TTS 18M through its
own `generate` command (its reported generation time, load not included).

| Text | Chars | Kokoro | Pocket TTS 18M | Faster by |
| :--- | :--- | :--- | :--- | :--- |
| "17 times 6 is 102." | 18 | 2190 ms | 152 ms | 14x |
| "The sky appears blue because" | 28 | 1535 ms | 123 ms | 12x |
| "Sure, the battery is at eighty." | 31 | 1597 ms | 125 ms | 13x |
| "Jane Austen wrote Pride and Prejudice." | 38 | 1967 ms | 142 ms | 14x |
| "Two plus two is four, which is a basic sum." | 43 | 2311 ms | 170 ms | 14x |
| "Your laptop fan may run loudly ... for example." | 83 | 4224 ms | 347 ms | 12x |

Peak memory: Kokoro 573-862 MB (grows with the piece), Pocket TTS 18M about 235 MB.
Owner's listening result: Kokoro best, Pocket TTS 18M second (D-52).
Reading: with the fp32 model Kokoro needs about 1.5 s even for a 28-character piece (a short piece with
digits was slowest), so shortening the first piece has reached its floor. Under 1 s needs either a
faster Kokoro (quantized model, settings), ready-made audio for easy answers, or a faster engine.
