use clap::Parser;

use audio_capture;

#[derive(Parser, Debug)]
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
        println!("Input type: {:?}", inputs[0].element_type());
        println!("Input dimensions: {:?}", inputs[0].dimensions());
        inputs[0].write(input)?;
        let outputs =
            self.compiled_model
                .create_output_tensor_buffers(&self.env, &self.model, sig_index)?;
        println!("Output type: {:?}", outputs[0].element_type());
        println!("Output dimensions: {:?}", outputs[0].dimensions());
        self.compiled_model.run(sig_index, &inputs, &outputs)?;
        outputs[0].read(output)?;
        Ok(())
    }
}

fn main() {
    let args = Args::parse();
    let model = SegmentationModel::initialize(args.model_path.as_str())
        .expect("Failed to initialize model");
    println!("Model initialized successfully!");

    for s in model
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

    let inputs = model
        .compiled_model
        .create_input_tensor_buffers(&model.env, &model.model, litert::SignatureIndex::from(0))
        .expect("Failed to get buffer");
    println!("Input type: {:?}", inputs[0].element_type());
    println!("Input dimensions: {:?}", inputs[0].dimensions());

    let outputs = model
        .compiled_model
        .create_output_tensor_buffers(&model.env, &model.model, litert::SignatureIndex::from(0))
        .expect("Failed to get buffer");
    println!("Output type: {:?}", outputs[0].element_type());
    println!("Output dimensions: {:?}", outputs[0].dimensions());

    const WINDOW_DURATION: usize = 3;
    const SAMPLE_RATE: usize = 48_000;
    let window_size = WINDOW_DURATION * SAMPLE_RATE;

    let mut iterator =
        audio_capture::read_microphone_audio().expect("Failed to start audio capture");
    let mut total_frames = 0;
    let mut audio_buffer: Vec<f32> = Vec::with_capacity(window_size);
    let mut preds_buffer: [f32; 6522] = [0.; 6522];
    loop {
        if let Some(frame) = iterator.next() {
            total_frames += frame.len();
            audio_buffer.extend(frame);
            if total_frames >= window_size {
                inputs[0]
                    .write(&audio_buffer[..window_size])
                    .expect("Failed to write to buffer");
                audio_buffer.drain(0..SAMPLE_RATE); // we want a 1 second length overlap
                total_frames = audio_buffer.len();
            }
            outputs[0]
                .read(&mut preds_buffer)
                .expect("Failed to read buffer");
            if let Some(argmax) = preds_buffer
                .iter()
                .filter(|x| !x.is_nan())
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
            {
                println!(
                    "Predict class {} with probability: {}",
                    argmax, preds_buffer[argmax]
                );
            }
        }
    }
}
