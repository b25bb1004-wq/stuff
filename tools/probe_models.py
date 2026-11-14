"""Probe which free models on the NVIDIA Build key answer a small coding prompt.
Run inside WSL (the key lives there and is never printed): python3 probe_models.py"""
import sys, time
sys.path.insert(0, "/home/dev/faith/tools")
import nv

PROMPT = ("Write a Rust function `fn gcd(a: u64, b: u64) -> u64` using the Euclidean algorithm. "
          "Reply with only the code, no explanation.")
MODELS = ["moonshotai/kimi-k2.6", "z-ai/glm-5.3-flash", "deepseek-ai/deepseek-v4.1-flash",
          "minimaxai/minimax-m3.1-flash", "mistralai/mistral-large-4-1t-a52b",
          "nvidia/nemotron-4-340b-instruct", "google/gemma-4-31b-it", "openai/gpt-oss-20b"]
for m in sys.argv[1:] or MODELS:
    t = time.time()
    try:
        out = nv.chat(PROMPT, "No preamble.", model=m, max_tokens=200, retries=1)
        verdict = "OK" if "fn gcd" in out else "ODD"
        print(f"{m:45s} {time.time() - t:5.1f}s {verdict} {len(out)} chars", flush=True)
    except Exception as e:  # report and move on: a dead model must not stop the probe
        print(f"{m:45s} {time.time() - t:5.1f}s FAIL {str(e)[:60]}", flush=True)
