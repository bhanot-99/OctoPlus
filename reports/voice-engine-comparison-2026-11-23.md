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
