# Project Onboarding Guide: wing-tape

## 🛠 Tech Stack
- **Language:** [Rust](https://www.rust-lang.org/)
- **ML Framework:** [LiteRT](https://github.com/google-ai-edge/litert) (using a local patched version for RPi3 cross-compilation in `LiteRT/`)
- **Audio Library:** `audio-capture` (local crate for ALSA-based audio capture)
- **CLI Parsing:** `clap`

## 🗺 Architecture & Purpose Map

The project follows an actor-based pattern to coordinate audio capture and ML inference.

### Core Workflow
1. **Audio Sourcing:** Audio is retrieved either from hardware (`CaptureActor` in `pipeline/capture.rs`) or from files (`WAVFileSource` in `pipeline/wav_source.rs`).
2. **Windowing:** Audio is segmented using `SlidingAudioWindow` (in `utils/windowing.rs`).
3. **Orchestration:** `Orchestrator` (in `pipeline/orchestrator.rs`) routes captured windows to the inference engine.
4. **Inference:** `InferenceActor` (in `pipeline/inference.rs`) executes models via LiteRT.

### Key Components
- **`src/pipeline/`**: The heart of the system, managing the lifecycle of audio data and ML predictions.
- **`audio-capture/`**: A dedicated crate for low-level audio hardware interaction.
- **`LiteRT/`**: Contains the patched Google LiteRT project to support RPi3 cross-compilation.
- **`WAVFileSource`**: Provides a way to use pre-recorded WAV files as an audio source.

## 🔍 File Index

### Root
- `Cargo.toml`: Workspace and package configuration.
- `Cargo.lock`: Dependency lockfile.
- `LICENSE`: Project license.
- `.gitignore`: Git ignore rules.
- `.gitmodules`: Git submodule configurations.

### Main Application (`src/`)
- `src/lib.rs`: Library entry point.
- `src/bin/birdnet-test.rs`: CLI application for testing BirdNET inference.
- `src/pipeline/mod.rs`: Pipeline module definitions and shared types.
- `src/pipeline/orchestrator.rs`: Coordinates audio capture and inference via actors.
- `src/pipeline/birdnet.rs`: BirdNET-specific processing logic.
- `src/pipeline/capture.rs`: Implements audio capture actors.
- `src/pipeline/inference.rs`: Implements ML inference actors.
- `src/pipeline/microphone.rs`: Microphone-specific audio capture logic.
- `src/pipeline/wav_source.rs`: Implements audio sourcing from WAV files.
- `src/utils/mod.rs`: Utility module definitions.
- `src/utils/windowing.rs`: Implements `SlidingAudioWindow` for audio segmenting.

### Audio Capture Crate (`audio-capture/`)
- `audio-capture/Cargo.toml`: Crate configuration.
- `audio-capture/Cargo.lock`: Dependency lockfile.
- `audio-capture/LICENSE`: License.
- `audio-capture/src/lib.rs`: Library entry point.
- `audio-capture/src/audio_capture.rs`: Main implementation of audio capture.
- `audio-capture/src/bin/list-device.rs`: Utility to list available audio devices.
- `audio-capture/src/bin/record-wav.rs`: Utility to record audio to a WAV file.

### Other
- `BirdNET/`: Reserved for BirdNET models/assets.
- `LiteRT/`: Patched Google LiteRT for RPi3 support.
- `docs/`: Project documentation.
- `tests/`: Project tests.
