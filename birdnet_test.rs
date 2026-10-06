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
        Ok(SegmentationModel { model: m, env: env, compiled_model: cm })
    }
    fn run(&self, input: &[f32], output: &mut [f32]) -> Result<(), litert::Error> {
        let sig_index = litert::SignatureIndex::from(0);
        let inputs =
            self.compiled_model.create_input_tensor_buffers(&self.env, &self.model, sig_index)?;
        println!("Input type: {:?}", inputs[0].element_type());
        println!("Input dimensions: {:?}", inputs[0].dimensions());
        inputs[0].write(input)?;
        let outputs =
            self.compiled_model.create_output_tensor_buffers(&self.env, &self.model, sig_index)?;
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

    for s in model.model.signatures().expect("Failed to get signature iterator") {
        let signature = s.expect("Failed to get signature");
        println!("Signature key: {:?}", signature.key());
        for input_name in signature.input_names().expect("Failed to get input names") {
            println!("Input name: {input_name:?}");
        }
        for output_name in signature.output_names().expect("Failed to get output names") {
            println!("Output name: {output_name:?}");
        }
    }

    let inputs = model.compiled_model
        .create_input_tensor_buffers(&model.env, &model.model, litert::SignatureIndex::from(0)).expect("Failed to get buffer");
    println!("Input type: {:?}", inputs[0].element_type());
    println!("Input dimensions: {:?}", inputs[0].dimensions());

    let outputs = model.compiled_model
        .create_output_tensor_buffers(&model.env, &model.model, litert::SignatureIndex::from(0)).expect("Failed to get buffer");
    println!("Output type: {:?}", outputs[0].element_type());
    println!("Output dimensions: {:?}", outputs[0].dimensions());

    let mut iterator = audio_capture::read_microphone_audio().expect("Failed to start audio capture");
    loop {
        if let Some(frame) = iterator.next() {
            inputs[0].write(frame.as_slice()).expect("Failed to write to buffer");
        }
    }
}
