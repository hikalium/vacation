use std::{fs, io};

use anyhow::anyhow;
use anyhow::Result;
use argh::FromArgs;

#[derive(FromArgs)]
/// VRM as a Code
struct Args {
    /// path to .vrm file to parse
    #[argh(option)]
    input: Option<String>,
}

fn run(path: &str) -> Result<()> {
    let file = fs::File::open(&path)?;
    let reader = io::BufReader::new(file);
    let gltf = gltf::Gltf::from_reader(reader)?;
    println!("{:#?}", gltf);
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    if let Some(path) = args.input {
        run(&path)
    } else {
        Err(anyhow!("Run vacation --help for more information."))
    }
}
