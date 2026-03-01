use clap::Parser;
use sir_optimizations::Optimizer;
use sir_parser::{EmitConfig, parse_or_panic};
use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
};

fn parse_optimization_passes(s: &str) -> Result<String, String> {
    for c in s.chars() {
        if !matches!(c, 's' | 'c' | 'u' | 'd') {
            return Err(format!(
                "invalid optimization pass '{}', valid passes: s (SCCP), c (copy propagation), u (unused elimination), d (defragment)",
                c
            ));
        }
    }
    Ok(s.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputSelection {
    InitCode,
    Runtime,
    Both,
}

fn resolve_output_selection(
    init_only: bool,
    runtime_only: bool,
) -> Result<OutputSelection, String> {
    match (init_only, runtime_only) {
        (true, true) => {
            Err("conflicting flags: --init-only and --runtime-only cannot be used together"
                .to_string())
        }
        (true, false) => Ok(OutputSelection::InitCode),
        (false, true) => Ok(OutputSelection::Runtime),
        (false, false) => Ok(OutputSelection::Both),
    }
}

#[derive(Parser)]
#[command(name = "sir")]
#[command(about = "Sensei IR to EVM bytecode compiler", long_about = None)]
#[command(version)]
struct Cli {
    /// Input file (use '-' or omit for stdin)
    input: Option<PathBuf>,

    /// Output only initcode (constructor), without runtime section
    #[arg(long)]
    init_only: bool,

    /// Output only runtime code section
    #[arg(long)]
    runtime_only: bool,

    /// Override init function name
    #[arg(long, default_value = "init")]
    init_name: String,

    /// Override main function name
    #[arg(long, default_value = "main")]
    main_name: String,

    /// Optimization passes to run in order. Each character is a pass:
    /// s = SCCP (constant propagation),
    /// c = copy propagation,
    /// u = unused operation elimination,
    /// d = defragment.
    /// Example: -O csud
    #[arg(short = 'O', long = "optimize", value_parser = parse_optimization_passes)]
    optimize: Option<String>,
}

fn read_input(input: Option<PathBuf>) -> String {
    let use_stdin = match &input {
        None => true,
        Some(path) => path.to_str() == Some("-"),
    };

    if use_stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer).expect("stdin read to succeed");
        buffer
    } else {
        let path = input.unwrap();
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read file '{}': {}", path.display(), e))
    }
}

fn print_hex(bytes: &[u8]) {
    print!("0x");
    for byte in bytes {
        print!("{:02x}", byte);
    }
    println!();
}

fn main() {
    let cli = Cli::parse();

    let output_selection = resolve_output_selection(cli.init_only, cli.runtime_only)
        .unwrap_or_else(|msg| {
            eprintln!("error: {msg}");
            std::process::exit(2);
        });

    // Read input source
    let source = read_input(cli.input);

    // Build emit configuration
    let config = if matches!(output_selection, OutputSelection::InitCode) {
        EmitConfig::init_only_with_name(&cli.init_name)
    } else {
        EmitConfig::new(&cli.init_name, &cli.main_name)
    };

    // Parse IR to EthIRProgram
    let mut program = parse_or_panic(&source, config);

    if let Some(passes) = cli.optimize {
        let mut optimizer = Optimizer::new(program);
        optimizer.run_passes(&passes);
        program = optimizer.finish();
    }

    let mut bytecode = Vec::with_capacity(0x6000);
    let offsets = sir_debug_backend::ir_to_bytecode_with_offsets(&program, &mut bytecode);

    let output = match output_selection {
        OutputSelection::InitCode => &bytecode[..offsets.runtime_start],
        OutputSelection::Runtime => &bytecode[offsets.runtime_start..offsets.initcode_end],
        OutputSelection::Both => &bytecode[..offsets.initcode_end],
    };

    print_hex(output);
}

#[cfg(test)]
mod tests {
    use super::{OutputSelection, resolve_output_selection};

    #[test]
    fn output_selection_all_combinations() {
        assert_eq!(resolve_output_selection(false, false).unwrap(), OutputSelection::Both);
        assert_eq!(resolve_output_selection(true, false).unwrap(), OutputSelection::InitCode);
        assert_eq!(resolve_output_selection(false, true).unwrap(), OutputSelection::Runtime);
        assert!(resolve_output_selection(true, true).is_err());
    }
}
