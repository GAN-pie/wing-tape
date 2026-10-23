use audio_capture;
use clap::Parser;
use litert_test::utils::windowing::SlidingAudioWindow;
use std::sync::mpsc;
use std::thread;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    model_path: String,
}

struct SegmentationModel {
    model: litert::Model,
    env: litert::Environment,
    compiled_model: litert::CompiledModel,
}

impl SegmentationModel {
    fn initialize(model_path: &str) -> Result<SegmentationModel, litert::Error> {
        let env = litert::EnvironmentBuilder::build_default()?;
        let m = litert::Model::create_model_from_file(&env, model_path)?;
        let options = litert::Options::create_with_accelerator(litert::LiteRtHwAccelerator::Cpu)?;
        let cm = litert::CompiledModel::create(&env, &m, &options)?;
        Ok(SegmentationModel {
            model: m,
            env: env,
            compiled_model: cm,
        })
    }

    fn run(&self, input: &[f32], output: &mut [f32]) -> Result<(), litert::Error> {
        let sig_index = litert::SignatureIndex::from(0);
        let inputs =
            self.compiled_model
                .create_input_tensor_buffers(&self.env, &self.model, sig_index)?;
        inputs[0].write(input)?;
        let outputs =
            self.compiled_model
                .create_output_tensor_buffers(&self.env, &self.model, sig_index)?;
        self.compiled_model.run(sig_index, &inputs, &outputs)?;
        outputs[0].read(output)?;
        Ok(())
    }

    fn print_signatures(&self) {
        for s in self
            .model
            .signatures()
            .expect("Failed to get signature iterator")
        {
            let signature = s.expect("Failed to get signature");
            println!("Signature key: {:?}", signature.key());
            for input_name in signature.input_names().expect("Failed to get input names") {
                println!("Input name: {input_name:?}");
            }
            for output_name in signature
                .output_names()
                .expect("Failed to get output names")
            {
                println!("Output name: {output_name:?}");
            }
        }
    }
}

fn main() {
    let args = Args::parse();

    // Constants for windowing
    const SAMPLE_RATE: usize = 48_000;
    const WINDOW_DURATION_SECS: f32 = 3.0;
    const OVERLAP_DURATION_SECS: f32 = 1.0;

    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Spawn inference thread
    let model_path = args.model_path.clone();
    let inference_thread = thread::spawn(move || {
        let model = SegmentationModel::initialize(&model_path).expect("Failed to initialize model");
        println!("Model initialized successfully in inference thread!");

        model.print_signatures();

        let mut preds_buffer: [f32; 6522] = [0.; 6522];
        while let Ok(window) = rx.recv() {
            if let Err(e) = model.run(&window, &mut preds_buffer) {
                eprintln!("Inference error: {:?}", e);
                continue;
            }

            if let Some(argmax) = preds_buffer
                .iter()
                .filter(|x| !x.is_nan())
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
            {
                println!(
                    "Predict class {} with probability: {:.4}",
                    argmax, preds_buffer[argmax]
                );
            }
        }
    });

    // Audio capture and windowing in main thread
    let mut window_processor =
        SlidingAudioWindow::new(SAMPLE_RATE, WINDOW_DURATION_SECS, OVERLAP_DURATION_SECS);
    let iterator =
        audio_capture::read_microphone_audio_with_config(audio_capture::AudioCaptureConfig {
            device_id: Some(String::from("alsa:default")),
            channels: 1,
            sample_rate: 48000,
            buffer_size: None,
            sample_format: String::from("F32"),
            start: true,
            duration: 10,
            record_file: Some(String::from("output.wav")),
        })
        .expect("Failed to start audio capture");

    println!("Starting audio capture and windowing...");
    for frame in iterator {
        if let Some(window) = window_processor.process_chunk(frame) {
            if tx.send(window).is_err() {
                break; // Inference thread dropped
            }
        }
    }

    // Wait for inference thread to finish (if it ever does)
    let _ = inference_thread.join();
}
