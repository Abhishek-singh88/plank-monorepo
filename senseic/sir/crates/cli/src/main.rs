use clap::{ArgAction, CommandFactory, Parser, ValueEnum, error::ErrorKind};
use sir_optimizations::{Optimizer, parse_passes_string};
use sir_parser::{EmitConfig, parse_or_panic};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputSelection {
    InitCode,
    Runtime,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OutputTarget {
    Initcode,
    Runtimecode,
}

fn resolve_output_selection(
    init_only: bool,
    output_targets: &[OutputTarget],
) -> Result<OutputSelection, clap::Error> {
    let selected: BTreeSet<_> = output_targets.iter().copied().collect();

    if init_only && selected.contains(&OutputTarget::Runtimecode) {
        return Err(Cli::command().error(
            ErrorKind::ArgumentConflict,
            "--init-only cannot be combined with --output runtimecode",
        ));
    }

    let want_init = selected.contains(&OutputTarget::Initcode);
    let want_runtime = selected.contains(&OutputTarget::Runtimecode);

    let selection = match (want_init, want_runtime) {
        (false, false) => OutputSelection::Both,
        (true, false) => OutputSelection::InitCode,
        (false, true) => OutputSelection::Runtime,
        (true, true) => OutputSelection::Both,
    };

    Ok(selection)
}

#[derive(Parser)]
#[command(name = "sir")]
#[command(about = "Sensei IR to EVM bytecode compiler", long_about = None)]
#[command(version)]
struct Cli {
    /// Input file (use '-' or omit for stdin)
    input: Option<PathBuf>,

    /// Compile only init function
    #[arg(long)]
    init_only: bool,

    /// Output selection. Repeat to combine modes, e.g.:
    /// --output initcode --output runtimecode
    #[arg(long = "output", value_enum, action = ArgAction::Append)]
    output: Vec<OutputTarget>,

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
    #[arg(short = 'O', long = "optimize", value_parser = parse_passes_string)]
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

fn print_named_hex(name: &str, bytes: &[u8]) {
    print!("{name}: 0x");
    for byte in bytes {
        print!("{:02x}", byte);
    }
    println!();
}

fn print_empty_warning(name: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        println!("warning: {name} output is empty");
    }
}

fn main() {
    let cli = Cli::parse();

    let output_selection =
        resolve_output_selection(cli.init_only, &cli.output).unwrap_or_else(|e| e.exit());

    // Read input source
    let source = read_input(cli.input);

    // Build emit configuration
    let config = if cli.init_only {
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

    let initcode = &bytecode[..offsets.initcode_end];
    let runtimecode = &bytecode[offsets.runtime_start..offsets.initcode_end];

    match output_selection {
        OutputSelection::InitCode => {
            print_empty_warning("initcode", initcode);
            print_hex(initcode);
        }
        OutputSelection::Runtime => {
            print_empty_warning("runtimecode", runtimecode);
            print_hex(runtimecode);
        }
        OutputSelection::Both => {
            print_empty_warning("initcode", initcode);
            print_empty_warning("runtimecode", runtimecode);
            print_named_hex("initcode", initcode);
            print_named_hex("runtimecode", runtimecode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputSelection, OutputTarget, resolve_output_selection};

    #[test]
    fn output_selection_all_combinations() {
        assert_eq!(resolve_output_selection(false, &[]).unwrap(), OutputSelection::Both);
        assert_eq!(
            resolve_output_selection(false, &[OutputTarget::Initcode]).unwrap(),
            OutputSelection::InitCode
        );
        assert_eq!(
            resolve_output_selection(false, &[OutputTarget::Runtimecode]).unwrap(),
            OutputSelection::Runtime
        );
        assert_eq!(
            resolve_output_selection(false, &[OutputTarget::Initcode, OutputTarget::Runtimecode])
                .unwrap(),
            OutputSelection::Both
        );
        assert_eq!(resolve_output_selection(true, &[]).unwrap(), OutputSelection::Both);
        assert!(resolve_output_selection(true, &[OutputTarget::Runtimecode]).is_err());
    }
}
