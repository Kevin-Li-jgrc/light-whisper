# Third-party notices

Light-Whisper contains or interoperates with third-party software and assets.
The project license in [`LICENSE`](LICENSE) applies only to material for which
the Light-Whisper licensor can grant those rights. The items below remain under
their own terms.

## FireRedVAD model assets

The bundled upstream FireRedVAD ONNX model and CMVN data, together with portions
of the integration adapted from upstream, remain subject to the Apache License
2.0. The license text is available at
[`src-tauri/resources/FireRedVAD-LICENSE.txt`](src-tauri/resources/FireRedVAD-LICENSE.txt).
Light-Whisper's original integration and post-processing changes are licensed
under GPL-3.0-only.

## transcribe.cpp and downloaded models

[transcribe.cpp](https://github.com/handy-computer/transcribe.cpp) is distributed
under the MIT License. Qwen3-ASR model files are downloaded separately and
remain subject to the terms published by their respective upstream providers.

Package-manager dependencies may carry additional licenses. Their inclusion in
the source tree, Python engine, or application bundle does not change those
licenses.

## Local text processing

The isolated `llama-server` executables are from llama.cpp **b11118** (MIT).
Bundled LLVM OpenMP runtime licenses are preserved in each backend directory.
The CUDA backend includes NVIDIA CUDA 12.4 runtime/cuBLAS redistributables under
their own terms; see `resources/local-llm/NVIDIA-CUDA-12.4-EULA.html` in the installation.
These independently licensed runtime components are not relicensed under GPL.

Qwen3.5-0.8B Q8 weights are downloaded from the pinned Unsloth GGUF repository and
remain Apache-2.0 licensed. LFM2.5-1.2B-Instruct Q8 weights are downloaded from the
pinned Liquid AI repository and remain subject to the **LFM Open License v1.0**,
including its commercial-use revenue threshold. The LFM weights are not Apache
or GPL licensed. Original license texts are in `resources/local-llm/` and are
provided before downloading models. Weights are not included in the installer.
Pinned revisions, sizes, and SHA-256 hashes are recorded in the local model
catalog and `resources/local-llm/manifest.json`.
