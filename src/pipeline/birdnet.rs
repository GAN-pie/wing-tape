use crate::pipeline::{AudioWindow, InferenceEngine, Prediction};
use std::time::Instant;

pub struct BirdNetClassifier {
    model: litert::Model,
    env: litert::Environment,
    compiled_model: litert::CompiledModel,
    preds_buffer: Vec<f32>,
}

// SAFETY: We are moving the classifier into a single dedicated thread in the InferenceActor.
// The underlying LiteRt C objects are used only within that thread, so manual Send implementation is safe.
unsafe impl Send for BirdNetClassifier {}

impl BirdNetClassifier {
    pub fn new(model_path: &str) -> Result<Self, String> {
        let env = litert::EnvironmentBuilder::build_default()
            .map_err(|e| format!("Failed to build litert environment: {:?}", e))?;
        let m = litert::Model::create_model_from_file(&env, model_path)
            .map_err(|e| format!("Failed to create model: {:?}", e))?;
        let options = litert::Options::create_with_accelerator(litert::LiteRtHwAccelerator::Cpu)
            .map_err(|e| format!("Failed to create options: {:?}", e))?;
        let cm = litert::CompiledModel::create(&env, &m, &options)
            .map_err(|e| format!("Failed to compile model: {:?}", e))?;

        // We'll use a fixed size buffer for predictions.
        // Based on birdnet-test.rs, it seems to be 6522.
        // However, to be more robust, we could check signatures,
        // but for now we follow the test's lead.
        Ok(Self {
            model: m,
            env,
            compiled_model: cm,
            preds_buffer: vec![0.0; 6522],
        })
    }
}

impl InferenceEngine for BirdNetClassifier {
    fn predict(&mut self, window: &AudioWindow) -> Result<Prediction, String> {
        let sig_index = litert::SignatureIndex::from(0);

        let inputs = self
            .compiled_model
            .create_input_tensor_buffers(&self.env, &self.model, sig_index)
            .map_err(|e| format!("Failed to create input buffers: {:?}", e))?;

        inputs[0]
            .write(&window.data)
            .map_err(|e| format!("Failed to write input: {:?}", e))?;

        let outputs = self
            .compiled_model
            .create_output_tensor_buffers(&self.env, &self.model, sig_index)
            .map_err(|e| format!("Failed to create output buffers: {:?}", e))?;

        self.compiled_model
            .run(sig_index, &inputs, &outputs)
            .map_err(|e| format!("Failed to run model: {:?}", e))?;

        outputs[0]
            .read(&mut self.preds_buffer)
            .map_err(|e| format!("Failed to read output: {:?}", e))?;

        // Argmax logic
        if let Some(argmax) = self
            .preds_buffer
            .iter()
            .filter(|x| !x.is_nan())
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
        {
            Ok(Prediction {
                class_id: argmax,
                probability: self.preds_buffer[argmax],
                timestamp: Instant::now(),
            })
        } else {
            Err("Failed to find valid prediction in output buffer".to_string())
        }
    }
}
