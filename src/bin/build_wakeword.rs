use rustpotter::Wakeword;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: build-wakeword <output.rpw> <sample1.wav> [sample2.wav ...]");
        std::process::exit(1);
    }

    let output = &args[1];
    let samples: Vec<String> = args[2..].to_vec();

    println!("Building wake word model from {} samples...", samples.len());
    for s in &samples {
        println!("  {}", s);
    }

    let wakeword = Wakeword::new_from_sample_files(
        "roger".to_string(),
        None,
        None,
        samples,
    )
    .unwrap_or_else(|e| {
        eprintln!("Error building model: {}", e);
        std::process::exit(1);
    });

    wakeword
        .save_to_file(output)
        .unwrap_or_else(|e| {
            eprintln!("Error saving model: {}", e);
            std::process::exit(1);
        });

    println!("Saved to {}", output);
}
