use clio::*;

#[cfg(feature = "clap-parse")]
use clap::Parser;
#[cfg(feature = "clap-parse")]
#[derive(Parser)]
#[clap(name = "cat")]
struct Opt {
    /// Input file, use '-' for stdin
    #[clap(value_parser, default_value = "-")]
    input: Input,

    /// Output file '-' for stdout
    #[clap(long, short, value_parser, default_value = "-")]
    output: Output,
}

#[cfg(feature = "clap-parse")]
fn main() {
    let mut opt = Opt::parse();

    std::io::copy(&mut opt.input, &mut opt.output).unwrap();
}

#[cfg(not(feature = "clap-parse"))]
fn main() {
    for arg in std::env::args_os() {
        let mut input = Input::new(&arg).unwrap();
        std::io::copy(&mut input, &mut Output::std()).unwrap();
    }
}
